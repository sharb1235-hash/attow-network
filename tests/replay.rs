//! Independent replay checks use real on-disk authority databases and owned data.

use std::{
    collections::BTreeSet,
    error::Error,
    sync::{Arc, Barrier},
};

use chronos_codec::ObjectId;
use chronos_ledger::{
    Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
    LedgerError, Policy, PromotionOutcome, PromotionPayload, PromotionRequest, verify_evidence,
};
use chronos_state::{Worldline, fork};
use rusqlite::{Connection, OpenFlags, types::Value};

type TestResult = Result<(), Box<dyn Error>>;

fn populate(authority: &mut Authority) -> Result<ObjectId, Box<dyn Error>> {
    let contract = ObjectId([7; 32]);
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 100,
        invariants: vec![],
    })?;
    authority.grant_right("fixture-budget", 1_000)?;
    let decision = ObjectId([8; 32]);
    authority.advance_generation(decision, 0)?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: Some(EffectProposal {
            contract,
            provider: "owned-fixture".into(),
            semantic_slot: "one-booking".into(),
            operation: ObjectId([9; 32]),
            right: "fixture-budget".into(),
            cost: 25,
            body: b"proposed-only".to_vec(),
        }),
    })?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [10; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    branch.put(
        b"selection/proposed-offer".to_vec(),
        b"fixture-offer".to_vec(),
    )?;
    let outcome = authority.promote(PromotionRequest {
        id: ObjectId([11; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })?;
    match outcome {
        PromotionOutcome::AcceptedPending { intent, .. } => Ok(intent),
        other => Err(format!("fixture promotion did not become pending: {other:?}").into()),
    }
}

fn database_rows(connection: &Connection) -> Result<Vec<(String, Vec<String>)>, rusqlite::Error> {
    let names = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut tables = Vec::new();
    for name in names {
        let quoted = name.replace('"', "\"\"");
        let mut statement = connection.prepare(&format!("SELECT * FROM \"{quoted}\""))?;
        let columns = statement.column_count();
        let mut rows = statement
            .query_map([], |row| {
                let values = (0..columns)
                    .map(|column| row.get::<_, Value>(column))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("{values:?}"))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.sort();
        tables.push((name, rows));
    }
    Ok(tables)
}

#[test]
fn repeated_replay_of_empty_authority_does_not_mutate_source() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let authority = Authority::open(&path)?;
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let before = database_rows(&connection)?;
    let first = format!("{:?}", authority.rebuild_and_verify()?);
    let second = format!("{:?}", authority.rebuild_and_verify()?);
    assert_eq!(first, second);
    assert_eq!(before, database_rows(&connection)?);
    Ok(())
}

#[test]
fn pending_effect_replay_needs_no_provider_and_cannot_claim_dispatch() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let mut authority = Authority::open(&path)?;
    let intent = populate(&mut authority)?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let before = database_rows(&connection)?;
    let first = format!("{:?}", authority.rebuild_and_verify()?);
    let second = format!("{:?}", authority.rebuild_and_verify()?);
    assert_eq!(first, second);
    assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
    assert_eq!(before, database_rows(&connection)?);
    // No EvidenceVerifier/provider is supplied to either replay invocation.
    Ok(())
}

#[test]
fn read_only_inspection_verifies_without_recovering_dispatching_intent() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let mut authority = Authority::open(&path)?;
    let intent = populate(&mut authority)?;
    let _ticket = authority.claim_effect(intent)?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Dispatching);
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let before = database_rows(&connection)?;
    let inspector = Authority::inspect(&path)?;
    inspector.rebuild_and_verify()?;
    assert_eq!(inspector.effect_state(intent)?, EffectState::Dispatching);
    assert_eq!(before, database_rows(&connection)?);
    Ok(())
}

struct OwnedFixtureVerifier;

impl EvidenceVerifier for OwnedFixtureVerifier {
    fn contract_id(&self) -> ObjectId {
        ObjectId([7; 32])
    }

