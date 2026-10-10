use crate::TransportError;
use chronos_codec::ObjectId;
use chronos_protocol::{PromotionReply, ProposalEnvelope};
#[derive(Clone, Copy, Debug)]
pub struct ProposalContext {
    pub peer: [u8; 32],
    pub tenant: ObjectId,
    pub domain: ObjectId,
}
/// Trusted, bounded application callback. Implementations own durable deduplication,
/// writer/epoch fencing and all authority policy. Transport grants no writer role.
pub trait ProposalHandler: Send + Sync {
    fn propose(
        &self,
        context: ProposalContext,
        proposal: ProposalEnvelope,
    ) -> Result<PromotionReply, TransportError>;
}
pub struct DenyProposals;
impl ProposalHandler for DenyProposals {
    fn propose(
        &self,
        _: ProposalContext,
        _: ProposalEnvelope,
    ) -> Result<PromotionReply, TransportError> {
        Err(TransportError::Unauthorized)
    }
}
