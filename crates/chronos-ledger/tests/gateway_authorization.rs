use attow_ledger::{
    Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
    LedgerError, Policy, PromotionOutcome, PromotionPayload, PromotionRequest, verify_evidence,
};
use chronos_codec::ObjectId;
use chronos_state::{Worldline, fork};
use std::collections::BTreeSet;
struct Verifier;
impl EvidenceVerifier for Verifier {
    fn contract_id(&self) -> ObjectId {
        ObjectId([8; 32])
    }
    fn verify(&self, _: &DispatchTicket, receipt: &[u8]) -> Result<EvidenceStatus, LedgerError> {
        match receipt {
            b"unknown" => Ok(EvidenceStatus::Unknown),
            b"confirmed" => Ok(EvidenceStatus::Confirmed),
            _ => Err(LedgerError::InvalidEvidence),
        }
    }
}
#[test]
fn body_lookup_requires_matching_live_state_domain_and_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ledger.sqlite");
    let mut authority = Authority::open(&path)?;
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([ObjectId([8; 32])]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [2; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    let decision = ObjectId([3; 32]);
    authority.advance_generation(decision, 0)?;
    let proposal = EffectProposal {
        contract: ObjectId([8; 32]),
        provider: "owned-test".into(),
        semantic_slot: "one".into(),
        operation: ObjectId([4; 32]),
        right: "budget".into(),
        cost: 1,
        body: b"exact-body".to_vec(),
    };
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: Some(proposal.clone()),
    })?;
    let PromotionOutcome::AcceptedPending { intent, .. } = authority.promote(PromotionRequest {
        id: ObjectId([5; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })?
    else {
        return Err("promotion".into());
    };
    assert_eq!(authority.effect_proposal(intent)?, proposal);
    assert_eq!(authority.latest_receipt(intent)?, None);
    let ticket = authority.claim_effect(intent)?;
    assert_eq!(ticket.domain(), authority.domain());
    assert_eq!(authority.dispatch_body(&ticket)?, b"exact-body");
    let alien = Authority::open(&directory.path().join("alien.sqlite"))?;
    assert!(alien.dispatch_body(&ticket).is_err());
    assert!(Authority::inspect(&path)?.dispatch_body(&ticket).is_err());
    let evidence = verify_evidence(&Verifier, &ticket, b"unknown")?;
    authority.record_outcome(ticket.clone(), evidence)?;
    let unknown_receipt = authority
        .latest_receipt(intent)?
        .ok_or("missing durable unknown evidence")?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Unknown);
    assert!(authority.dispatch_body(&ticket).is_err());
    let query = authority.reconciliation_ticket(intent)?;
    assert_eq!(authority.dispatch_body(&query)?, b"exact-body");
    let evidence = verify_evidence(&Verifier, &query, b"confirmed")?;
    authority.record_outcome(query.clone(), evidence)?;
    let confirmed_receipt = authority
        .latest_receipt(intent)?
        .ok_or("missing durable confirmation")?;
    assert_ne!(unknown_receipt, confirmed_receipt);
    assert_eq!(
        Authority::inspect(&path)?.latest_receipt(intent)?,
        Some(confirmed_receipt)
    );
    assert!(authority.dispatch_body(&query).is_err());
    authority.rebuild_and_verify()?;
    chronos_store::Store::open(&path)?.collect_garbage()?;
    assert_eq!(
        Authority::inspect(&path)?.latest_receipt(intent)?,
        Some(confirmed_receipt)
    );
    let database = rusqlite::Connection::open(&path)?;
    database.execute(
        "UPDATE objects SET bytes = x'00' WHERE id = ?1",
        [&confirmed_receipt.0[..]],
    )?;
    assert!(
        authority.latest_receipt(intent).is_err(),
        "receipt accessor trusted corrupt CAS bytes"
    );
    Ok(())
}
