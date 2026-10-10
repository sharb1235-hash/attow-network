use attow_cli::{BranchRecord, RunRecord, Workspace, bundle::Bundle};
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, ObjectId, Value, object_id};
use chronos_local::core::persistence::LocalStateStorage;
use chronos_state::{DomainId, StateRoot, StateStorage, fork};
use std::{collections::BTreeMap, error::Error};
fn fixture(workspace: &Workspace) -> Result<RunRecord, Box<dyn Error>> {
    let initial = StateRoot::new(DomainId([3; 32]));
    let mut branch = fork(initial.domain(), &initial, [4; 32])?;
    branch.put(b"owned".to_vec(), b"changed".to_vec())?;
    let branch = branch.freeze()?;
    let predicted = initial.apply(branch.overlay())?;
    let mut store = workspace.store()?;
    let descriptor = branch.admit(&mut LocalStateStorage(&mut store), "bundle-fixture")?;
    LocalStateStorage(&mut store).transaction(|tx| {
        tx.put_object(&chronos_state::ObjectData {
            kind: OBJECT_KIND,
            schema: OBJECT_SCHEMA,
            bytes: predicted.encode()?,
        })?;
        Ok(())
    })?;
    let mut record = RunRecord::new("11".repeat(32), "data-change".into());
    record.phase = "created".into();
    record.initial_root = Some(initial.object_id()?.to_string());
    record.managed_root = Some(initial.object_id()?.to_string());
    record.selected_branch = Some(ObjectId(branch.id().0).to_string());
    record.branches.push(BranchRecord {
        branch_id: ObjectId(branch.id().0).to_string(),
        descriptor: descriptor.to_string(),
        label: "original fixture".into(),
        state_root: predicted.object_id()?.to_string(),
        observation_root: None,
    });
    workspace.save_run(&record)?;
    Ok(record)
}
#[test]
fn data_bundle_roundtrip_is_immutable_and_carries_no_writer_state() -> Result<(), Box<dyn Error>> {
    let first = tempfile::tempdir()?;
    let source = Workspace::init(first.path())?;
    let record = fixture(&source)?;
    let bundle = Bundle::gather(&source, record.clone())?;
    let bytes = bundle.encode()?;
    assert!(
        !bytes
            .windows(source.writer_id()?.to_string().len())
            .any(|w| w == source.writer_id().expect("writer").to_string().as_bytes())
    );
    let decoded = Bundle::decode(&bytes)?;
    assert_eq!(decoded.record, record);
    let second = tempfile::tempdir()?;
    let target = Workspace::init(second.path())?;
    let writer = target.writer_id()?;
    decoded.import(&target)?;
    assert_eq!(target.load_run(&record.run_id)?, record);
    assert!(!target.is_native(&record.run_id)?);
    assert!(target.save_run(&record).is_err());
    assert_eq!(target.writer_id()?, writer);
    Ok(())
}
#[test]
fn bundle_rejects_tamper_missing_objects_and_paths() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let workspace = Workspace::init(temp.path())?;
    let record = fixture(&workspace)?;
    let mut bundle = Bundle::gather(&workspace, record)?;
    let good = bundle.encode()?;
    let first = *bundle.objects.keys().next().ok_or("empty fixture")?;
    bundle.objects.remove(&first);
    assert!(bundle.encode().is_err());
    let mut decoded = Bundle::decode(&good)?;
    decoded.record.managed_root = Some("ff".repeat(32));
    assert!(decoded.encode().is_err());
    let mut decoded = Bundle::decode(&good)?;
    decoded.record.bundle_path = "../../escape".into();
    assert!(decoded.encode().is_err());
    let mut trailing = good;
    trailing.push(0);
    assert!(Bundle::decode(&trailing).is_err());
    assert!(Bundle::decode(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
    Ok(())
}
#[test]
fn arbitrary_json_root_is_not_managed_state_evidence() -> Result<(), Box<dyn Error>> {
    let mut record = RunRecord::new("22".repeat(32), "data-change".into());
    record.managed_root = Some("33".repeat(32));
    assert!(
        Bundle {
            record,
            objects: BTreeMap::new()
        }
        .encode()
        .is_err()
    );
    let value = Value::Array(vec![Value::Unsigned(1)]).encode()?;
    assert_ne!(
        object_id(OBJECT_KIND, OBJECT_SCHEMA, &value),
        ObjectId([0; 32])
    );
    assert!(Value::decode(&value, &Limits::default()).is_ok());
    Ok(())
}

#[test]
fn completed_phase_requires_actual_execution_and_effect_evidence() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let workspace = Workspace::init(temp.path())?;
    let record = fixture(&workspace)?;
    let mut bundle = Bundle::gather(&workspace, record)?;
    bundle.record.phase = "completed".into();
    bundle.record.managed_root = Some(bundle.record.branches[0].state_root.clone());
    assert!(
        bundle.encode().is_err(),
        "completed record without trace or promotion identity was admitted"
    );
    bundle.record.scenario = "checkout".into();
    assert!(
        bundle.encode().is_err(),
        "completed checkout without durable receipt was admitted"
    );
    Ok(())
}

