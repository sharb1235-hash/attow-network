//! Independent authority tests. A fork never grants spendable rights.
use attow_network::{
    ObjectId,
    ledger::{
        Authority, EffectProposal, Policy, PromotionOutcome, PromotionPayload, PromotionRequest,
    },
    state::{Branch, Worldline, fork},
};
use std::{
    collections::BTreeSet,
    error::Error,
    sync::{Arc, Barrier},
};
type TestResult = Result<(), Box<dyn Error>>;
const CONTRACT: ObjectId = ObjectId([200; 32]);
fn configure(a: &mut Authority) -> Result<ObjectId, Box<dyn Error>> {
    let policy = a.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([CONTRACT]),
        max_effect_cost: 100,
        invariants: vec![],
    })?;
    a.grant_right("budget", 100)?;
    Ok(policy)
}
fn branch(a: &Authority, policy: ObjectId, nonce: u8) -> Result<Branch, Box<dyn Error>> {
    let mut b = fork(a.domain(), &a.current_root()?, [nonce; 32])?;
    b.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    Ok(b)
}
fn request(
    a: &mut Authority,
    policy: ObjectId,
    branch: Branch,
    number: u8,
    effect: Option<EffectProposal>,
) -> Result<PromotionRequest, Box<dyn Error>> {
    let decision = ObjectId([number; 32]);
    a.advance_generation(decision, 0)?;
    let payload = a.store_payload(&PromotionPayload {
        domain: a.domain(),
        epoch: a.epoch(),
        policy,
        effect,
    })?;
    Ok(PromotionRequest {
        id: ObjectId([number.wrapping_add(100); 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })
}
fn effect(operation: u8, cost: u64) -> EffectProposal {
    EffectProposal {
        contract: CONTRACT,
        provider: "owned-simulator".into(),
        semantic_slot: "seat-A".into(),
        operation: ObjectId([operation; 32]),
        right: "budget".into(),
        cost,
        body: b"one-seat".to_vec(),
    }
}
fn committed(outcome: &PromotionOutcome) -> bool {
    matches!(
        outcome,
        PromotionOutcome::LocalCommitted { .. } | PromotionOutcome::AcceptedPending { .. }
    )
}

#[test]
fn independent_authorities_reject_each_others_branches() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("a.sqlite"))?;
    let mut b = Authority::open(&d.path().join("b.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut proposal = branch(&a, policy, 1)?;
    proposal.put(b"alien".to_vec(), b"no".to_vec())?;
    let req = request(&mut a, policy, proposal, 1, None)?;
    let result = b.promote(req);
    assert!(result.is_err() || matches!(result, Ok(PromotionOutcome::Conflict(_))));
    assert_eq!(b.current_root()?.get(b"alien")?, None);
    b.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn write_skew_checks_reads_even_when_writes_are_disjoint() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut left = branch(&a, policy, 1)?;
    let mut right = branch(&a, policy, 2)?;
    assert_eq!(left.get(b"right")?, None);
    assert_eq!(right.get(b"left")?, None);
    left.put(b"left".to_vec(), b"on".to_vec())?;
    right.put(b"right".to_vec(), b"on".to_vec())?;
    let left = request(&mut a, policy, left, 1, None)?;
    let right = request(&mut a, policy, right, 2, None)?;
    assert!(committed(&a.promote(left)?));
    assert!(matches!(a.promote(right)?, PromotionOutcome::Conflict(_)));
    assert_eq!(a.current_root()?.get(b"right")?, None);
    a.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn missing_key_aba_cannot_hide_a_conflict() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut stale = branch(&a, policy, 1)?;
    assert_eq!(stale.get(b"token")?, None);
    stale.put(b"stale-result".to_vec(), b"no".to_vec())?;
    let stale = request(&mut a, policy, stale, 1, None)?;
    let mut insert = branch(&a, policy, 2)?;
    insert.put(b"token".to_vec(), b"present".to_vec())?;
    let insert = request(&mut a, policy, insert, 2, None)?;
    assert!(committed(&a.promote(insert)?));
    let mut delete = branch(&a, policy, 3)?;
    delete.delete(b"token")?;
    let delete = request(&mut a, policy, delete, 3, None)?;
    assert!(committed(&a.promote(delete)?));
    assert_eq!(a.current_root()?.get(b"token")?, None);
    assert!(matches!(a.promote(stale)?, PromotionOutcome::Conflict(_)));
    assert_eq!(a.current_root()?.get(b"stale-result")?, None);
    a.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn scan_phantom_rejects_stale_branch() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut scan = branch(&a, policy, 1)?;
    assert!(scan.scan_prefix(b"booking/")?.is_empty());
    scan.put(b"scan-result".to_vec(), b"empty".to_vec())?;
    let scan = request(&mut a, policy, scan, 1, None)?;
    let mut insert = branch(&a, policy, 2)?;
    insert.put(b"booking/1".to_vec(), b"present".to_vec())?;
    let insert = request(&mut a, policy, insert, 2, None)?;
    assert!(committed(&a.promote(insert)?));
    assert!(matches!(a.promote(scan)?, PromotionOutcome::Conflict(_)));
    assert_eq!(a.current_root()?.get(b"scan-result")?, None);
    Ok(())
}
#[test]
fn duplicate_request_binds_payload_and_complete_proposal() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut original = branch(&a, policy, 1)?;
    original.put(b"one".to_vec(), b"value".to_vec())?;
    let req = request(&mut a, policy, original, 1, None)?;
    let first = a.promote(req.clone())?;
    let before = format!("{:?}", a.rebuild_and_verify()?);
    let repeated = a.promote(req.clone())?;
    assert_eq!(first, repeated);
    assert_eq!(before, format!("{:?}", a.rebuild_and_verify()?));
    let mut altered = req.clone();
    altered.payload = ObjectId([255; 32]);
    assert!(a.promote(altered).is_err());
    let mut new_branch = branch(&a, policy, 4)?;
    new_branch.put(b"different".to_vec(), b"forbidden".to_vec())?;
    let mut altered = req;
    altered.branch = new_branch.freeze()?;
    assert!(a.promote(altered).is_err());
    assert_eq!(a.current_root()?.get(b"different")?, None);
    assert_eq!(before, format!("{:?}", a.rebuild_and_verify()?));
    Ok(())
}
#[test]
fn two_connections_race_one_slot_without_leaking_loser_writes() -> TestResult {
    let d = tempfile::tempdir()?;
    let path = d.path().join("authority.sqlite");
    let mut a = Authority::open(&path)?;
    let policy = configure(&mut a)?;
    let mut left = branch(&a, policy, 1)?;
    left.put(b"left".to_vec(), b"pending".to_vec())?;
    let mut right = branch(&a, policy, 2)?;
    right.put(b"right".to_vec(), b"pending".to_vec())?;
    let left = request(&mut a, policy, left, 1, Some(effect(10, 60)))?;
    let right = request(&mut a, policy, right, 2, Some(effect(11, 60)))?;
    let mut b = Authority::open(&path)?;
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        b.promote(right)
    });
    barrier.wait();
    let left = a.promote(left)?;
    let right = worker.join().map_err(|_| "writer panicked")??;
    assert_eq!(
        usize::from(committed(&left)) + usize::from(committed(&right)),
        1
    );
    let root = a.current_root()?;
    assert_eq!(root.get(b"left")?.is_some(), committed(&left));
    assert_eq!(root.get(b"right")?.is_some(), committed(&right));
    let balance = a.right_balance("budget")?;
    assert_eq!(
        (
            balance.granted,
            balance.reserved,
            balance.spent,
            balance.available
        ),
        (100, 60, 0, 40)
    );
    a.rebuild_and_verify()?;
    Ok(())
}
#[test]
fn budget_failure_is_atomic_and_stale_policy_is_rejected() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut proposed = branch(&a, policy, 1)?;
    proposed.put(b"expensive".to_vec(), b"no".to_vec())?;
    let too_expensive = request(&mut a, policy, proposed, 1, Some(effect(10, 101)))?;
    assert!(matches!(
        a.promote(too_expensive)?,
        PromotionOutcome::Conflict(_)
    ));
    assert_eq!(a.current_root()?.get(b"expensive")?, None);
    assert_eq!(a.right_balance("budget")?.reserved, 0);
    let mut stale = branch(&a, policy, 2)?;
    stale.put(b"stale-policy".to_vec(), b"no".to_vec())?;
    let stale = request(&mut a, policy, stale, 2, None)?;
    a.configure_policy(Policy {
        allowed_contracts: BTreeSet::new(),
        max_effect_cost: 1,
        invariants: vec![],
    })?;
    assert!(matches!(a.promote(stale)?, PromotionOutcome::Conflict(_)));
    assert_eq!(a.current_root()?.get(b"stale-policy")?, None);
    a.rebuild_and_verify()?;
    Ok(())
}

