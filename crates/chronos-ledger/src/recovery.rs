use crate::reducer::{TxReader, append, apply};
use crate::*;

pub(crate) fn open(
    path: &Path,
    hook: Option<fn(TransitionBoundary)>,
) -> Result<Authority, LedgerError> {
    let mut store = Store::open(path)?;
    let location = path.canonicalize()?.to_string_lossy().into_owned();
    store.transaction_with(|tx| {
        tx.sql().execute_batch(schema::DDL)?;
        if !schema::exists(tx.sql())? {
            let mut random = [0; 32];
            getrandom::fill(&mut random).map_err(|_| LedgerError::Random)?;
            let domain = DomainId(random);
            let mut state = schema::Model::new(domain, location.clone());
            let command = Value::Array(vec![
                Value::Unsigned(0),
                Value::Bytes(domain.0.to_vec()),
                Value::Text(location.clone()),
            ]);
            apply(&mut state, &command, &TxReader(tx))?;
            append(tx, &mut state, command)?;
        }
        let state = schema::load(tx.sql())?;
        if state.location != location || state.writer.is_some() {
            return Err(LedgerError::ActivationRequired);
        }
        Ok::<_, LedgerError>(())
    })?;
    let state = store.read_transaction_with(|tx| schema::load(tx.sql()))?;
    let mut authority = Authority {
        store,
        domain: state.root.domain(),
        epoch: state.epoch,
        hook,
        read_only: false,
        fence: None,
    };
    authority.rebuild_and_verify()?;
    authority.recover_dispatching()?;
    Ok(authority)
}

impl Authority {
    pub(crate) fn recover_dispatching(&mut self) -> Result<(), LedgerError> {
        self.require_writable()?;
        let binding = self.write_binding();
        let hook = self.hook;
        let changed = self.store.transaction_with(|tx| {
            let mut state = schema::load(tx.sql())?;
            federation::check_binding(&state, binding)?;
            let pending: Vec<_> = state
                .intents
                .iter()
                .filter(|(_, intent)| intent.state == EffectState::Dispatching)
                .map(|(id, _)| v_id(*id))
                .collect();
            if pending.is_empty() {
                return Ok(false);
            }
            let command = Value::Array(vec![Value::Unsigned(8), Value::Array(pending)]);
            apply(&mut state, &command, &TxReader(tx))?;
            append(tx, &mut state, command)?;
            if let Some(hook) = hook {
                hook(TransitionBoundary::BeforeRecoveryCommit);
            }
            Ok::<_, LedgerError>(true)
        })?;
        if changed {
            self.finish_write()?;
            if let Some(hook) = hook {
                hook(TransitionBoundary::AfterRecoveryCommit);
            }
        }
        Ok(())
    }
}
