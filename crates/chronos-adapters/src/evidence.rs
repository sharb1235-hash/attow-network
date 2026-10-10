//! Trusted provider port. Evidence bytes are claims until the ledger verifies them.
use crate::gateway::DispatchAuthorization;
use chronos_ledger::{EffectProposal, EvidenceVerifier};

pub const MAX_PROVIDER_EVIDENCE: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderEvidence {
    bytes: Vec<u8>,
}
impl ProviderEvidence {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ProviderError> {
        if bytes.len() > MAX_PROVIDER_EVIDENCE {
            return Err(ProviderError::Limit);
        }
        Ok(Self { bytes })
    }
    pub fn unknown() -> Self {
        Self { bytes: Vec::new() }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("provider transport unavailable or response incomplete")]
    Unavailable,
    #[error("invalid bounded provider protocol message")]
    Protocol,
    #[error("provider authorization does not match the durable request")]
    InvalidAuthorization,
    #[error("provider evidence failed verification")]
    InvalidEvidence,
    #[error("provider idempotency retention expired; retry is forbidden")]
    RetentionExpired,
    #[error("provider fixture resource limit exceeded")]
    Limit,
    #[error("provider fixture storage failed")]
    Storage,
    #[error("provider fixture configuration is invalid")]
    Configuration,
}

/// The gateway alone can construct this authorization. Providers cannot be
/// invoked with an agent-supplied ID or a detached/stale ticket alone.
pub trait EffectProvider: EvidenceVerifier {
    /// Trusted, local preflight before a durable claim or any provider send.
    /// Validate the entire body against reserved rights and the provider's
    /// concrete contract. Unknown provider semantics deny dispatch by default.
    fn preflight(&self, _proposal: &EffectProposal) -> Result<(), ProviderError> {
        Err(ProviderError::InvalidAuthorization)
    }
    fn dispatch(
        &mut self,
        authorization: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError>;
    fn reconcile(
        &mut self,
        authorization: &DispatchAuthorization<'_>,
    ) -> Result<ProviderEvidence, ProviderError>;
}
