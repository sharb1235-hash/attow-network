//! Local serializable authority and durable effect intents. No provider dispatch.
mod effects;
mod promotion;
mod recovery;
mod reducer;
mod schema;
mod verify;

use chronos_codec::{CodecError, ObjectId, Value};
use chronos_state::{DomainId, FrozenBranch, StateError, StateRoot};
use chronos_store::{Store, StoreError};
use std::{collections::BTreeSet, path::Path};

pub use effects::{
    DispatchTicket, EvidenceStatus, EvidenceVerifier, VerifiedEvidence, verify_evidence,
};
pub use verify::VerificationReport;

pub const MAX_LEDGER_ROWS: usize = 4096;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("invalid bounded ledger request")]
    InvalidRequest,
    #[error("request ID was reused with different content")]
    IdempotencyConflict,
    #[error("illegal effect transition")]
    IllegalTransition,
    #[error("evidence does not match the admitted adapter contract and intent")]
    InvalidEvidence,
    #[error("required ledger record is missing")]
    Missing,
    #[error("ledger data or materialized tables failed verification")]
    Corrupt,
    #[error("ledger resource limit exceeded")]
    Limit,
    #[error(
        "copied or moved authority requires explicit activation; automatic activation is unavailable"
    )]
    ActivationRequired,
    #[error("operating system randomness unavailable")]
    Random,
    #[error("operation aborted")]
    Aborted,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invariant {
    KeyEquals {
        key: Vec<u8>,
        expected: Option<Vec<u8>>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub allowed_contracts: BTreeSet<ObjectId>,
    pub max_effect_cost: u64,
    pub invariants: Vec<Invariant>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectProposal {
    pub contract: ObjectId,
    pub provider: String,
    pub semantic_slot: String,
    pub operation: ObjectId,
    pub right: String,
    pub cost: u64,
    pub body: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionPayload {
    pub domain: DomainId,
    pub epoch: u64,
    pub policy: ObjectId,
    pub effect: Option<EffectProposal>,
}
#[derive(Clone, Debug)]
pub struct PromotionRequest {
    pub id: ObjectId,
    pub payload: ObjectId,
    pub branch: FrozenBranch,
    pub decision: ObjectId,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectState {
    Authorized,
    Dispatching,
    Unknown,
    Confirmed,
    DefinitivelyRejected,
    CancelledBeforeSend,
}
impl EffectState {
    pub(crate) fn code(self) -> u64 {
        match self {
            Self::Authorized => 0,
            Self::Dispatching => 1,
            Self::Unknown => 2,
            Self::Confirmed => 3,
            Self::DefinitivelyRejected => 4,
            Self::CancelledBeforeSend => 5,
        }
    }
    pub(crate) fn from_code(value: u64) -> Result<Self, LedgerError> {
        match value {
            0 => Ok(Self::Authorized),
            1 => Ok(Self::Dispatching),
            2 => Ok(Self::Unknown),
            3 => Ok(Self::Confirmed),
            4 => Ok(Self::DefinitivelyRejected),
            5 => Ok(Self::CancelledBeforeSend),
            _ => Err(LedgerError::Corrupt),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictReport {
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromotionOutcome {
    LocalCommitted { seq: u64 },
    AcceptedPending { seq: u64, intent: ObjectId },
    Conflict(ConflictReport),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RightBalance {
    pub granted: u64,
    pub reserved: u64,
    pub spent: u64,
    pub available: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionBoundary {
    BeforePromoteCommit,
    AfterPromoteCommit,
    BeforeClaimCommit,
    AfterClaimCommit,
    BeforeOutcomeCommit,
    AfterOutcomeCommit,
    BeforeRecoveryCommit,
    AfterRecoveryCommit,
}

pub struct Authority {
    pub(crate) store: Store,
    domain: DomainId,
    epoch: u64,
    pub(crate) hook: Option<fn(TransitionBoundary)>,
    pub(crate) read_only: bool,
}
impl Authority {
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        Self::open_with_hook(path, None)
    }
    pub fn open_with_hook(
        path: &Path,
        hook: Option<fn(TransitionBoundary)>,
    ) -> Result<Self, LedgerError> {
        recovery::open(path, hook)
    }
    pub fn inspect(path: &Path) -> Result<Self, LedgerError> {
        let store = Store::open_read_only(path)?;
        let state = store.read_transaction_with(|tx| schema::load(tx.sql()))?;
        Ok(Self {
            store,
            domain: state.root.domain(),
            epoch: state.epoch,
            hook: None,
            read_only: true,
        })
    }
    pub fn domain(&self) -> DomainId {
        self.domain
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Explicit instrumentation callback; no ambient environment variable hook.
    pub fn set_boundary_hook(&mut self, hook: Option<fn(TransitionBoundary)>) {
        self.hook = hook;
    }
    pub fn current_root(&self) -> Result<StateRoot, LedgerError> {
        self.store
            .read_transaction_with(|tx| Ok(schema::load(tx.sql())?.root))
    }
    pub fn right_balance(&self, name: &str) -> Result<RightBalance, LedgerError> {
        self.store.read_transaction_with(|tx| {
            let state = schema::load(tx.sql())?;
            let right = state.rights.get(name).ok_or(LedgerError::Missing)?;
            Ok(RightBalance {
                granted: right.granted,
                reserved: right.reserved,
                spent: right.spent,
                available: right.granted - right.reserved - right.spent,
            })
        })
    }
    pub fn effect_state(&self, id: ObjectId) -> Result<EffectState, LedgerError> {
        self.store.read_transaction_with(|tx| {
            schema::load(tx.sql())?
                .intents
                .get(&id)
                .map(|intent| intent.state)
                .ok_or(LedgerError::Missing)
        })
    }
    pub(crate) fn require_writable(&self) -> Result<(), LedgerError> {
        if self.read_only {
            Err(LedgerError::ActivationRequired)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn integer(value: u64) -> Result<i64, LedgerError> {
    i64::try_from(value).map_err(|_| LedgerError::Limit)
}
pub(crate) fn string(value: &str) -> Result<(), LedgerError> {
    if value.is_empty() || value.len() > 256 || value.contains('\0') {
        Err(LedgerError::InvalidRequest)
    } else {
        Ok(())
    }
}
pub(crate) fn list(value: Value) -> Result<Vec<Value>, LedgerError> {
    if let Value::Array(values) = value {
        Ok(values)
    } else {
        Err(LedgerError::Corrupt)
    }
}
pub(crate) fn fixed(value: Value, count: usize) -> Result<std::vec::IntoIter<Value>, LedgerError> {
    let values = list(value)?;
    if values.len() != count {
        return Err(LedgerError::Corrupt);
    }
    Ok(values.into_iter())
}
pub(crate) fn take(values: &mut impl Iterator<Item = Value>) -> Result<Value, LedgerError> {
    values.next().ok_or(LedgerError::Corrupt)
}
pub(crate) fn num(value: Value) -> Result<u64, LedgerError> {
    if let Value::Unsigned(value) = value {
        Ok(value)
    } else {
        Err(LedgerError::Corrupt)
    }
}
pub(crate) fn text_value(value: Value) -> Result<String, LedgerError> {
    if let Value::Text(value) = value {
        Ok(value)
    } else {
        Err(LedgerError::Corrupt)
    }
}
pub(crate) fn blob(value: Value) -> Result<Vec<u8>, LedgerError> {
    if let Value::Bytes(value) = value {
        Ok(value)
    } else {
        Err(LedgerError::Corrupt)
    }
}
pub(crate) fn id(value: Value) -> Result<ObjectId, LedgerError> {
    Ok(ObjectId(
        blob(value)?.try_into().map_err(|_| LedgerError::Corrupt)?,
    ))
}
pub(crate) fn v_id(id: ObjectId) -> Value {
    Value::Bytes(id.0.to_vec())
}
pub(crate) fn option_id(value: Value) -> Result<Option<ObjectId>, LedgerError> {
    match value {
        Value::Null => Ok(None),
        value => Ok(Some(id(value)?)),
    }
}
pub(crate) fn v_option(value: Option<ObjectId>) -> Value {
    value.map_or(Value::Null, v_id)
}