#[test]
fn full_catalog_import_rejection_publishes_no_objects_or_pins() -> Result<(), Box<dyn Error>> {
    let source_dir = tempfile::tempdir()?;
    let source = Workspace::init(source_dir.path())?;
    let record = fixture(&source)?;
    let bundle = Bundle::gather(&source, record)?;
    let target_dir = tempfile::tempdir()?;
    let target = Workspace::init(target_dir.path())?;
    let mut catalog = rusqlite::Connection::open(target.data_dir().join("catalog.sqlite"))?;
    let tx = catalog.transaction()?;
    for index in 0..256 {
        let record = RunRecord::new(format!("{index:064x}"), "data-change".into());
        tx.execute(
            "INSERT INTO runs(id,native,record) VALUES(?1,1,?2)",
            rusqlite::params![record.run_id, serde_json::to_vec(&record)?],
        )?;
    }
    tx.commit()?;
    let cas = rusqlite::Connection::open(target.authority_path())?;
    let count = || -> rusqlite::Result<(i64, i64)> {
        Ok((
            cas.query_row("SELECT count(*) FROM objects", [], |row| row.get(0))?,
            cas.query_row("SELECT count(*) FROM root_pins", [], |row| row.get(0))?,
        ))
    };
    let before = count()?;
    assert!(bundle.import(&target).is_err());
    assert_eq!(
        count()?,
        before,
        "rejected import retained CAS data or owner pins"
    );
    Ok(())
}

#[test]
fn interrupted_import_reservations_are_hidden_bounded_and_resumable() -> Result<(), Box<dyn Error>>
{
    let source_dir = tempfile::tempdir()?;
    let source = Workspace::init(source_dir.path())?;
    let record = fixture(&source)?;
    let bundle = Bundle::gather(&source, record)?;
    let target_dir = tempfile::tempdir()?;
    let target = Workspace::init(target_dir.path())?;
    let connection = rusqlite::Connection::open(target.data_dir().join("catalog.sqlite"))?;
    connection.execute(
        "INSERT INTO runs(id,native,record,ready) VALUES(?1,0,?2,0)",
        rusqlite::params![bundle.record.run_id, serde_json::to_vec(&bundle.record)?],
    )?;
    assert!(target.load_run(&bundle.record.run_id).is_err());
    assert!(target.list_runs()?.is_empty());
    assert!(target.save_run(&bundle.record).is_err());
    bundle.import(&target)?;
    assert_eq!(target.load_run(&bundle.record.run_id)?, bundle.record);
    // Simulate the other durable boundary: CAS committed but catalog publication
    // interrupted. A retry reuses the same owner rather than adding retention.
    connection.execute(
        "UPDATE runs SET ready=0 WHERE id=?1",
        [&bundle.record.run_id],
    )?;
    let cas = rusqlite::Connection::open(target.authority_path())?;
    let before: i64 = cas.query_row("SELECT count(*) FROM root_pins", [], |row| row.get(0))?;
    bundle.import(&target)?;
    let after: i64 = cas.query_row("SELECT count(*) FROM root_pins", [], |row| row.get(0))?;
    assert_eq!(before, after);
    assert!(!target.is_native(&bundle.record.run_id)?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_response_rejects_a_baseline_only_model_trace() -> Result<(), Box<dyn Error>> {
    use attow_cli::demo::{self, DemoOptions, Scenario};
    use chronos_codec::Object;
    use chronos_scheduler::{ExecutionTrace, SemanticEvent};
    let dir = tempfile::tempdir()?;
    let workspace = Workspace::init(dir.path())?;
    let mut record = demo::run(
        &workspace,
        DemoOptions {
            scenario: Scenario::Checkout,
            workers: 2,
            resume: None,
            browser: false,
            pause_after_unknown: false,
            boundary_hook: None,
        },
    )
    .await?;
    Bundle::gather(&workspace, record.clone())?;
    let mut store = workspace.store()?;
    let old_trace: ObjectId = record
        .trace_roots
        .last()
        .ok_or("missing response trace")?
        .parse()?;
    let mut trace = ExecutionTrace::decode(&store.get(old_trace)?, &Limits::default())?;
    let boundary = trace
        .events
        .iter()
        .position(|event| matches!(event, SemanticEvent::ObservationReplaced { .. }))
        .ok_or("missing replacement")?;
    trace.events.truncate(boundary);
    let new_trace = store.put(OBJECT_KIND, OBJECT_SCHEMA, &trace.encode()?)?;
    let descriptor_id: ObjectId = record
        .response_descriptor
        .as_deref()
        .ok_or("missing descriptor")?
        .parse()?;
    let mut descriptor = Object::decode(&store.get(descriptor_id)?, &Limits::default())?;
    let Value::Array(fields) = &mut descriptor.payload else {
        return Err("descriptor shape".into());
    };
    let Value::Array(worldline) = &mut fields[9] else {
        return Err("worldline shape".into());
    };
    let Value::Bytes(bindings) = &worldline[6] else {
        return Err("bindings shape".into());
    };
    let old_bindings = ObjectId(bindings.as_slice().try_into()?);
    let mut bindings = Object::decode(&store.get(old_bindings)?, &Limits::default())?;
    assert!(bindings.refs.remove(&old_trace));
    bindings.refs.insert(new_trace);
    let new_bindings = store.put(OBJECT_KIND, OBJECT_SCHEMA, &bindings.encode()?)?;
    worldline[6] = Value::Bytes(new_bindings.0.to_vec());
    assert!(descriptor.refs.remove(&old_bindings));
    descriptor.refs.insert(new_bindings);
    record.response_descriptor = Some(
        store
            .put(OBJECT_KIND, OBJECT_SCHEMA, &descriptor.encode()?)?
            .to_string(),
    );
    *record.trace_roots.last_mut().ok_or("trace")? = new_trace.to_string();
    for event in &mut record.events {
        if event["kind"] == "receipt_rerun" {
            event["trace"] = serde_json::json!(new_trace.to_string());
        }
    }
    assert!(
        Bundle::gather(&workspace, record).is_err(),
        "response accepted a model-only trace without consuming the confirmed receipt"
    );
    Ok(())
}
