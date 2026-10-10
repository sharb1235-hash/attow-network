use attow_adapters::{
    Adapter, AdapterError, AdapterStore, WorkerBinding,
    sql::{SqlAdapter, SqlValue, SqlWorker, store_query},
};
use chronos_codec::{ObjectId, Value};
use chronos_state::execution::{
    EffectClass, ExecutionError, ResourceClass, StepExit, StepRequest, WorkerBackend,
};
use rusqlite::Connection;
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn owned(
    dir: &std::path::Path,
) -> Result<(Connection, AdapterStore, SqlAdapter), Box<dyn std::error::Error>> {
    let path = dir.join("owned.sqlite");
    let connection = Connection::open(&path)?;
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT NOT NULL); INSERT INTO items VALUES(1,'one'); PRAGMA wal_checkpoint(TRUNCATE);")?;
    let store = AdapterStore::open(&dir.join("objects.sqlite"))?;
    let adapter = SqlAdapter::open_owned(&path, ObjectId([1; 32]), store.clone())?;
    Ok((connection, store, adapter))
}
#[test]
fn online_backup_includes_wal_and_restores_only_matching_owner() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (writer, store, mut adapter) = owned(directory.path())?;
    writer.execute("INSERT INTO items VALUES(2,'two')", [])?;
    assert!(directory.path().join("owned.sqlite-wal").metadata()?.len() > 0);
    let snapshot = adapter.snapshot()?;
    writer.execute("INSERT INTO items VALUES(3,'three')", [])?;
    let saved = adapter.read_snapshot(snapshot, "SELECT id,name FROM items ORDER BY id,name")?;
    assert_eq!(saved.rows.len(), 2);
    assert_eq!(
        saved.rows[1],
        vec![SqlValue::Integer(2), SqlValue::Text("two".into())]
    );
    let other_path = directory.path().join("other.sqlite");
    Connection::open(&other_path)?
        .execute_batch("CREATE TABLE untouched(value INTEGER); INSERT INTO untouched VALUES(7)")?;
    let mut foreign = SqlAdapter::open_owned(&other_path, ObjectId([2; 32]), store)?;
    assert!(matches!(
        foreign.restore(snapshot),
        Err(AdapterError::OwnerMismatch)
    ));
    assert_eq!(
        Connection::open(&other_path)?
            .query_row("SELECT value FROM untouched", [], |row| row
                .get::<_, i64>(0))?,
        7
    );
    adapter.restore(snapshot)?;
    assert_eq!(
        writer.query_row("SELECT count(*) FROM items", [], |row| row.get::<_, i64>(0))?,
        2
    );
    Ok(())
}
#[test]
fn sql_subset_rejects_nondeterminism_writes_views_and_ambiguous_order() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (writer, _, mut adapter) = owned(directory.path())?;
    writer.execute_batch("CREATE VIEW changing AS SELECT random() AS value;")?;
    let snapshot = adapter.snapshot()?;
    assert_eq!(
        adapter
            .read_snapshot(snapshot, "SELECT id FROM items ORDER BY id")?
            .rows
            .len(),
        1
    );
    for sql in [
        "SELECT random() FROM items ORDER BY id",
        "SELECT datetime('now') FROM items ORDER BY id",
        "DELETE FROM items",
        "PRAGMA user_version=2",
        "SELECT id FROM items",
        "SELECT id,name FROM items ORDER BY id",
        "SELECT id FROM items ORDER BY id; DELETE FROM items",
        "SELECT value FROM changing ORDER BY value",
        "SELECT load_extension('x') FROM items ORDER BY id",
    ] {
        assert!(
            adapter.read_snapshot(snapshot, sql).is_err(),
            "accepted {sql}"
        );
    }
    assert_eq!(
        writer.query_row("SELECT count(*) FROM items", [], |row| row.get::<_, i64>(0))?,
        1
    );
    Ok(())
}
#[test]
fn result_row_and_byte_limits_reject_without_partial_success() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (writer, _, mut adapter) = owned(directory.path())?;
    writer.execute_batch("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<1025) INSERT INTO items SELECT x,'row' FROM n;")?;
    let snapshot = adapter.snapshot()?;
    assert!(matches!(
        adapter.read_snapshot(snapshot, "SELECT id FROM items ORDER BY id"),
        Err(AdapterError::Limit(_))
    ));
    writer
        .execute_batch("DELETE FROM items; INSERT INTO items VALUES(1, printf('%0300000d',0));")?;
    let snapshot = adapter.snapshot()?;
    assert!(
        adapter
            .read_snapshot(snapshot, "SELECT name FROM items ORDER BY name")
            .is_err()
    );
    Ok(())
}
fn binding() -> Result<WorkerBinding, AdapterError> {
    let budget = SqlWorker::default_budget();
    Ok(WorkerBinding {
        tenant: ObjectId([3; 32]),
        security_scope: ObjectId([1; 32]),
        policy: ObjectId([4; 32]),
        fidelity: SqlWorker::fidelity_id(),
        code: SqlWorker::code_id(),
        profile: SqlWorker::profile_id(&budget)?,
        effect_class: EffectClass::Pure,
        budget,
    })
}
#[test]
fn worker_reads_only_exact_admitted_snapshot_request_and_records_dependencies() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (_, store, mut adapter) = owned(directory.path())?;
    let snapshot = adapter.snapshot()?;
    let query = store_query(
        &store,
        "sql-query",
        "SELECT id,name FROM items ORDER BY id,name",
    )?;
    let binding = binding()?;
    let empty = store.put_object("empty", Value::Null, BTreeSet::new())?;
    let request = StepRequest {
        attempt: ObjectId([5; 32]),
        tenant: binding.tenant,
        security_scope: binding.security_scope,
        policy: binding.policy,
        fidelity: binding.fidelity,
        code: binding.code,
        profile: binding.profile,
        effect_class: binding.effect_class,
        budget: binding.budget,
        input_root: snapshot,
        continuation: query,
        observation_root: empty,
        sample_id: None,
        resource_class: ResourceClass::Sql,
    };
    let worker = SqlWorker::new(store.clone(), binding.clone())?;
    assert!(worker.permits_pure_reuse(&request));
    let outcome = worker.run_step(request.clone())?;
    outcome.validate_for(&request)?;
    let StepExit::Completed {
        result,
        observation_root,
        ..
    } = outcome.exit
    else {
        return Err("unexpected suspension".into());
    };
    assert_eq!(observation_root, empty);
    assert!(
        store
            .get_object(result)?
            .refs
            .is_superset(&BTreeSet::from([snapshot, query]))
    );
    for field in 0..11 {
        let mut changed = request.clone();
        let id = ObjectId([99; 32]);
        match field {
            0 => changed.tenant = id,
            1 => changed.security_scope = id,
            2 => changed.policy = id,
            3 => changed.fidelity = id,
            4 => changed.code = id,
            5 => changed.profile = id,
            6 => changed.effect_class = EffectClass::ManagedWrite,
            7 => changed.budget.fuel += 1,
            8 => changed.sample_id = Some(id),
            9 => changed.resource_class = ResourceClass::Browser,
            _ => changed.budget.output_bytes -= 1,
        };
        assert!(worker.run_step(changed).is_err(), "accepted field {field}");
    }
    let mut forged = binding;
    forged.code = empty;
    assert!(SqlWorker::new(store, forged).is_err());
    Ok(())
}
#[test]
fn worker_fuel_exhaustion_is_an_incomplete_attempt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (_, store, mut adapter) = owned(directory.path())?;
    let snapshot = adapter.snapshot()?;
    let query = store_query(&store, "query", "SELECT id FROM items ORDER BY id")?;
    let mut binding = binding()?;
    binding.budget.fuel = 1;
    binding.profile = SqlWorker::profile_id(&binding.budget)?;
    let request = StepRequest {
        attempt: ObjectId([5; 32]),
        tenant: binding.tenant,
        security_scope: binding.security_scope,
        policy: binding.policy,
        fidelity: binding.fidelity,
        code: binding.code,
        profile: binding.profile,
        effect_class: binding.effect_class,
        budget: binding.budget,
        input_root: snapshot,
        continuation: query,
        observation_root: snapshot,
        sample_id: None,
        resource_class: ResourceClass::Sql,
    };
    assert_eq!(
        SqlWorker::new(store, binding)?.run_step(request),
        Err(ExecutionError::ResourceExhausted)
    );
    Ok(())
}
#[test]
fn unknown_identifiers_never_become_sqlite_quoted_string_literals() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (_, _, mut adapter) = owned(directory.path())?;
    let snapshot = adapter.snapshot()?;
    assert!(
        adapter
            .read_snapshot(snapshot, "SELECT missing FROM items ORDER BY missing")
            .is_err()
    );
    Ok(())
}

