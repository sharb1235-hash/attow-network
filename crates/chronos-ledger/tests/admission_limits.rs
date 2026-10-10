//! Unadmitted staging data must not exhaust replay's admitted-history budget.
use attow_ledger::{
    Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
    LedgerError, MAX_LEDGER_ROWS, Policy, PromotionOutcome, PromotionPayload, PromotionRequest,
    verify_evidence,
};
use chronos_codec::ObjectId;
use chronos_state::{Worldline, fork};
use std::collections::BTreeSet;

#[test]
fn staged_payloads_cannot_make_an_unchanged_authority_unopenable()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("admission.db");
    let mut authority = Authority::open(&path)?;
    let contract = ObjectId([20; 32]);
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 1,
        invariants: vec![],
    })?;
    let before = authority.rebuild_and_verify()?;

    // Distinct, valid objects exceed the 64 MiB replay allocation budget.
    // None belongs to the committed authority history; all staging calls succeed.
    for sequence in 0..1024u64 {
        let mut operation = [0u8; 32];
        operation[..8].copy_from_slice(&sequence.to_le_bytes());
        authority.store_payload(&PromotionPayload {
            domain: authority.domain(),
            epoch: authority.epoch(),
            policy,
            effect: Some(EffectProposal {
                contract,
                provider: "owned-simulator".into(),
                semantic_slot: "unadmitted-seat".into(),
                operation: ObjectId(operation),
                right: "not-granted".into(),
                cost: 1,
                body: vec![0; 65_536],
            }),
        })?;
    }

    assert_eq!(authority.rebuild_and_verify()?, before);
    drop(authority);
    let inspection = Authority::inspect(&path)?;
    assert_eq!(inspection.rebuild_and_verify()?, before);
    drop(inspection);
    let reopened = Authority::open(&path)?;
    assert_eq!(reopened.rebuild_and_verify()?, before);
    Ok(())
}

fn pending_request(
    authority: &mut Authority,
) -> Result<PromotionRequest, Box<dyn std::error::Error>> {
    let contract = ObjectId([20; 32]);
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 1,
        invariants: vec![],
    })?;
    authority.grant_right("seat", 1)?;
    let decision = ObjectId([21; 32]);
    authority.advance_generation(decision, 0)?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: Some(EffectProposal {
            contract,
            provider: "owned-simulator".into(),
            semantic_slot: "last-seat".into(),
            operation: ObjectId([22; 32]),
            right: "seat".into(),
            cost: 1,
            body: b"one-seat".to_vec(),
        }),
    })?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [23; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    Ok(PromotionRequest {
        id: ObjectId([24; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })
}

#[test]
fn promotion_rejects_before_consuming_its_mandatory_lifecycle_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("late-admission.db");
    let mut authority = Authority::open(&path)?;
    let request = pending_request(&mut authority)?;
    // Initialization, policy, grant and generation consume four events.
    for _ in 0..MAX_LEDGER_ROWS - 6 {
        authority.grant_right("seat", 1)?;
    }
    let before = authority.rebuild_and_verify()?;
    assert_eq!(before.events, MAX_LEDGER_ROWS as u64 - 2);
    let outcome = authority.promote(request);
    assert!(
        matches!(outcome, Err(LedgerError::Limit)),
        "two remaining event slots cannot safely authorize claim, recovery and outcome: {outcome:?}"
    );
    assert_eq!(authority.rebuild_and_verify()?, before);
    assert_eq!(authority.right_balance("seat")?.reserved, 0);
    drop(authority);
    assert_eq!(Authority::open(&path)?.rebuild_and_verify()?, before);
    Ok(())
}

struct ConfirmedReceipt;
impl EvidenceVerifier for ConfirmedReceipt {
    fn contract_id(&self) -> ObjectId {
        ObjectId([20; 32])
    }

    fn verify(
        &self,
        ticket: &DispatchTicket,
        receipt: &[u8],
    ) -> Result<EvidenceStatus, LedgerError> {
        assert!(!ticket.may_dispatch());
        assert_eq!(receipt, b"owned-confirmation");
        Ok(EvidenceStatus::Confirmed)
    }
}

#[test]
fn admitted_effect_can_claim_recover_and_confirm_after_admin_capacity_is_exhausted()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("reserved-capacity.db");
    let mut authority = Authority::open(&path)?;
    let request = pending_request(&mut authority)?;
    let PromotionOutcome::AcceptedPending { intent, .. } = authority.promote(request)? else {
        return Err("initial effect should be accepted".into());
    };
    // Stop at a real public-API rejection, not at a fabricated sequence value.
    let mut reached_limit = false;
    for _ in 0..MAX_LEDGER_ROWS {
        match authority.grant_right("seat", 1) {
            Ok(()) => {}
            Err(LedgerError::Limit) => {
                reached_limit = true;
                break;
            }
            Err(other) => return Err(other.into()),
        }
    }
    assert!(reached_limit);
    assert_eq!(
        authority.rebuild_and_verify()?.events,
        MAX_LEDGER_ROWS as u64 - 3,
        "authorization must retain claim, uncertain-restart, and terminal-outcome capacity"
    );
    let ticket = authority.claim_effect(intent)?;
    assert!(ticket.may_dispatch());
    drop(authority);

    let mut reopened = Authority::open(&path)?;
    assert_eq!(reopened.effect_state(intent)?, EffectState::Unknown);
    let ticket = reopened.reconciliation_ticket(intent)?;
    let evidence = verify_evidence(&ConfirmedReceipt, &ticket, b"owned-confirmation")?;
    reopened.record_outcome(ticket, evidence)?;
    assert_eq!(reopened.effect_state(intent)?, EffectState::Confirmed);
    assert_eq!(reopened.right_balance("seat")?.reserved, 0);
    assert_eq!(reopened.right_balance("seat")?.spent, 1);
    let completed = reopened.rebuild_and_verify()?;
    assert_eq!(completed.events, MAX_LEDGER_ROWS as u64);
    drop(reopened);
    assert_eq!(Authority::open(&path)?.rebuild_and_verify()?, completed);
    Ok(())
}
