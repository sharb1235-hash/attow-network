//! Abrupt child-process termination tests. Production boundary cases are added
//! against the explicit ledger observer API; no production environment backdoor.

use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use attow_network::{
    codec::ObjectId,
    ledger::{
        Authority, DispatchTicket, EffectProposal, EffectState, EvidenceStatus, EvidenceVerifier,
        LedgerError, Policy, PromotionOutcome, PromotionPayload, PromotionRequest,
        TransitionBoundary, verify_evidence,
    },
    state::{StateRoot, Worldline, fork},
};

const CONTRACT: ObjectId = ObjectId([14; 32]);
const DECISION: ObjectId = ObjectId([12; 32]);
const REQUEST: ObjectId = ObjectId([11; 32]);
const OPERATION: ObjectId = ObjectId([13; 32]);

#[derive(Debug, PartialEq, Eq)]
struct DurableSnapshot {
    events: i64,
    requests: i64,
    intents: i64,
    outbox: i64,
    slots: i64,
    evidence: i64,
    objects: i64,
    pins: i64,
    root: Vec<u8>,
    head: Vec<u8>,
    rights: (i64, i64, i64),
    effect_state: Option<i64>,
    outbox_state: Option<i64>,
}

fn raw_snapshot(
    path: &Path,
    intent: Option<ObjectId>,
) -> Result<DurableSnapshot, Box<dyn std::error::Error>> {
    use rusqlite::OptionalExtension;
    let mut connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let transaction = connection.transaction()?;
    let count = |table: &str| {
        transaction.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
    };
    let (root, head) = transaction.query_row(
        "SELECT root, head FROM ledger_meta WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let rights = transaction.query_row(
        "SELECT granted, reserved, spent FROM ledger_rights WHERE name='test-seat'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let effect_state = if let Some(intent) = intent {
        transaction
            .query_row(
                "SELECT state FROM ledger_intents WHERE id=?1",
                [&intent.0[..]],
                |row| row.get(0),
            )
            .optional()?
    } else {
        None
    };
    let outbox_state = if let Some(intent) = intent {
        transaction
            .query_row(
                "SELECT state FROM ledger_outbox WHERE intent=?1",
                [&intent.0[..]],
                |row| row.get(0),
            )
            .optional()?
    } else {
        None
    };
    Ok(DurableSnapshot {
        events: count("ledger_events")?,
        requests: count("ledger_promotions")?,
        intents: count("ledger_intents")?,
        outbox: count("ledger_outbox")?,
        slots: count("ledger_slots")?,
        evidence: count("ledger_evidence")?,
        objects: count("objects")?,
        pins: count("root_pins")?,
        root,
        head,
        rights,
        effect_state,
        outbox_state,
    })
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct DropSentinel(std::path::PathBuf);
impl Drop for DropSentinel {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"graceful drop occurred");
    }
}

struct Fixture {
    root: StateRoot,
    policy: ObjectId,
    payload: ObjectId,
}

impl Fixture {
    fn initialize(authority: &mut Authority) -> Result<Self, Box<dyn std::error::Error>> {
        let policy = authority.configure_policy(Policy {
            allowed_contracts: std::collections::BTreeSet::from([CONTRACT]),
            max_effect_cost: 1,
            invariants: Vec::new(),
        })?;
        authority.grant_right("test-seat", 1)?;
        authority.advance_generation(DECISION, 0)?;
        let payload = authority.store_payload(&PromotionPayload {
            domain: authority.domain(),
            epoch: authority.epoch(),
            policy,
            effect: Some(EffectProposal {
                contract: CONTRACT,
                provider: "owned-test-simulator".into(),
                semantic_slot: "one-seat".into(),
                operation: OPERATION,
                right: "test-seat".into(),
                cost: 1,
                body: b"no network operation".to_vec(),
            }),
        })?;
        Ok(Self {
            root: authority.current_root()?,
            policy,
            payload,
        })
    }