#[test]
fn worker_rejects_invalid_snapshot_envelopes_before_pure_reuse_admission() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (_, store, mut adapter) = owned(directory.path())?;
    let original = store.get_object(adapter.snapshot()?)?;
    let query = store_query(&store, "query", "SELECT id FROM items ORDER BY id")?;
    let admission = binding()?;
    let empty = store.put_object("empty", Value::Null, BTreeSet::new())?;
    let worker = SqlWorker::new(store.clone(), admission.clone())?;
    for corruption in 0..4 {
        let Value::Array(mut fields) = original.payload.clone() else {
            return Err("snapshot envelope".into());
        };
        let mut refs = BTreeSet::new();
        match corruption {
            0 => fields[1] = Value::Unsigned(2),
            1 => fields[3] = Value::Text("unadmitted-sqlite-version".into()),
            2 => fields.push(Value::Null),
            _ => {
                refs.insert(empty);
            }
        }
        let input_root = store.put_object("invalid-snapshot", Value::Array(fields), refs)?;
        let request = StepRequest {
            attempt: ObjectId([5; 32]),
            tenant: admission.tenant,
            security_scope: admission.security_scope,
            policy: admission.policy,
            fidelity: admission.fidelity,
            code: admission.code,
            profile: admission.profile,
            effect_class: admission.effect_class,
            budget: admission.budget,
            input_root,
            continuation: query,
            observation_root: empty,
            sample_id: None,
            resource_class: ResourceClass::Sql,
        };
        assert_eq!(
            worker.validate_request(&request),
            Err(ExecutionError::InvalidRequest),
            "snapshot corruption {corruption} admitted"
        );
        assert!(!worker.permits_pure_reuse(&request));
    }
    Ok(())
}

