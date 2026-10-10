use crate::reducer::{TxReader, append, apply};
use crate::*;
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, object_id};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceStatus {
    Confirmed,
    DefinitivelyRejected,
    Unknown,
}
impl EvidenceStatus {
    pub(crate) fn code(self) -> u64 {
        match self {
            Self::Confirmed => 3,
            Self::DefinitivelyRejected => 4,
            Self::Unknown => 2,
        }
    }
}

/// Implemented only by trusted adapter code selected by the host's deployment
/// configuration. Agent text cannot construct a verifier or VerifiedEvidence.
pub trait EvidenceVerifier: Send + Sync {
    fn contract_id(&self) -> ObjectId;
    fn verify(
        &self,
        ticket: &DispatchTicket,
        receipt: &[u8],
    ) -> Result<EvidenceStatus, LedgerError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchTicket {
    pub(crate) domain: DomainId,
    pub(crate) intent: ObjectId,
    pub(crate) operation: ObjectId,
    pub(crate) contract: ObjectId,
    pub(crate) payload: ObjectId,
    pub(crate) provider: String,
    pub(crate) attempt: u64,
    pub(crate) epoch: u64,
    pub(crate) dispatch: bool,
}
impl DispatchTicket {
    pub fn intent(&self) -> ObjectId {
        self.intent
    }
    pub fn operation(&self) -> ObjectId {
        self.operation
    }
    pub fn contract(&self) -> ObjectId {
        self.contract
    }
    pub fn payload(&self) -> ObjectId {
        self.payload
    }
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn attempt(&self) -> u64 {
        self.attempt
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn may_dispatch(&self) -> bool {
        self.dispatch
    }
}
pub struct VerifiedEvidence {
    ticket: DispatchTicket,
    status: EvidenceStatus,
    bytes: Vec<u8>,
}
pub fn verify_evidence(
    verifier: &dyn EvidenceVerifier,
    ticket: &DispatchTicket,
    receipt: &[u8],
) -> Result<VerifiedEvidence, LedgerError> {
    if verifier.contract_id() != ticket.contract || receipt.len() > 64 * 1024 {
        return Err(LedgerError::InvalidEvidence);
    }
    let status = verifier.verify(ticket, receipt)?;
    let bytes = Object {
        refs: BTreeSet::new(),
        payload: Value::Array(vec![
            Value::Text("attow-evidence".into()),
            Value::Unsigned(1),
            v_id(ticket.intent),
            v_id(ticket.contract),
            v_id(ticket.operation),
            v_id(ticket.payload),
            Value::Unsigned(ticket.attempt),
            Value::Unsigned(ticket.epoch),
            Value::Unsigned(status.code()),
            Value::Bytes(receipt.to_vec()),
        ]),
    }
    .encode()?;
    Ok(VerifiedEvidence {
        ticket: ticket.clone(),
        status,
        bytes,
    })
}
pub(crate) fn ticket(
    domain: DomainId,
    id: ObjectId,
    intent: &schema::Intent,
    dispatch: bool,
) -> DispatchTicket {
    DispatchTicket {
        domain,
        intent: id,
        operation: intent.proposal.operation,
        contract: intent.proposal.contract,
        payload: object_id(0x4244, 1, &intent.proposal.body),
        provider: intent.proposal.provider.clone(),
        attempt: intent.attempt,
        epoch: intent.epoch,
        dispatch,
    }
}
pub(crate) fn check_receipt(
    bytes: &[u8],
    id: ObjectId,
    intent: &schema::Intent,
    status: u64,
) -> Result<(), LedgerError> {
    let object = Object::decode(bytes, &Limits::default())?;
    if !object.refs.is_empty() {
        return Err(LedgerError::InvalidEvidence);
    }
    let mut f = fixed(object.payload, 10)?;
    if text_value(take(&mut f)?)? != "attow-evidence"
        || num(take(&mut f)?)? != 1
        || crate::id(take(&mut f)?)? != id
        || crate::id(take(&mut f)?)? != intent.proposal.contract
        || crate::id(take(&mut f)?)? != intent.proposal.operation
        || crate::id(take(&mut f)?)? != object_id(0x4244, 1, &intent.proposal.body)
        || num(take(&mut f)?)? != intent.attempt
        || num(take(&mut f)?)? != intent.epoch
        || num(take(&mut f)?)? != status
        || blob(take(&mut f)?)?.len() > 64 * 1024
    {
        return Err(LedgerError::InvalidEvidence);
    }
    Ok(())
}
impl Authority {
    pub fn claim_effect(&mut self, id: ObjectId) -> Result<DispatchTicket, LedgerError> {
        self.require_writable()?;
        let hook = self.hook;
        let result = self.store.transaction_with(|tx| {
            let mut model = schema::load(tx.sql())?;
            let command = Value::Array(vec![Value::Unsigned(5), v_id(id)]);
            apply(&mut model, &command, &TxReader(tx))?;
            append(tx, &mut model, command)?;
            let ticket = ticket(
                model.root.domain(),
                id,
                model.intents.get(&id).ok_or(LedgerError::Missing)?,
                true,
            );
            if let Some(hook) = hook {
                hook(TransitionBoundary::BeforeClaimCommit);
            }
            Ok::<_, LedgerError>(ticket)
        })?;
        if let Some(hook) = hook {
            hook(TransitionBoundary::AfterClaimCommit);
        }
        Ok(result)
    }
    pub fn reconciliation_ticket(&self, id: ObjectId) -> Result<DispatchTicket, LedgerError> {
        self.store.read_transaction_with(|tx| {
            let model = schema::load(tx.sql())?;
            let intent = model.intents.get(&id).ok_or(LedgerError::Missing)?;
            if intent.state != EffectState::Unknown {
                return Err(LedgerError::IllegalTransition);
            }
            Ok(ticket(model.root.domain(), id, intent, false))
        })
    }
    pub fn record_outcome(
        &mut self,
        ticket: DispatchTicket,
        evidence: VerifiedEvidence,
    ) -> Result<(), LedgerError> {
        self.require_writable()?;
        if ticket != evidence.ticket {
            return Err(LedgerError::InvalidEvidence);
        }
        let hook = self.hook;
        self.store.transaction_with(|tx| {
            let mut model = schema::load(tx.sql())?;
            let intent = model
                .intents
                .get(&ticket.intent)
                .ok_or(LedgerError::Missing)?;
            if ticket != self::ticket(model.root.domain(), ticket.intent, intent, ticket.dispatch) {
                return Err(LedgerError::InvalidEvidence);
            }
            let receipt = object_id(OBJECT_KIND, OBJECT_SCHEMA, &evidence.bytes);
            if model.evidence.iter().any(|previous| {
                previous.intent == ticket.intent
                    && previous.attempt == ticket.attempt
                    && previous.status == evidence.status.code()
                    && previous.receipt == receipt
            }) {
                return Ok(());
            }
            tx.put(OBJECT_KIND, OBJECT_SCHEMA, &evidence.bytes)?;
            let command = Value::Array(vec![
                Value::Unsigned(6),
                v_id(ticket.intent),
                Value::Unsigned(ticket.attempt),
                Value::Unsigned(evidence.status.code()),
                v_id(receipt),
            ]);
            apply(&mut model, &command, &TxReader(tx))?;
            append(tx, &mut model, command)?;
            tx.pin(&format!("ledger:evidence:{}", model.seq), receipt)?;
            if let Some(hook) = hook {
                hook(TransitionBoundary::BeforeOutcomeCommit);
            }
            Ok::<_, LedgerError>(())
        })?;
        if let Some(hook) = hook {
            hook(TransitionBoundary::AfterOutcomeCommit);
        }
        Ok(())
    }
    pub fn cancel_before_send(&mut self, id: ObjectId) -> Result<(), LedgerError> {
        self.require_writable()?;
        let hook = self.hook;
        self.store.transaction_with(|tx| {
            let mut model = schema::load(tx.sql())?;
            let command = Value::Array(vec![Value::Unsigned(7), v_id(id)]);
            apply(&mut model, &command, &TxReader(tx))?;
            append(tx, &mut model, command)?;
            if let Some(hook) = hook {
                hook(TransitionBoundary::BeforeOutcomeCommit);
            }
            Ok::<_, LedgerError>(())
        })?;
        if let Some(hook) = hook {
            hook(TransitionBoundary::AfterOutcomeCommit);
        }
        Ok(())
    }
}