    fn request(&self) -> Result<PromotionRequest, Box<dyn std::error::Error>> {
        let mut branch = fork(self.root.domain(), &self.root, [15; 32])?;
        branch.set_worldline(Worldline {
            policy: Some(self.policy),
            ..Worldline::default()
        })?;
        branch.put(b"winner".to_vec(), b"pending".to_vec())?;
        Ok(PromotionRequest {
            id: REQUEST,
            payload: self.payload,
            branch: branch.freeze()?,
            decision: DECISION,
            generation: 0,
        })
    }
}

fn accepted(outcome: PromotionOutcome) -> Result<ObjectId, Box<dyn std::error::Error>> {
    match outcome {
        PromotionOutcome::AcceptedPending { intent, .. } => Ok(intent),
        other => Err(format!("expected AcceptedPending, got {other:?}").into()),
    }
}

struct OwnedReceiptVerifier;
impl EvidenceVerifier for OwnedReceiptVerifier {
    fn contract_id(&self) -> ObjectId {
        CONTRACT
    }
    fn verify(
        &self,
        ticket: &DispatchTicket,
        receipt: &[u8],
    ) -> Result<EvidenceStatus, LedgerError> {
        assert_eq!(ticket.contract(), CONTRACT);
        assert_eq!(receipt, b"owned-simulator-confirmed");
        Ok(EvidenceStatus::Confirmed)
    }
}

fn boundary_hook(boundary: TransitionBoundary) {
    let Ok(wanted) = std::env::var("ATTOW_LEDGER_FAILPOINT") else {
        return;
    };
    if format!("{boundary:?}") != wanted {
        return;
    }
    let Ok(marker) = std::env::var("ATTOW_CRASH_MARKER") else {
        std::process::exit(71);
    };
    if fs::write(marker, b"transaction boundary reached").is_err() {
        std::process::exit(72);
    }
    loop {
        thread::park();
    }
}

fn crash_at(
    directory: &Path,
    database: &Path,
    boundary: &str,
    fixture: &Fixture,
    intent: Option<ObjectId>,
) -> TestResult {
    let marker = directory.join(format!("{boundary}.ready"));
    let dropped = directory.join(format!("{boundary}.dropped"));
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "crash_child_entry", "--nocapture"])
        .env("ATTOW_CRASH_MARKER", &marker)
        .env("ATTOW_CRASH_DROP", &dropped)
        .env("ATTOW_LEDGER_FAILPOINT", boundary)
        .env("ATTOW_CRASH_DATABASE", database)
        .env("ATTOW_CRASH_POLICY", fixture.policy.to_string())
        .env("ATTOW_CRASH_PAYLOAD", fixture.payload.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(intent) = intent {
        command.env("ATTOW_CRASH_INTENT", intent.to_string());
    }
    let mut child = command.spawn()?;
    kill_at_marker(&mut child, &marker)?;
    assert!(
        !dropped.exists(),
        "a ledger child must terminate without running destructors"
    );
    Ok(())
}

