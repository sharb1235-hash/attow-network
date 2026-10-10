//! The only constructor of provider dispatch authorization lives in this module.
use crate::evidence::{EffectProvider, ProviderError};
use chronos_codec::ObjectId;
use chronos_ledger::{Authority, DispatchTicket, EffectState, LedgerError, verify_evidence};

/// An unforgeable safe-Rust borrow of the active authority, exact durable ticket,
/// and its immutable body. Applications cannot construct or clone this permit.
pub struct DispatchAuthorization<'a> {
    authority: &'a mut Authority,
    ticket: DispatchTicket,
    body: Vec<u8>,
}
impl DispatchAuthorization<'_> {
    pub fn ticket(&self) -> &DispatchTicket {
        &self.ticket
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("provider contract does not match the authorized intent")]
    ProviderMismatch,
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("provider rejected the immutable effect proposal before dispatch: {0}")]
    Preflight(ProviderError),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayResult {
    pub intent: ObjectId,
    pub state: EffectState,
    pub provider_error: Option<ProviderError>,
    /// Durable canonical evidence envelope, including Unknown observations.
    /// Only a Confirmed state permits interpreting it as confirmation.
    pub receipt: ObjectId,
}
pub struct Gateway<'a, P: EffectProvider> {
    authority: &'a mut Authority,
    provider: &'a mut P,
}
impl<'a, P: EffectProvider> Gateway<'a, P> {
    pub fn new(authority: &'a mut Authority, provider: &'a mut P) -> Self {
        Self {
            authority,
            provider,
        }
    }
    pub fn dispatch(&mut self, intent: ObjectId) -> Result<GatewayResult, GatewayError> {
        self.check_contract(intent)?;
        let ticket = self.authority.claim_effect(intent)?;
        self.invoke(ticket)
    }
    pub fn reconcile(&mut self, intent: ObjectId) -> Result<GatewayResult, GatewayError> {
        self.check_contract(intent)?;
        let ticket = self.authority.reconciliation_ticket(intent)?;
        self.invoke(ticket)
    }
    fn check_contract(&self, intent: ObjectId) -> Result<(), GatewayError> {
        let proposal = self.authority.effect_proposal(intent)?;
        if proposal.contract != self.provider.contract_id() {
            return Err(GatewayError::ProviderMismatch);
        }
        self.provider
            .preflight(&proposal)
            .map_err(GatewayError::Preflight)?;
        Ok(())
    }
    fn invoke(&mut self, ticket: DispatchTicket) -> Result<GatewayResult, GatewayError> {
        let body = self.authority.dispatch_body(&ticket)?;
        let authorization = DispatchAuthorization {
            authority: self.authority,
            ticket,
            body,
        };
        let reply = if authorization.ticket.may_dispatch() {
            self.provider.dispatch(&authorization)
        } else {
            self.provider.reconcile(&authorization)
        };
        let (bytes, mut provider_error) = match reply {
            Ok(evidence) => (evidence.into_bytes(), None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let evidence = match verify_evidence(self.provider, &authorization.ticket, &bytes) {
            Ok(evidence) => evidence,
            Err(_) => {
                provider_error = Some(ProviderError::InvalidEvidence);
                verify_evidence(self.provider, &authorization.ticket, &[])?
            }
        };
        let intent = authorization.ticket.intent();
        authorization
            .authority
            .record_outcome(authorization.ticket, evidence)?;
        Ok(GatewayResult {
            intent,
            state: authorization.authority.effect_state(intent)?,
            provider_error,
            receipt: authorization
                .authority
                .latest_receipt(intent)?
                .ok_or(LedgerError::Corrupt)?,
        })
    }
}