#[test]
fn worker_output_budget_counts_the_full_canonical_result_envelope() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (writer, store, mut adapter) = owned(directory.path())?;
    writer.execute("DELETE FROM items", [])?;
    let snapshot = adapter.snapshot()?;
    let query = store_query(&store, "query", "SELECT id FROM items ORDER BY id")?;
    let mut admission = binding()?;
    admission.budget.output_bytes = 64;
    admission.profile = SqlWorker::profile_id(&admission.budget)?;
    let empty = store.put_object("empty", Value::Null, BTreeSet::new())?;
    let worker = SqlWorker::new(store, admission.clone())?;
    let request = StepRequest {
        attempt: ObjectId([5; 32]),
        tenant: admission.tenant,
        security_scope: admission.security_scope,
        policy: admission.policy,
        fidelity: admission.fidelity,
        code: admission.code,
        profile: admission.profile,
        effect_class: admission.effect_class,
        budget: admission.budget,
        input_root: snapshot,
        continuation: query,
        observation_root: empty,
        sample_id: None,
        resource_class: ResourceClass::Sql,
    };
    let object_count = || -> Result<i64, Box<dyn std::error::Error>> {
        Ok(
            Connection::open(directory.path().join("objects.sqlite"))?.query_row(
                "SELECT count(*) FROM objects",
                [],
                |row| row.get(0),
            )?,
        )
    };
    let before = object_count()?;
    assert_eq!(
        worker.run_step(request),
        Err(ExecutionError::ResourceExhausted)
    );
    assert_eq!(
        object_count()?,
        before,
        "oversized result published a partial object"
    );
    Ok(())
}
