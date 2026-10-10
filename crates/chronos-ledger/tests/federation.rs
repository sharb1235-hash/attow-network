//! Owned process-only fence fixture; this is not a distributed fencing service.
use attow_ledger::{
    Authority, AuthorityCheckpoint, DispatchTicket, EffectProposal, EffectState, EvidenceStatus,
    EvidenceVerifier, FenceRequest, LedgerError, Policy, PromotionOutcome, PromotionPayload,
    PromotionRequest, WriterFence, WriterLease, verify_evidence,
};
use chronos_codec::ObjectId;
use chronos_state::{Worldline, fork};
use chronos_store::Store;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
const A: ObjectId = ObjectId([41; 32]);
const B: ObjectId = ObjectId([42; 32]);
const CONTRACT: ObjectId = ObjectId([9; 32]);
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct FenceState {
    head: AuthorityCheckpoint,
    active: bool,
    available: bool,
    fail_advance: bool,
}
#[derive(Clone)]
struct Fence {
    state: Arc<Mutex<FenceState>>,
    identity: ObjectId,
}
struct Lease(Arc<Mutex<FenceState>>);
impl Fence {
    fn new(head: AuthorityCheckpoint) -> Self {
        Self {
            state: Arc::new(Mutex::new(FenceState {
                head,
                active: false,
                available: true,
                fail_advance: false,
            })),
            identity: A,
        }
    }
    fn for_writer(&self, identity: ObjectId) -> Self {
        Self {
            state: self.state.clone(),
            identity,
        }
    }
}
impl WriterFence for Fence {
    fn acquire(&self, request: &FenceRequest) -> Result<Box<dyn WriterLease>, LedgerError> {
        let mut state = self.state.lock().map_err(|_| LedgerError::Fenced)?;
        let cp = request.checkpoint;
        let allowed = match (cp.writer, cp.next_writer) {
            (None, None) => request.epoch == cp.epoch,
            (Some(writer), None) => request.writer == writer && request.epoch == cp.epoch,
            (Some(_), Some(next)) => {
                request.writer == next && Some(request.epoch) == cp.epoch.checked_add(1)
            }
            _ => false,
        };
        if !state.available
            || state.active
            || state.head != cp
            || self.identity != request.writer
            || !allowed
        {
            return Err(LedgerError::Fenced);
        }
        state.active = true;
        Ok(Box::new(Lease(self.state.clone())))
    }
}
impl WriterLease for Lease {
    fn check(&self, checkpoint: &AuthorityCheckpoint) -> Result<(), LedgerError> {
        let state = self.0.lock().map_err(|_| LedgerError::Fenced)?;
        if state.available && state.active && state.head == *checkpoint {
            Ok(())
        } else {
            Err(LedgerError::Fenced)
        }
    }
    fn advance(
        &self,
        previous: &AuthorityCheckpoint,
        next: &AuthorityCheckpoint,
    ) -> Result<(), LedgerError> {
        self.check(previous)?;
        let mut state = self.0.lock().map_err(|_| LedgerError::Fenced)?;
        if state.fail_advance {
            return Err(LedgerError::Fenced);
        }
        state.head = *next;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = false;
        }
    }
}
struct Verifier;
impl EvidenceVerifier for Verifier {
    fn contract_id(&self) -> ObjectId {
        CONTRACT
    }
    fn verify(&self, _: &DispatchTicket, receipt: &[u8]) -> Result<EvidenceStatus, LedgerError> {
        if receipt == b"confirmed" {
            Ok(EvidenceStatus::Confirmed)
        } else {
            Err(LedgerError::InvalidEvidence)
        }
    }
}
fn pending(authority: &mut Authority, number: u8) -> Result<ObjectId, LedgerError> {
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([CONTRACT]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    let decision = ObjectId([number; 32]);
    authority.advance_generation(decision, 0)?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [number; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: Some(EffectProposal {
            contract: CONTRACT,
            provider: "owned".into(),
            semantic_slot: format!("slot-{number}"),
            operation: ObjectId([number + 100; 32]),
            right: "budget".into(),
            cost: 1,
            body: vec![number],
        }),
    })?;
    match authority.promote(PromotionRequest {
        id: ObjectId([number + 50; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })? {
        PromotionOutcome::AcceptedPending { intent, .. } => Ok(intent),
        _ => Err(LedgerError::Aborted),
    }
}
#[test]
fn identical_snapshot_has_no_credential_and_cannot_acquire_second_writer() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.db");
    let copy = dir.path().join("copy.db");
    let mut authority = Authority::open(&path)?;
    let mut preexisting = Authority::open(&path)?;
    assert!(authority.require_federated_writer().is_err());
    let fence = Fence::new(authority.checkpoint()?);
    authority.initialize_federation(A, &fence)?;
    assert_eq!(authority.require_federated_writer()?.writer, Some(A));
    Store::open(&path)?.backup_to(&copy)?;
    assert!(Authority::open(&path).is_err());
    assert!(Authority::open(&copy).is_err());
    assert!(Authority::open_federated(&copy, A, &fence).is_err());
    assert!(Authority::open_federated(&copy, B, &fence.for_writer(B)).is_err());
    assert!(preexisting.grant_right("bypass", 1).is_err());
    let mut data = Authority::inspect(&copy)?;
    assert!(data.grant_right("bypass", 1).is_err());
    data.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn restart_requires_current_external_checkpoint_and_partition_never_activates() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.db");
    let stale = dir.path().join("stale.db");
    let mut a = Authority::open(&path)?;
    let fence = Fence::new(a.checkpoint()?);
    a.initialize_federation(A, &fence)?;
    Store::open(&path)?.backup_to(&stale)?;
    a.grant_right("budget", 1)?;
    drop(a);
    assert!(Authority::open_federated(&stale, A, &fence).is_err());
    let mut a = Authority::open_federated(&path, A, &fence)?;
    assert_eq!(a.right_balance("budget")?.available, 1);
    fence.state.lock().map_err(|_| "poison")?.available = false;
    assert!(a.grant_right("budget", 1).is_err());
    assert!(a.close_epoch(B).is_err());
    assert!(Authority::activate_handoff(&stale, B, &fence.for_writer(B)).is_err());
    assert_eq!(a.right_balance("budget")?.available, 1);
    Ok(())
}
#[test]
fn closed_epoch_handoff_preserves_unknown_identity_and_reconciliation() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("old.db");
    let copy = dir.path().join("new.db");
    let mut a = Authority::open(&path)?;
    let fence = Fence::new(a.checkpoint()?);
    a.initialize_federation(A, &fence)?;
    let intent = pending(&mut a, 1)?;
    let queued = pending(&mut a, 2)?;
    let ticket = a.claim_effect(intent)?;
    assert_eq!(a.dispatch_body(&ticket)?, vec![1]);
    let closed = a.close_epoch(B)?;
    assert_eq!(closed.next_writer, Some(B));
    assert_eq!(a.effect_state(intent)?, EffectState::Unknown);
    assert!(a.dispatch_body(&ticket).is_err());
    assert!(a.grant_right("budget", 1).is_err());
    assert!(Authority::open_federated(&path, A, &fence).is_err());
    Store::open(&path)?.backup_to(&copy)?;
    assert!(Authority::activate_handoff(&copy, A, &fence).is_err());
    let mut b = Authority::activate_handoff(&copy, B, &fence.for_writer(B))?;
    assert_eq!(b.epoch(), 2);
    assert_eq!(b.require_federated_writer()?.writer, Some(B));
    let query = b.reconciliation_ticket(intent)?;
    assert!(!query.may_dispatch());
    assert_eq!(query.operation(), ticket.operation());
    assert_eq!(query.attempt(), ticket.attempt());
    assert_eq!(query.epoch(), ticket.epoch());
    assert_eq!(b.dispatch_body(&query)?, vec![1]);
    assert!(b.claim_effect(intent).is_err());
    assert!(b.dispatch_body(&ticket).is_err());
    assert_eq!(b.right_balance("budget")?.reserved, 2);
    let evidence = verify_evidence(&Verifier, &query, b"confirmed")?;
    b.record_outcome(query, evidence)?;
    assert_eq!(b.effect_state(intent)?, EffectState::Confirmed);
    let new_ticket = b.claim_effect(queued)?;
    assert_eq!(new_ticket.epoch(), 2);
    assert_eq!(b.dispatch_body(&new_ticket)?, vec![2]);
    b.rebuild_and_verify()?;
    assert!(Authority::activate_handoff(&path, B, &fence.for_writer(B)).is_err());
    drop(b);
    let b = Authority::open_federated(&copy, B, &fence.for_writer(B))?;
    assert_eq!(b.effect_state(queued)?, EffectState::Unknown);
    b.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn failed_external_commit_never_returns_dispatch_authorization() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.db");
    let mut a = Authority::open(&path)?;
    let fence = Fence::new(a.checkpoint()?);
    a.initialize_federation(A, &fence)?;
    let intent = pending(&mut a, 3)?;
    fence.state.lock().map_err(|_| "poison")?.fail_advance = true;
    assert!(a.claim_effect(intent).is_err());
    assert!(a.require_federated_writer().is_err());
    assert!(a.close_epoch(B).is_err());
    drop(a);
    fence.state.lock().map_err(|_| "poison")?.fail_advance = false;
    assert!(
        Authority::open_federated(&path, A, &fence).is_err(),
        "unanchored state requires external administrator recovery"
    );
    Ok(())
}
#[test]
fn old_epoch_proposal_cannot_commit_after_explicit_handoff() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.db");
    let mut a = Authority::open(&path)?;
    let fence = Fence::new(a.checkpoint()?);
    a.initialize_federation(A, &fence)?;
    let policy = a.configure_policy(Policy {
        allowed_contracts: BTreeSet::new(),
        max_effect_cost: 0,
        invariants: vec![],
    })?;
    let decision = ObjectId([5; 32]);
    a.advance_generation(decision, 0)?;
    let mut branch = fork(a.domain(), &a.current_root()?, [7; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    let payload = a.store_payload(&PromotionPayload {
        domain: a.domain(),
        epoch: 1,
        policy,
        effect: None,
    })?;
    a.close_epoch(B)?;
    let mut b = Authority::activate_handoff(&path, B, &fence.for_writer(B))?;
    let before = b.current_root()?;
    let outcome = b.promote(PromotionRequest {
        id: ObjectId([8; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })?;
    assert!(matches!(outcome, PromotionOutcome::Conflict(_)));
    assert_eq!(b.current_root()?.object_id()?, before.object_id()?);
    b.rebuild_and_verify()?;
    Ok(())
}