fn kill_at_marker(child: &mut Child, marker: &Path) -> TestResult {
    let started = Instant::now();
    while !marker.exists() {
        if let Some(status) = child.try_wait()? {
            return Err(format!("child exited before failpoint marker: {status}").into());
        }
        if started.elapsed() > Duration::from_secs(15) {
            child.kill()?;
            child.wait()?;
            return Err("child did not reach the requested failpoint".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    child.kill()?;
    let status = child.wait()?;
    assert!(!status.success(), "killed child must not report success");
    Ok(())
}

#[test]
fn crash_child_entry() -> TestResult {
    let Ok(marker) = std::env::var("ATTOW_CRASH_MARKER") else {
        return Ok(());
    };
    let drop_path = std::env::var("ATTOW_CRASH_DROP")?;
    let _sentinel = DropSentinel(drop_path.into());
    if let Ok(boundary) = std::env::var("ATTOW_LEDGER_FAILPOINT") {
        let path = std::path::PathBuf::from(std::env::var("ATTOW_CRASH_DATABASE")?);
        if boundary.contains("Recovery") {
            Authority::open_with_hook(&path, Some(boundary_hook))?;
            return Err("recovery did not invoke requested boundary".into());
        }
        let mut authority = Authority::open(&path)?;
        if boundary.contains("Promote") {
            let fixture = Fixture {
                root: authority.current_root()?,
                policy: std::env::var("ATTOW_CRASH_POLICY")?.parse()?,
                payload: std::env::var("ATTOW_CRASH_PAYLOAD")?.parse()?,
            };
            authority.set_boundary_hook(Some(boundary_hook));
            authority.promote(fixture.request()?)?;
        } else {
            let intent = std::env::var("ATTOW_CRASH_INTENT")?.parse()?;
            if boundary.contains("Claim") {
                authority.set_boundary_hook(Some(boundary_hook));
                authority.claim_effect(intent)?;
            } else if boundary.contains("Outcome") {
                let ticket = authority.claim_effect(intent)?;
                assert!(ticket.may_dispatch());
                let evidence =
                    verify_evidence(&OwnedReceiptVerifier, &ticket, b"owned-simulator-confirmed")?;
                authority.set_boundary_hook(Some(boundary_hook));
                authority.record_outcome(ticket, evidence)?;
            } else {
                return Err("unknown child boundary".into());
            }
        }
        return Err("ledger operation did not invoke requested boundary".into());
    }
    fs::write(marker, b"ready for abrupt termination")?;
    loop {
        thread::park();
    }
}

fn promotion_crash_case(boundary: &str, committed: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("promotion.db");
    let mut authority = Authority::open(&path)?;
    let fixture = Fixture::initialize(&mut authority)?;
    let baseline = authority.rebuild_and_verify()?;
    drop(authority);
    let raw_before = raw_snapshot(&path, None)?;
    crash_at(directory.path(), &path, boundary, &fixture, None)?;
    let raw_after = raw_snapshot(&path, None)?;
    if committed {
        assert_eq!(raw_after.requests, raw_before.requests + 1);
        assert_eq!(raw_after.intents, raw_before.intents + 1);
        assert_eq!(raw_after.outbox, raw_before.outbox + 1);
        assert_eq!(raw_after.slots, raw_before.slots + 1);
        assert_eq!(raw_after.rights, (1, 1, 0));
    } else {
        assert_eq!(
            raw_after, raw_before,
            "precommit kill must roll back CAS, pins, root, dedup, outbox, event, and reservation together"
        );
    }
    let mut authority = Authority::open(&path)?;
    let report = authority.rebuild_and_verify()?;
    assert_eq!(
        authority.current_root()?.get(b"winner")?,
        committed.then(|| b"pending".to_vec())
    );
    if committed {
        assert!(report.events > baseline.events);
    } else {
        assert_eq!(report.events, baseline.events);
        assert_eq!(report.head, baseline.head);
        assert_eq!(report.state_root, baseline.state_root);
    }
    let intent = accepted(authority.promote(fixture.request()?)?)?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Authorized);
    let after = authority.rebuild_and_verify()?;
    if committed {
        assert_eq!(
            after.head, report.head,
            "duplicate retry must not append an event"
        );
    }
    Ok(())
}

#[test]
fn promotion_before_commit_rolls_back_all_authority_changes() -> TestResult {
    promotion_crash_case("BeforePromoteCommit", false)
}

#[test]
fn promotion_after_commit_replays_one_durable_result() -> TestResult {
    promotion_crash_case("AfterPromoteCommit", true)
}

fn effect_crash_case(boundary: &str, expected: EffectState) -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("effect.db");
    let mut authority = Authority::open(&path)?;
    let fixture = Fixture::initialize(&mut authority)?;
    let intent = accepted(authority.promote(fixture.request()?)?)?;
    let managed = authority.current_root()?.object_id()?;
    drop(authority);
    crash_at(directory.path(), &path, boundary, &fixture, Some(intent))?;
    let raw = raw_snapshot(&path, Some(intent))?;
    let raw_state = match boundary {
        "BeforeClaimCommit" => 0,
        "AfterClaimCommit" | "BeforeOutcomeCommit" => 1,
        "AfterOutcomeCommit" => 3,
        _ => return Err("unexpected effect boundary".into()),
    };
    assert_eq!(
        (raw.effect_state, raw.outbox_state),
        (Some(raw_state), Some(raw_state))
    );
    assert_eq!(raw.evidence, i64::from(boundary == "AfterOutcomeCommit"));
    assert_eq!(
        raw.rights,
        if boundary == "AfterOutcomeCommit" {
            (1, 0, 1)
        } else {
            (1, 1, 0)
        }
    );
    assert_eq!(
        raw.slots, 1,
        "pending, ambiguous, and confirmed operations retain their semantic slot"
    );
    let authority = Authority::open(&path)?;
    assert_eq!(authority.effect_state(intent)?, expected);
    assert_eq!(authority.current_root()?.object_id()?, managed);
    let first = authority.rebuild_and_verify()?;
    let second = authority.rebuild_and_verify()?;
    assert_eq!(
        (first.events, first.head, first.state_root),
        (second.events, second.head, second.state_root)
    );
    Ok(())
}

#[test]
fn claim_before_commit_remains_authorized() -> TestResult {
    effect_crash_case("BeforeClaimCommit", EffectState::Authorized)
}

#[test]
fn claim_after_commit_recovers_as_unknown_even_without_a_send() -> TestResult {
    effect_crash_case("AfterClaimCommit", EffectState::Unknown)
}

#[test]
fn outcome_before_commit_recovers_as_unknown() -> TestResult {
    effect_crash_case("BeforeOutcomeCommit", EffectState::Unknown)
}

#[test]
fn outcome_after_commit_preserves_confirmed_evidence() -> TestResult {
    effect_crash_case("AfterOutcomeCommit", EffectState::Confirmed)
}

fn recovery_crash_case(boundary: &str, committed: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("recovery.db");
    let mut authority = Authority::open(&path)?;
    let fixture = Fixture::initialize(&mut authority)?;
    let intent = accepted(authority.promote(fixture.request()?)?)?;
    let ticket = authority.claim_effect(intent)?;
    assert!(ticket.may_dispatch());
    drop(authority);
    let before = raw_snapshot(&path, Some(intent))?;
    assert_eq!(before.effect_state, Some(1));
    crash_at(directory.path(), &path, boundary, &fixture, Some(intent))?;
    let after = raw_snapshot(&path, Some(intent))?;
    if committed {
        assert_eq!(after.events, before.events + 1);
        assert_eq!((after.effect_state, after.outbox_state), (Some(2), Some(2)));
    } else {
        assert_eq!(
            after, before,
            "precommit recovery crash must leave the old durable dispatching record intact"
        );
    }
    assert_eq!(after.rights, (1, 1, 0));
    assert_eq!(after.slots, 1);
    let authority = Authority::open(&path)?;
    assert_eq!(authority.effect_state(intent)?, EffectState::Unknown);
    let verified = authority.rebuild_and_verify()?;
    assert_eq!(verified.events, u64::try_from(before.events + 1)?);
    drop(authority);
    let reopened = Authority::open(&path)?;
    assert_eq!(
        reopened.rebuild_and_verify()?.head,
        verified.head,
        "repeated startup must not append another recovery event"
    );
    Ok(())
}

#[test]
fn recovery_before_commit_can_be_retried_without_losing_reservation() -> TestResult {
    recovery_crash_case("BeforeRecoveryCommit", false)
}

#[test]
fn recovery_after_commit_is_idempotent_on_next_startup() -> TestResult {
    recovery_crash_case("AfterRecoveryCommit", true)
}

#[test]
fn child_process_kill_does_not_run_destructors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("ready");
    let dropped = directory.path().join("dropped");
    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "crash_child_entry", "--nocapture"])
        .env("ATTOW_CRASH_MARKER", &marker)
        .env("ATTOW_CRASH_DROP", &dropped)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    kill_at_marker(&mut child, &marker)?;
    assert!(
        !dropped.exists(),
        "child destruction would turn this into a graceful-close test"
    );
    Ok(())
}