#[test]
fn business_invariant_is_checked_against_candidate_state() -> TestResult {
    use attow_network::ledger::Invariant;
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let initial_policy = configure(&mut a)?;
    let mut seed = branch(&a, initial_policy, 1)?;
    seed.put(b"guard".to_vec(), b"allowed".to_vec())?;
    let seed = request(&mut a, initial_policy, seed, 1, None)?;
    assert!(committed(&a.promote(seed)?));
    let policy = a.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([CONTRACT]),
        max_effect_cost: 100,
        invariants: vec![Invariant::KeyEquals {
            key: b"guard".to_vec(),
            expected: Some(b"allowed".to_vec()),
        }],
    })?;
    let mut violating = branch(&a, policy, 2)?;
    violating.put(b"guard".to_vec(), b"forbidden".to_vec())?;
    let violating = request(&mut a, policy, violating, 2, None)?;
    assert!(matches!(
        a.promote(violating)?,
        PromotionOutcome::Conflict(_)
    ));
    assert_eq!(a.current_root()?.get(b"guard")?, Some(b"allowed".to_vec()));
    a.rebuild_and_verify()?;
    Ok(())
}

#[test]
fn unknown_and_confirmed_keep_slot_across_generation_changes() -> TestResult {
    use attow_network::ledger::{
        DispatchTicket, EffectState, EvidenceStatus, EvidenceVerifier, LedgerError, verify_evidence,
    };
    struct OwnedVerifier(EvidenceStatus);
    impl EvidenceVerifier for OwnedVerifier {
        fn contract_id(&self) -> ObjectId {
            CONTRACT
        }
        fn verify(
            &self,
            _ticket: &DispatchTicket,
            response: &[u8],
        ) -> Result<EvidenceStatus, LedgerError> {
            if response != b"owned fixture evidence" {
                return Err(LedgerError::InvalidEvidence);
            }
            Ok(self.0)
        }
    }
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut first = branch(&a, policy, 1)?;
    first.put(b"first".to_vec(), b"pending".to_vec())?;
    let first = request(&mut a, policy, first, 1, Some(effect(10, 60)))?;
    let intent = match a.promote(first)? {
        PromotionOutcome::AcceptedPending { intent, .. } => intent,
        other => return Err(format!("expected pending, got {other:?}").into()),
    };
    let ticket = a.claim_effect(intent)?;
    let evidence = verify_evidence(
        &OwnedVerifier(EvidenceStatus::Unknown),
        &ticket,
        b"owned fixture evidence",
    )?;
    a.record_outcome(ticket.clone(), evidence)?;
    assert_eq!(a.effect_state(intent)?, EffectState::Unknown);
    for generation in [1, 2] {
        a.advance_generation(ObjectId([1; 32]), generation)?;
        let mut replacement = branch(&a, policy, 20 + generation as u8)?;
        replacement.put(b"replacement".to_vec(), b"forbidden".to_vec())?;
        let payload = a.store_payload(&PromotionPayload {
            domain: a.domain(),
            epoch: a.epoch(),
            policy,
            effect: Some(effect(20 + generation as u8, 1)),
        })?;
        let replacement = PromotionRequest {
            id: ObjectId([20 + generation as u8; 32]),
            payload,
            branch: replacement.freeze()?,
            decision: ObjectId([1; 32]),
            generation,
        };
        assert!(matches!(
            a.promote(replacement)?,
            PromotionOutcome::Conflict(_)
        ));
        if generation == 1 {
            let evidence = verify_evidence(
                &OwnedVerifier(EvidenceStatus::Confirmed),
                &ticket,
                b"owned fixture evidence",
            )?;
            a.record_outcome(ticket.clone(), evidence)?;
            assert_eq!(a.effect_state(intent)?, EffectState::Confirmed);
        }
    }
    assert_eq!(a.current_root()?.get(b"replacement")?, None);
    let balance = a.right_balance("budget")?;
    assert_eq!(
        (
            balance.granted,
            balance.reserved,
            balance.spent,
            balance.available
        ),
        (100, 0, 60, 40)
    );
    a.rebuild_and_verify()?;
    Ok(())
}

