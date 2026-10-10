use attow_ledger::{
    Authority, EffectProposal, Policy, PromotionOutcome, PromotionPayload, PromotionRequest,
};
use chronos_codec::ObjectId;
use chronos_state::{Worldline, fork};
use std::collections::BTreeSet;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request(
    authority: &mut Authority,
    policy: ObjectId,
    nonce: u8,
    effect: bool,
) -> Result<PromotionRequest, Box<dyn std::error::Error>> {
    let mut branch = fork(authority.domain(), &authority.current_root()?, [nonce; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    branch.put(b"value".to_vec(), vec![nonce])?;
    let decision = ObjectId([nonce; 32]);
    authority.advance_generation(decision, 0)?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: effect.then(|| EffectProposal {
            contract: ObjectId([99; 32]),
            provider: "owned".into(),
            semantic_slot: "slot".into(),
            operation: ObjectId([nonce; 32]),
            right: "units".into(),
            cost: 1,
            body: vec![nonce],
        }),
    })?;
    Ok(PromotionRequest {
        id: ObjectId([nonce + 100; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })
}
fn setup(path: &std::path::Path) -> Result<(Authority, ObjectId), Box<dyn std::error::Error>> {
    let mut authority = Authority::open(path)?;
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([ObjectId([99; 32])]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("units", 10)?;
    Ok((authority, policy))
}
#[test]
fn promotion_record_preserves_original_state_after_later_commits_and_reopen() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authority.sqlite");
    let (mut authority, policy) = setup(&path)?;
    let first = request(&mut authority, policy, 1, false)?;
    let outcome = authority.promote(first.clone())?;
    let original = authority.current_root()?.object_id()?;
    let second = request(&mut authority, policy, 2, false)?;
    authority.promote(second)?;
    assert_ne!(original, authority.current_root()?.object_id()?);
    let before = authority.checkpoint()?;
    let record = authority
        .promotion_record(first.id)?
        .expect("verified original promotion");
    assert_eq!(record.request_id, first.id);
    assert_eq!(record.payload, first.payload);
    assert_eq!(record.branch, first.branch.object_id()?);
    assert_eq!(record.decision, first.decision);
    assert_eq!(record.generation, 0);
    assert_eq!(record.outcome, outcome);
    assert_eq!(record.state_root, original);
    assert!(matches!(outcome,PromotionOutcome::LocalCommitted{seq} if seq==record.sequence));
    assert_eq!(authority.checkpoint()?, before);
    drop(authority);
    let inspected = Authority::inspect(&path)?;
    assert_eq!(inspected.promotion_record(first.id)?, Some(record));
    assert_eq!(inspected.promotion_record(ObjectId([250; 32]))?, None);
    Ok(())
}
#[test]
fn pending_and_conflicted_promotions_return_their_actual_event_outcomes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let (mut authority, policy) = setup(&dir.path().join("authority.sqlite"))?;
    let pending = request(&mut authority, policy, 3, true)?;
    let outcome = authority.promote(pending.clone())?;
    assert!(matches!(outcome, PromotionOutcome::AcceptedPending { .. }));
    let verified = authority
        .promotion_record(pending.id)?
        .expect("pending promotion");
    assert_eq!(verified.outcome, outcome);
    let mut conflict = pending;
    conflict.id = ObjectId([104; 32]);
    let outcome = authority.promote(conflict.clone())?;
    assert!(matches!(outcome, PromotionOutcome::Conflict(_)));
    let record = authority
        .promotion_record(conflict.id)?
        .expect("conflicted event");
    assert_eq!(record.outcome, outcome);
    assert_eq!(record.state_root, verified.state_root);
    assert!(record.sequence > verified.sequence);
    Ok(())
}
#[test]
fn promotion_lookup_verifies_later_events_and_materialized_tables_before_returning() -> TestResult {
    for tamper in 0..2 {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("authority.sqlite");
        let (mut authority, policy) = setup(&path)?;
        let first = request(&mut authority, policy, 5, false)?;
        authority.promote(first.clone())?;
        authority.grant_right("units", 1)?;
        drop(authority);
        let inspected = Authority::inspect(&path)?;
        let connection = rusqlite::Connection::open(&path)?;
        if tamper == 0 {
            connection.execute("UPDATE ledger_events SET bytes=x'00' WHERE seq=(SELECT max(seq) FROM ledger_events)",[])?;
        } else {
            connection.execute("UPDATE ledger_cells SET value=x'00'", [])?;
        }
        assert!(
            inspected.promotion_record(first.id).is_err(),
            "tamper {tamper}"
        );
        assert!(
            inspected.promotion_record(ObjectId([250; 32])).is_err(),
            "missing lookup must verify too"
        );
    }
    Ok(())
}
