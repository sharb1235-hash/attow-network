//! Trusted application composition of peer data with an explicitly fenced writer.
use chronos_codec::ObjectId;
use chronos_ledger::{Authority, PromotionOutcome, PromotionRequest};
use chronos_protocol::{PromotionReply, PromotionStatus, ProposalEnvelope};
use chronos_state::{FrozenBranch, ObjectData, StateError, StateReader};
use chronos_store::Store;
use chronos_transport::{
    ObjectRepository, ProposalContext, ProposalHandler, Scope, SqliteObjectRepository,
    TransferObject, TransportError,
};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

/// Application callback budget, independent of transport framing reservations.
const MAX_PROPOSAL_GRAPH_BYTES: usize = 8 * 1024 * 1024;
struct ProposalGraph(BTreeMap<ObjectId, TransferObject>);
impl StateReader for ProposalGraph {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get(&id).ok_or(StateError::Storage)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes.clone(),
        })
    }
}

/// Import a graph through the transport repository before proposing its descriptor.
/// The supplied authority must already own a trusted external writer lease.
pub struct PeerAuthority {
    tenant: ObjectId,
    state: Mutex<(Authority, SqliteObjectRepository)>,
}
impl PeerAuthority {
    /// Consume this callback after all serving endpoints have shut down. The
    /// returned authority keeps the same exclusive writer lease for dispatch.
    pub fn into_authority(self) -> Result<Authority, TransportError> {
        self.state
            .into_inner()
            .map(|(authority, _)| authority)
            .map_err(|_| TransportError::Application)
    }
    pub fn new(
        tenant: ObjectId,
        writer: ObjectId,
        authority: Authority,
        objects: &Path,
    ) -> Result<Self, TransportError> {
        let checkpoint = authority
            .require_federated_writer()
            .map_err(|_| TransportError::Application)?;
        if checkpoint.writer != Some(writer) {
            return Err(TransportError::Unauthorized);
        }
        // Require an existing store. The repository only mutates CAS/pin tables;
        // it cannot create ledger authority or execute imported code.
        drop(Store::open_read_only(objects).map_err(|_| TransportError::Storage)?);
        let objects = SqliteObjectRepository::open(objects)?;
        Ok(Self {
            tenant,
            state: Mutex::new((authority, objects)),
        })
    }
}
impl ProposalHandler for PeerAuthority {
    fn propose(
        &self,
        context: ProposalContext,
        proposal: ProposalEnvelope,
    ) -> Result<PromotionReply, TransportError> {
        // Peer identity and its operation allowlist are authenticated by the
        // transport. Repeat scope validation at this trusted application boundary.
        if context.tenant != self.tenant
            || proposal.tenant != self.tenant
            || context.domain != proposal.domain
        {
            return Err(TransportError::Unauthorized);
        }
        let mut state = self.state.lock().map_err(|_| TransportError::Application)?;
        let (authority, objects) = &mut *state;
        let checkpoint = authority
            .require_federated_writer()
            .map_err(|_| TransportError::Application)?;
        if proposal.domain.0 != checkpoint.domain.0 || proposal.epoch != checkpoint.epoch {
            return Err(TransportError::Unauthorized);
        }
        // Grant checks and graph bytes come from one repository read snapshot.
        // Knowing a content ID from another tenant does not confer access.
        let graph = ProposalGraph(
            objects
                .export(
                    Scope {
                        tenant: self.tenant,
                        domain: proposal.domain,
                    },
                    &[proposal.branch, proposal.payload],
                    MAX_PROPOSAL_GRAPH_BYTES,
                )?
                .into_iter()
                .map(|object| (object.id, object))
                .collect(),
        );
        let branch = FrozenBranch::reopen(&graph, proposal.branch, checkpoint.domain)
            .map_err(|_| TransportError::InvalidGraph)?;
        let outcome = authority
            .promote(PromotionRequest {
                id: proposal.request_id,
                payload: proposal.payload,
                branch,
                decision: proposal.decision,
                generation: proposal.generation,
            })
            .map_err(|_| TransportError::Application)?;
        let status = match outcome {
            PromotionOutcome::LocalCommitted { seq } => PromotionStatus::LocalCommitted { seq },
            PromotionOutcome::AcceptedPending { seq, intent } => {
                PromotionStatus::AcceptedPending { seq, intent }
            }
            PromotionOutcome::Conflict(report) => {
                // Keep diagnostics within the protocol envelope. Detailed local
                // conflict history remains in the ledger and is not sent blindly.
                let reasons = if !report.reasons.is_empty()
                    && report.reasons.len() <= 32
                    && report.reasons.iter().all(|reason| {
                        !reason.is_empty() && reason.len() <= 256 && !reason.contains('\0')
                    }) {
                    report.reasons
                } else {
                    vec!["proposal conflicts with the current authority state".into()]
                };
                PromotionStatus::Conflict { reasons }
            }
        };
        Ok(PromotionReply {
            request_id: proposal.request_id,
            domain: proposal.domain,
            epoch: proposal.epoch,
            status,
        })
    }
}
