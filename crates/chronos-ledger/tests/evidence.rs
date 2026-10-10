use attow_ledger::{
    Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
    LedgerError, Policy, PromotionOutcome, PromotionPayload, PromotionRequest, verify_evidence,
};
use chronos_codec::ObjectId;
use chronos_state::{Worldline, fork};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
};

const CONTRACT: ObjectId = ObjectId([17; 32]);
type TestResult = Result<(), Box<dyn std::error::Error>>;

struct OwnedVerifier {
    contract: ObjectId,
    calls: AtomicUsize,
}
impl OwnedVerifier {
    fn new(contract: ObjectId) -> Self {
        Self {
            contract,
            calls: AtomicUsize::new(0),
        }
    }
}
impl EvidenceVerifier for OwnedVerifier {
    fn contract_id(&self) -> ObjectId {
        self.contract
    }
    fn verify(&self, _: &DispatchTicket, receipt: &[u8]) -> Result<EvidenceStatus, LedgerError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match receipt {
            b"owned-confirmation" => Ok(EvidenceStatus::Confirmed),
            b"owned-rejection" => Ok(EvidenceStatus::DefinitivelyRejected),
            b"owned-uncertainty" => Ok(EvidenceStatus::Unknown),
            _ => Err(LedgerError::InvalidEvidence),
        }
    }
}

fn configure(authority: &mut Authority) -> Result<ObjectId, LedgerError> {
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([CONTRACT]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    Ok(policy)
}
fn pending(
    authority: &mut Authority,
    policy: ObjectId,
    number: u8,
) -> Result<ObjectId, LedgerError> {
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
            provider: "owned-test".into(),
            semantic_slot: format!("slot-{number}"),
            operation: ObjectId([number.wrapping_add(100); 32]),
            right: "budget".into(),
            cost: 1,
            body: vec![number],
        }),
    })?;
    match authority.promote(PromotionRequest {
        id: ObjectId([number.wrapping_add(50); 32]),
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
fn invalid_contract_and_oversize_receipts_never_invoke_verifier() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut authority = Authority::open(&directory.path().join("authority.db"))?;
    let policy = configure(&mut authority)?;
    let intent = pending(&mut authority, policy, 1)?;
    let ticket = authority.claim_effect(intent)?;
    let wrong = OwnedVerifier::new(ObjectId([99; 32]));
    assert!(matches!(
        verify_evidence(&wrong, &ticket, b"owned-confirmation"),
        Err(LedgerError::InvalidEvidence)
    ));
    assert_eq!(wrong.calls.load(Ordering::Relaxed), 0);
    let verifier = OwnedVerifier::new(CONTRACT);
    assert!(matches!(
        verify_evidence(&verifier, &ticket, &vec![0; 65_537]),
        Err(LedgerError::InvalidEvidence)
    ));
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 0);
    assert!(matches!(
        verify_evidence(&verifier, &ticket, b"untrusted text"),
        Err(LedgerError::InvalidEvidence)
    ));
    assert_eq!(verifier.calls.load(Ordering::Relaxed), 1);
    assert_eq!(authority.effect_state(intent)?, EffectState::Dispatching);
    assert_eq!(authority.right_balance("budget")?.reserved, 1);
    authority.rebuild_and_verify()?;
    Ok(())
}