#[test]
fn cancellation_and_claim_serialize_before_dispatch() -> TestResult {
    use attow_network::ledger::EffectState;
    let d = tempfile::tempdir()?;
    let path = d.path().join("authority.sqlite");
    let mut a = Authority::open(&path)?;
    let policy = configure(&mut a)?;
    let proposal = branch(&a, policy, 1)?;
    let req = request(&mut a, policy, proposal, 1, Some(effect(10, 60)))?;
    let intent = match a.promote(req)? {
        PromotionOutcome::AcceptedPending { intent, .. } => intent,
        _ => return Err("expected pending effect".into()),
    };
    let mut b = Authority::open(&path)?;
    let barrier = Arc::new(Barrier::new(2));
    let other_barrier = Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        other_barrier.wait();
        b.cancel_before_send(intent)
    });
    barrier.wait();
    let claimed = a.claim_effect(intent);
    let cancelled = worker.join().map_err(|_| "cancel worker panicked")?;
    assert_ne!(claimed.is_ok(), cancelled.is_ok());
    if claimed.is_ok() {
        assert_eq!(a.effect_state(intent)?, EffectState::Dispatching);
        assert_eq!(a.right_balance("budget")?.reserved, 60);
    } else {
        assert_eq!(a.effect_state(intent)?, EffectState::CancelledBeforeSend);
        assert_eq!(a.right_balance("budget")?.reserved, 0);
    }
    a.rebuild_and_verify()?;
    Ok(())
}

