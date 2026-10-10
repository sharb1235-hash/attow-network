use crate::reducer::{TxReader, append, apply};
use crate::*;

/// Public, credential-free writer state. A checkpoint is data, not activation authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityCheckpoint {
    pub domain: DomainId,
    pub writer: Option<ObjectId>,
    pub epoch: u64,
    pub head: ObjectId,
    /// Some means this epoch is permanently closed to all writes and dispatch.
    pub next_writer: Option<ObjectId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceRequest {
    pub checkpoint: AuthorityCheckpoint,
    pub writer: ObjectId,
    pub epoch: u64,
}
/// Trusted deployment port, outside exported databases. Implementations must
/// authenticate the writer using credentials held outside the snapshot, retain
/// the latest anchored checkpoint durably, and grant at most one live session
/// per domain even when identity/credentials are copied. No timeout or partition
/// may grant another session. Administrative recovery must actually fence any
/// former dispatcher; a file path, signed identity, or caller boolean is not proof.
/// Initial enrollment requires explicit administrative authorization of the
/// local checkpoint. Resume must match its designated writer and epoch. Handoff
/// requires the exact closed checkpoint, its named successor, and epoch+1.
///
/// The crate provides no distributed fencing service. A deployment without one
/// must keep authorities local and cannot enable remote promotion.
pub trait WriterFence: Send + Sync {
    fn acquire(&self, request: &FenceRequest) -> Result<Box<dyn WriterLease>, LedgerError>;
}
/// Exclusive, non-cloneable external session. It must remain exclusive until
/// dropped or the old process is demonstrably fenced. In-flight provider work
/// can survive process fencing and is therefore preserved as Unknown.
///
/// `check` fails closed when service status is unknown. `advance` must atomically
/// compare the old checkpoint and durably anchor the new one before success.
/// If SQLite commits but this call fails, the authority stops: reconciling the
/// two durable stores is an external administrative recovery task. Implementors
/// must never accept an older head simply because it has the right writer ID.
pub trait WriterLease: Send + Sync {
    fn check(&self, checkpoint: &AuthorityCheckpoint) -> Result<(), LedgerError>;
    fn advance(
        &self,
        previous: &AuthorityCheckpoint,
        next: &AuthorityCheckpoint,
    ) -> Result<(), LedgerError>;
}
pub(crate) struct FenceSession {
    lease: Box<dyn WriterLease>,
    checkpoint: AuthorityCheckpoint,
}
pub(crate) fn checkpoint(model: &schema::Model) -> AuthorityCheckpoint {
    AuthorityCheckpoint {
        domain: model.root.domain(),
        writer: model.writer,
        epoch: model.epoch,
        head: model.head,
        next_writer: model.next_writer,
    }
}
/// Validate inside the same SQLite transaction as mutation. This closes a race
/// with an already-open local handle while federation is being initialized.
pub(crate) fn check_binding(
    model: &schema::Model,
    expected: Option<AuthorityCheckpoint>,
) -> Result<(), LedgerError> {
    match expected {
        None if model.writer.is_none() && model.next_writer.is_none() => Ok(()),
        Some(expected) if expected == checkpoint(model) && model.next_writer.is_none() => Ok(()),
        _ => Err(LedgerError::Fenced),
    }
}
impl Authority {
    pub fn checkpoint(&self) -> Result<AuthorityCheckpoint, LedgerError> {
        self.store
            .read_transaction_with(|tx| Ok(checkpoint(&schema::load(tx.sql())?)))
    }
    /// Use before accepting remote proposals. Unlike `checkpoint`, this verifies
    /// a live exclusive external lease and the exact anchored database head.
    pub fn require_federated_writer(&self) -> Result<AuthorityCheckpoint, LedgerError> {
        self.require_writable()?;
        let session = self.fence.as_ref().ok_or(LedgerError::ActivationRequired)?;
        Ok(session.checkpoint)
    }
    pub(crate) fn write_binding(&self) -> Option<AuthorityCheckpoint> {
        self.fence.as_ref().map(|session| session.checkpoint)
    }
    pub(crate) fn check_fence(&self) -> Result<(), LedgerError> {
        let current = self.checkpoint()?;
        match &self.fence {
            None if current.writer.is_none() && current.next_writer.is_none() => Ok(()),
            Some(session)
                if current == session.checkpoint
                    && current.epoch == self.epoch
                    && current.next_writer.is_none() =>
            {
                session.lease.check(&current)
            }
            _ => Err(LedgerError::Fenced),
        }
    }
    /// Do not hand out a successful result or dispatch ticket before the external
    /// checkpoint has durably advanced. Failed anchoring permanently closes this handle.
    pub(crate) fn finish_write(&mut self) -> Result<(), LedgerError> {
        if self.fence.is_none() {
            return Ok(());
        }
        let current = self.checkpoint()?;
        let session = self.fence.as_mut().ok_or(LedgerError::Fenced)?;
        if let Err(error) = session.lease.advance(&session.checkpoint, &current) {
            self.read_only = true;
            return Err(error);
        }
        session.checkpoint = current;
        Ok(())
    }
    /// Explicit administrator opt-in. The external port must authorize taking
    /// this exact local history into exclusive federated custody.
    pub fn initialize_federation(
        &mut self,
        writer: ObjectId,
        fence: &dyn WriterFence,
    ) -> Result<(), LedgerError> {
        self.require_writable()?;
        if self.fence.is_some() {
            return Err(LedgerError::Fenced);
        }
        self.rebuild_and_verify()?;
        let before = self.checkpoint()?;
        let request = FenceRequest {
            checkpoint: before,
            writer,
            epoch: before.epoch,
        };
        let lease = fence.acquire(&request)?;
        lease.check(&before)?;
        self.fence = Some(FenceSession {
            lease,
            checkpoint: before,
        });
        let result = self.store.transaction_with(|tx| {
            let mut model = schema::load(tx.sql())?;
            if checkpoint(&model) != before {
                return Err(LedgerError::Fenced);
            }
            let command = Value::Array(vec![Value::Unsigned(9), v_id(writer)]);
            apply(&mut model, &command, &TxReader(tx))?;
            append(tx, &mut model, command)
        });
        if let Err(error) = result {
            self.fence = None;
            return Err(error);
        }
        self.finish_write()
    }
    /// Resume only with credentials and the current externally anchored head.
    /// A copied data file alone cannot activate this API.
    pub fn open_federated(
        path: &Path,
        writer: ObjectId,
        fence: &dyn WriterFence,
    ) -> Result<Self, LedgerError> {
        Self::open_fenced(path, writer, fence, false)
    }
    /// Explicitly close admission and dispatch. Already dispatched work remains
    /// Unknown with its original provider operation identity and reservation.
    /// After the close is externally anchored the exclusive session is released.
    pub fn close_epoch(
        &mut self,
        next_writer: ObjectId,
    ) -> Result<AuthorityCheckpoint, LedgerError> {
        let before = self.require_federated_writer()?;
        if before.writer == Some(next_writer) {
            return Err(LedgerError::InvalidRequest);
        }
        self.store.transaction_with(|tx| {
            let mut model = schema::load(tx.sql())?;
            check_binding(&model, Some(before))?;
            let command = Value::Array(vec![Value::Unsigned(10), v_id(next_writer)]);
            apply(&mut model, &command, &TxReader(tx))?;
            append(tx, &mut model, command)
        })?;
        self.finish_write()?;
        let closed = self.checkpoint()?;
        self.read_only = true;
        self.fence = None;
        Ok(closed)
    }
    /// Activate only the named successor, at exactly epoch+1, from the closed
    /// checkpoint accepted by the external fencer. This never contacts providers.
    pub fn activate_handoff(
        path: &Path,
        writer: ObjectId,
        fence: &dyn WriterFence,
    ) -> Result<Self, LedgerError> {
        Self::open_fenced(path, writer, fence, true)
    }
    fn open_fenced(
        path: &Path,
        writer: ObjectId,
        fence: &dyn WriterFence,
        activate: bool,
    ) -> Result<Self, LedgerError> {
        let inspected = Self::inspect(path)?;
        inspected.rebuild_and_verify()?;
        let before = inspected.checkpoint()?;
        let epoch = if activate {
            if before.writer.is_none() || before.next_writer != Some(writer) {
                return Err(LedgerError::ActivationRequired);
            }
            before.epoch.checked_add(1).ok_or(LedgerError::Limit)?
        } else {
            if before.writer != Some(writer) || before.next_writer.is_some() {
                return Err(LedgerError::ActivationRequired);
            }
            before.epoch
        };
        integer(epoch)?;
        let lease = fence.acquire(&FenceRequest {
            checkpoint: before,
            writer,
            epoch,
        })?;
        lease.check(&before)?;
        let mut authority = Self {
            store: Store::open(path)?,
            domain: before.domain,
            epoch,
            hook: None,
            read_only: false,
            fence: Some(FenceSession {
                lease,
                checkpoint: before,
            }),
        };
        if authority.checkpoint()? != before {
            return Err(LedgerError::Fenced);
        }
        if activate {
            let location = path.canonicalize()?.to_string_lossy().into_owned();
            authority.store.transaction_with(|tx| {
                let mut model = schema::load(tx.sql())?;
                if checkpoint(&model) != before {
                    return Err(LedgerError::Fenced);
                }
                let command = Value::Array(vec![
                    Value::Unsigned(11),
                    v_id(writer),
                    Value::Unsigned(epoch),
                    Value::Text(location),
                ]);
                apply(&mut model, &command, &TxReader(tx))?;
                append(tx, &mut model, command)
            })?;
            authority.finish_write()?;
        } else {
            authority.recover_dispatching()?;
        }
        authority.require_federated_writer()?;
        Ok(authority)
    }
}