    fn verify(
        &self,
        ticket: &DispatchTicket,
        receipt: &[u8],
    ) -> Result<EvidenceStatus, LedgerError> {
        if ticket.operation() != ObjectId([9; 32])
            || ticket.provider() != "owned-fixture"
            || receipt != b"owned-fixture-confirmed"
        {
            return Err(LedgerError::InvalidEvidence);
        }
        Ok(EvidenceStatus::Confirmed)
    }
}

#[test]
fn confirmed_fixture_evidence_replays_and_tampered_evidence_is_rejected() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let mut authority = Authority::open(&path)?;
    let intent = populate(&mut authority)?;
    let ticket = authority.claim_effect(intent)?;
    // This verifier represents only the owned test fixture, never a real provider.
    let evidence = verify_evidence(&OwnedFixtureVerifier, &ticket, b"owned-fixture-confirmed")?;
    authority.record_outcome(ticket, evidence)?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Confirmed);
    let right = authority.right_balance("fixture-budget")?;
    assert_eq!((right.reserved, right.spent, right.available), (0, 25, 975));
    let connection = Connection::open(&path)?;
    let before = database_rows(&connection)?;
    authority.rebuild_and_verify()?;
    assert_eq!(before, database_rows(&connection)?);
    assert_eq!(
        connection.execute("UPDATE ledger_evidence SET status=4", [])?,
        1
    );
    assert!(authority.rebuild_and_verify().is_err());
    Ok(())
}

#[test]
fn replay_rejects_tampered_materializations_with_untouched_event_history() -> TestResult {
    for (name, sql) in [
        ("state", "UPDATE ledger_cells SET value=X'74616d7065726564'"),
        ("effects", "UPDATE ledger_intents SET state=3"),
        (
            "rights",
            "UPDATE ledger_rights SET reserved=reserved-1 WHERE name='fixture-budget'",
        ),
        ("dedup", "UPDATE ledger_promotions SET response=X'80'"),
        ("outbox", "UPDATE ledger_outbox SET state=3"),
        ("slot", "DELETE FROM ledger_slots"),
        (
            "decision",
            "UPDATE ledger_decisions SET generation=generation+1",
        ),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("authority.sqlite");
        let mut authority = Authority::open(&path)?;
        populate(&mut authority)?;
        authority.rebuild_and_verify()?;
        let connection = Connection::open(&path)?;
        let events_before = database_rows(&connection)?
            .into_iter()
            .find(|(table, _)| table == "ledger_events")
            .ok_or("ledger event table missing")?;
        assert_eq!(connection.execute(sql, [])?, 1, "tamper fixture {name}");
        let events_after = database_rows(&connection)?
            .into_iter()
            .find(|(table, _)| table == "ledger_events")
            .ok_or("ledger event table missing")?;
        assert_eq!(
            events_before, events_after,
            "event history changed for {name}"
        );
        assert!(
            authority.rebuild_and_verify().is_err(),
            "accepted tampered {name}"
        );
    }
    Ok(())
}

#[test]
fn replay_rejects_event_bytes_tampering() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let mut authority = Authority::open(&path)?;
    populate(&mut authority)?;
    let connection = Connection::open(&path)?;
    assert_eq!(connection.execute(
        "UPDATE ledger_events SET bytes=CAST(bytes || X'00' AS BLOB) WHERE seq=(SELECT min(seq) FROM ledger_events)",
        [],
    )?, 1);
    assert!(authority.rebuild_and_verify().is_err());
    Ok(())
}

#[test]
fn verification_observes_consistent_snapshot_during_other_writer_commits() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("authority.sqlite");
    let authority = Authority::open(&path)?;
    let mut writer = Authority::open(&path)?;
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        writer_barrier.wait();
        for index in 0..32 {
            writer.grant_right(&format!("concurrent-{index}"), 100)?;
            std::thread::yield_now();
        }
        Ok::<_, chronos_ledger::LedgerError>(())
    });
    barrier.wait();
    let verification = (|| {
        for _ in 0..32 {
            authority.rebuild_and_verify()?;
            std::thread::yield_now();
        }
        Ok::<_, chronos_ledger::LedgerError>(())
    })();
    let writing = worker.join().map_err(|_| "writer thread panicked")?;
    verification?;
    writing?;
    authority.rebuild_and_verify()?;
    Ok(())
}