#[test]
fn cancelled_operation_identity_cannot_be_rebound_to_different_payload_or_slot() -> TestResult {
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let proposal = branch(&a, policy, 1)?;
    let req = request(&mut a, policy, proposal, 1, Some(effect(10, 60)))?;
    let intent = match a.promote(req)? {
        PromotionOutcome::AcceptedPending { intent, .. } => intent,
        _ => return Err("expected pending effect".into()),
    };
    a.cancel_before_send(intent)?;
    let mut altered_effect = effect(10, 1);
    altered_effect.semantic_slot = "another-seat".into();
    altered_effect.body = b"different-body".to_vec();
    let mut proposal = branch(&a, policy, 2)?;
    proposal.put(b"reused-operation".to_vec(), b"forbidden".to_vec())?;
    let req = request(&mut a, policy, proposal, 2, Some(altered_effect))?;
    assert!(matches!(a.promote(req)?, PromotionOutcome::Conflict(_)));
    assert_eq!(a.current_root()?.get(b"reused-operation")?, None);
    assert_eq!(a.right_balance("budget")?.reserved, 0);
    a.rebuild_and_verify()?;
    Ok(())
}

#[test]
fn evidence_is_bound_to_contract_and_exact_ticket_and_duplicate_receipts_are_idempotent()
-> TestResult {
    use attow_network::ledger::{
        DispatchTicket, EvidenceStatus, EvidenceVerifier, LedgerError, verify_evidence,
    };
    struct FixtureVerifier(ObjectId);
    impl EvidenceVerifier for FixtureVerifier {
        fn contract_id(&self) -> ObjectId {
            self.0
        }
        fn verify(
            &self,
            _ticket: &DispatchTicket,
            receipt: &[u8],
        ) -> Result<EvidenceStatus, LedgerError> {
            if receipt == b"fixture-confirmed" {
                Ok(EvidenceStatus::Confirmed)
            } else {
                Err(LedgerError::InvalidEvidence)
            }
        }
    }
    let d = tempfile::tempdir()?;
    let mut a = Authority::open(&d.path().join("authority.sqlite"))?;
    let policy = configure(&mut a)?;
    let mut tickets = Vec::new();
    for number in [1u8, 2] {
        let proposed = branch(&a, policy, number)?;
        let mut operation = effect(number + 10, 20);
        operation.semantic_slot = format!("seat-{number}");
        let req = request(&mut a, policy, proposed, number, Some(operation))?;
        let intent = match a.promote(req)? {
            PromotionOutcome::AcceptedPending { intent, .. } => intent,
            _ => return Err("expected pending effect".into()),
        };
        tickets.push(a.claim_effect(intent)?);
    }
    assert!(
        verify_evidence(
            &FixtureVerifier(ObjectId([33; 32])),
            &tickets[0],
            b"fixture-confirmed"
        )
        .is_err()
    );
    assert!(verify_evidence(&FixtureVerifier(CONTRACT), &tickets[0], b"unverified-text").is_err());
    let evidence = verify_evidence(
        &FixtureVerifier(CONTRACT),
        &tickets[0],
        b"fixture-confirmed",
    )?;
    assert!(a.record_outcome(tickets[1].clone(), evidence).is_err());
    assert_eq!(a.right_balance("budget")?.reserved, 40);
    let evidence = verify_evidence(
        &FixtureVerifier(CONTRACT),
        &tickets[0],
        b"fixture-confirmed",
    )?;
    a.record_outcome(tickets[0].clone(), evidence)?;
    let before = a.rebuild_and_verify()?;
    let evidence = verify_evidence(
        &FixtureVerifier(CONTRACT),
        &tickets[0],
        b"fixture-confirmed",
    )?;
    a.record_outcome(tickets[0].clone(), evidence)?;
    assert_eq!(before, a.rebuild_and_verify()?);
    let balance = a.right_balance("budget")?;
    assert_eq!(
        (balance.reserved, balance.spent, balance.available),
        (20, 20, 60)
    );
    Ok(())
}
