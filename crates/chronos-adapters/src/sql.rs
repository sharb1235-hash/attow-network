//! Owned SQLite snapshots and a deliberately small deterministic read subset.
use crate::{
    Adapter, AdapterCapabilities, AdapterError, AdapterLimits, AdapterManifest, AdapterStore,
    Fidelity, Observation, ObservationRequest, StorageSupport, WorkerBinding,
};
use chronos_codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id};
use chronos_state::execution::{
    EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest,
    WorkerBackend,
};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
    limits::Limit,
    types::ValueRef,
};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
pub const MAX_RESULT_BYTES: usize = 256 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Text(String),
    Blob(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlResult {
    pub snapshot: ObjectId,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}
pub struct SqlAdapter {
    connection: Connection,
    scope: ObjectId,
    store: AdapterStore,
}
impl SqlAdapter {
    /// Owned physical capture/restore and the declared immutable read subset.
    /// The deadline bounds backup stepping, not operating-system scheduling.
    pub fn manifest() -> AdapterManifest {
        AdapterManifest {
            version: 1,
            name: "attow-owned-sqlite".into(),
            fidelity: Fidelity::OwnedEnvironment,
            storage: StorageSupport::default(),
            deterministic_subset: vec!["Immutable snapshot: SELECT identifiers FROM ordinary-table ORDER BY all selected columns".into()],
            effect_semantics: vec!["Online backup and owner-scoped restore; no remote writes, functions, views or floats".into()],
            limits: AdapterLimits {
                snapshot_bytes: MAX_SNAPSHOT_BYTES,
                output_bytes: MAX_RESULT_BYTES,
                deadline_ms: 2_000,
            },
        }
    }
    pub fn open_owned(
        path: &Path,
        scope: ObjectId,
        store: AdapterStore,
    ) -> Result<Self, AdapterError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "trusted_schema", false)?;
        Ok(Self {
            connection,
            scope,
            store,
        })
    }
    pub fn read_snapshot(&self, root: ObjectId, sql: &str) -> Result<SqlResult, AdapterError> {
        read(
            &self.store,
            self.scope,
            root,
            sql,
            SqlWorker::default_budget(),
        )
    }
}
impl Adapter for SqlAdapter {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            fidelity: Fidelity::OwnedEnvironment,
            snapshot: true,
            restore: true,
            ..AdapterCapabilities::default()
        }
    }
    fn snapshot(&mut self) -> Result<ObjectId, AdapterError> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("snapshot.sqlite");
        let mut destination = Connection::open(&path)?;
        let transaction = self.connection.transaction()?;
        let pages: i64 = transaction.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let size: i64 = transaction.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        if pages < 0
            || size <= 0
            || pages
                .checked_mul(size)
                .is_none_or(|bytes| bytes > MAX_SNAPSHOT_BYTES as i64)
        {
            return Err(AdapterError::Limit("snapshot bytes"));
        }
        bounded_backup(&transaction, &mut destination)?;
        transaction.rollback()?;
        drop(destination);
        if path.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
            return Err(AdapterError::Limit("snapshot bytes"));
        }
        let bytes = std::fs::read(path)?;
        self.store.put_object(
            &format!("sql:snapshot:{}", self.scope),
            Value::Array(vec![
                Value::Text("attow-sql-snapshot".into()),
                Value::Unsigned(1),
                Value::Bytes(self.scope.0.to_vec()),
                Value::Text(rusqlite::version().into()),
                Value::Bytes(bytes),
            ]),
            BTreeSet::new(),
        )
    }
    fn restore(&mut self, root: ObjectId) -> Result<(), AdapterError> {
        let (_directory, source) = materialize(&self.store, self.scope, root)?;
        bounded_backup(&source, &mut self.connection)
    }
    fn observe(&mut self, request: ObservationRequest) -> Result<Observation, AdapterError> {
        if request.method != "query" {
            return Err(AdapterError::Unsupported);
        }
        let query = request.input.ok_or(AdapterError::InvalidRequest)?;
        let sql = query_text(&self.store, query)?;
        let snapshot = self.snapshot()?;
        let result = self.read_snapshot(snapshot, &sql)?;
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AdapterError::BackendUnavailable)?
            .as_millis();
        let captured_at_unix_ms =
            u64::try_from(time).map_err(|_| AdapterError::Limit("capture time"))?;
        let provenance = self
            .store
            .put_object("sql:provenance", code_value(), BTreeSet::new())?;
        let root = self.store.put_object(
            "sql:observations",
            Value::Array(vec![
                Value::Text("attow-sql-observation".into()),
                Value::Unsigned(1),
                Value::Unsigned(captured_at_unix_ms),
                Value::Text("OwnedEnvironment".into()),
                result_value(&result),
            ]),
            BTreeSet::from([snapshot, query, provenance]),
        )?;
        Ok(Observation {
            root,
            provenance,
            fidelity: Fidelity::OwnedEnvironment,
            captured_at_unix_ms,
        })
    }
}
fn bounded_backup(source: &Connection, destination: &mut Connection) -> Result<(), AdapterError> {
    let backup = Backup::new(source, destination)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    for _ in 0..128 {
        if Instant::now() > deadline {
            return Err(AdapterError::Timeout);
        }
        match backup.step(32)? {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            _ => return Err(AdapterError::BackendUnavailable),
        }
    }
    Err(AdapterError::Limit("backup steps"))
}
fn materialize(
    store: &AdapterStore,
    scope: ObjectId,
    root: ObjectId,
) -> Result<(tempfile::TempDir, Connection), AdapterError> {
    let bytes = snapshot_bytes(store, scope, root)?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("snapshot.sqlite");
    std::fs::write(&path, bytes)?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA temp_store=MEMORY;",
    )?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_RESULT_BYTES as i32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 128)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 4096)?;
    connection.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 16)?;
    connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 1)?;
    connection.set_limit(Limit::SQLITE_LIMIT_WORKER_THREADS, 0)?;
    let check: String = connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(AdapterError::InvalidSnapshot);
    }
    Ok((directory, connection))
}
// Admission and materialization share the complete versioned-envelope check.
fn snapshot_bytes(
    store: &AdapterStore,
    scope: ObjectId,
    root: ObjectId,
) -> Result<Vec<u8>, AdapterError> {
    let object = store.get_object(root)?;
    let Value::Array(fields) = object.payload else {
        return Err(AdapterError::InvalidSnapshot);
    };
    let fields: [Value; 5] = fields
        .try_into()
        .map_err(|_| AdapterError::InvalidSnapshot)?;
    let [
        Value::Text(tag),
        Value::Unsigned(1),
        Value::Bytes(owner),
        Value::Text(version),
        Value::Bytes(bytes),
    ] = fields
    else {
        return Err(AdapterError::InvalidSnapshot);
    };
    if tag != "attow-sql-snapshot"
        || version != rusqlite::version()
        || !object.refs.is_empty()
        || bytes.len() > MAX_SNAPSHOT_BYTES
    {
        return Err(AdapterError::InvalidSnapshot);
    }
    if owner.as_slice() != scope.0 {
        return Err(AdapterError::OwnerMismatch);
    }
    Ok(bytes)
}
struct Query {
    columns: Vec<String>,
    table: String,
}
fn parse_query(sql: &str) -> Result<Query, AdapterError> {
    fn identifier(value: &str) -> bool {
        let mut bytes = value.bytes();
        matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
            && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && value.len() <= 64
    }
    if sql.len() > 4096 || !sql.is_ascii() {
        return Err(AdapterError::Limit("query text"));
    }
    let fields: Vec<_> = sql.split_whitespace().collect();
    let ["SELECT", columns, "FROM", table, "ORDER", "BY", order] = fields.as_slice() else {
        return Err(AdapterError::Unsupported);
    };
    let columns: Vec<_> = columns.split(',').map(str::to_owned).collect();
    if columns.is_empty()
        || columns.len() > 16
        || !identifier(table)
        || table.starts_with("sqlite_")
        || columns.iter().any(|column| !identifier(column))
        || columns.join(",") != *order
        || columns.iter().collect::<BTreeSet<_>>().len() != columns.len()
    {
        return Err(AdapterError::Unsupported);
    }
    Ok(Query {
        columns,
        table: table.to_string(),
    })
}
pub fn store_query(store: &AdapterStore, owner: &str, sql: &str) -> Result<ObjectId, AdapterError> {
    parse_query(sql)?;
    store.put_object(
        owner,
        Value::Array(vec![
            Value::Text("attow-sql-query".into()),
            Value::Unsigned(1),
            Value::Text(sql.into()),
        ]),
        BTreeSet::new(),
    )
}
fn query_text(store: &AdapterStore, root: ObjectId) -> Result<String, AdapterError> {
    let object = store.get_object(root)?;
    let Value::Array(fields) = object.payload else {
        return Err(AdapterError::InvalidRequest);
    };
    let [Value::Text(tag), Value::Unsigned(1), Value::Text(sql)] = fields.as_slice() else {
        return Err(AdapterError::InvalidRequest);
    };
    if tag != "attow-sql-query" || !object.refs.is_empty() {
        return Err(AdapterError::InvalidRequest);
    }
    parse_query(sql)?;
    Ok(sql.clone())
}
fn read(
    store: &AdapterStore,
    scope: ObjectId,
    snapshot: ObjectId,
    sql: &str,
    budget: ResourceBudget,
) -> Result<SqlResult, AdapterError> {
    let query = parse_query(sql)?;
    let (_directory, connection) = materialize(store, scope, snapshot)?;
    let fuel = Arc::new(AtomicU64::new(budget.fuel));
    let remaining = fuel.clone();
    connection.progress_handler(
        1,
        Some(move || {
            remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
        }),
    )?;
    let operation = (|| {
        let kind: String = connection.query_row(
            "SELECT type FROM sqlite_schema WHERE name=?1",
            [&query.table],
            |row| row.get(0),
        )?;
        let definition: String = connection.query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [&query.table],
            |row| row.get(0),
        )?;
        if kind != "table"
            || definition.to_ascii_uppercase().contains("VIRTUAL")
            || definition.to_ascii_uppercase().contains("GENERATED")
        {
            return Err(AdapterError::Unsupported);
        }
        // Quoted unknown identifiers can otherwise become SQLite string literals.
        let mut metadata = connection.prepare("SELECT name,hidden FROM pragma_table_xinfo(?1)")?;
        let mut metadata_rows = metadata.query([&query.table])?;
        let mut available = BTreeSet::new();
        while let Some(row) = metadata_rows.next()? {
            if available.len() >= 128 {
                return Err(AdapterError::Limit("table columns"));
            }
            if row.get::<_, i64>(1)? != 0 {
                return Err(AdapterError::Unsupported);
            }
            available.insert(row.get::<_, String>(0)?);
        }
        if query
            .columns
            .iter()
            .any(|column| !available.contains(column))
        {
            return Err(AdapterError::Unsupported);
        }
        let quoted = query
            .columns
            .iter()
            .map(|column| format!("\"{column}\""))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT {quoted} FROM \"{}\" ORDER BY {quoted}", query.table);
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query([])?;
        let mut values = Vec::new();
        let mut bytes = 0_u64;
        while let Some(row) = rows.next()? {
            if values.len() >= 1024 {
                return Err(AdapterError::Limit("rows"));
            }
            let mut cells = Vec::new();
            for index in 0..query.columns.len() {
                let raw = row.get_ref(index)?;
                let size = match raw {
                    ValueRef::Text(v) | ValueRef::Blob(v) => v.len() as u64,
                    _ => 8,
                };
                bytes = bytes
                    .checked_add(size + 16)
                    .ok_or(AdapterError::Limit("output bytes"))?;
                if bytes > budget.output_bytes {
                    return Err(AdapterError::Limit("output bytes"));
                }
                cells.push(match raw {
                    ValueRef::Null => SqlValue::Null,
                    ValueRef::Integer(n) => SqlValue::Integer(n),
                    ValueRef::Text(v) => SqlValue::Text(
                        std::str::from_utf8(v)
                            .map_err(|_| AdapterError::InvalidSnapshot)?
                            .into(),
                    ),
                    ValueRef::Blob(v) => SqlValue::Blob(v.to_vec()),
                    ValueRef::Real(_) => return Err(AdapterError::Unsupported),
                });
            }
            values.push(cells);
        }
        Ok(SqlResult {
            snapshot,
            columns: query.columns,
            rows: values,
        })
    })();
    if operation.is_err() && fuel.load(Ordering::Relaxed) == 0 {
        return Err(AdapterError::Limit("fuel"));
    }
    operation
}
fn result_value(result: &SqlResult) -> Value {
    Value::Array(vec![
        Value::Text("attow-sql-result".into()),
        Value::Unsigned(1),
        Value::Array(result.columns.iter().cloned().map(Value::Text).collect()),
        Value::Array(
            result
                .rows
                .iter()
                .map(|row| {
                    Value::Array(
                        row.iter()
                            .map(|cell| match cell {
                                SqlValue::Null => Value::Null,
                                SqlValue::Integer(n) => {
                                    if *n >= 0 {
                                        Value::Unsigned(*n as u64)
                                    } else {
                                        Value::Negative((-1_i128 - i128::from(*n)) as u64)
                                    }
                                }
                                SqlValue::Text(text) => Value::Text(text.clone()),
                                SqlValue::Blob(bytes) => Value::Bytes(bytes.clone()),
                            })
                            .collect(),
                    )
                })
                .collect(),
        ),
    ])
}
fn code_value() -> Value {
    Value::Array(vec![Value::Text("attow-sql-engine".into()),Value::Unsigned(1),Value::Text("SELECT identifiers FROM ordinary-table ORDER BY all-selected-columns; no floats, functions, writes, samples".into())])
}
fn fidelity_value() -> Value {
    Value::Array(vec![
        Value::Text("attow-sql-fidelity".into()),
        Value::Unsigned(1),
        Value::Text("RecordedReplay:immutable-owned-snapshot".into()),
    ])
}
fn identity(payload: Value) -> ObjectId {
    let bytes = Object {
        refs: BTreeSet::new(),
        payload,
    }
    .encode()
    .expect("bounded internal SQL descriptor");
    object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes)
}
fn profile_value(budget: &ResourceBudget) -> Result<Value, AdapterError> {
    if !(1..=10_000_000).contains(&budget.fuel)
        || budget.memory_bytes != 16 * 1024 * 1024
        || !(1..=MAX_RESULT_BYTES as u64).contains(&budget.output_bytes)
        || budget.host_calls != 1
    {
        return Err(AdapterError::Unsupported);
    }
    Ok(Value::Array(vec![Value::Text("attow-sql-profile".into()),Value::Unsigned(1),Value::Text(rusqlite::version().into()),Value::Text("rusqlite-0.40.2; snapshot512KiB; rows1024; columns16; quick-check; explicit-order; logical-memory-not-RSS".into()),Value::Unsigned(budget.fuel),Value::Unsigned(budget.memory_bytes),Value::Unsigned(budget.output_bytes),Value::Unsigned(budget.host_calls.into())]))
}
pub struct SqlWorker {
    store: AdapterStore,
    binding: WorkerBinding,
    delta: ObjectId,
}
impl SqlWorker {
    pub fn new(store: AdapterStore, binding: WorkerBinding) -> Result<Self, AdapterError> {
        if binding.code != Self::code_id()
            || binding.profile != Self::profile_id(&binding.budget)?
            || binding.fidelity != Self::fidelity_id()
            || binding.effect_class != EffectClass::Pure
        {
            return Err(AdapterError::Unsupported);
        }
        store.put_object("sql:code", code_value(), BTreeSet::new())?;
        store.put_object(
            "sql:profile",
            profile_value(&binding.budget)?,
            BTreeSet::new(),
        )?;
        store.put_object("sql:fidelity", fidelity_value(), BTreeSet::new())?;
        let delta = store.put_object("sql:empty-delta", Value::Null, BTreeSet::new())?;
        Ok(Self {
            store,
            binding,
            delta,
        })
    }
    pub fn default_budget() -> ResourceBudget {
        ResourceBudget {
            fuel: 1_000_000,
            memory_bytes: 16 * 1024 * 1024,
            output_bytes: MAX_RESULT_BYTES as u64,
            host_calls: 1,
        }
    }
    pub fn code_id() -> ObjectId {
        identity(code_value())
    }
    pub fn fidelity_id() -> ObjectId {
        identity(fidelity_value())
    }
    pub fn profile_id(budget: &ResourceBudget) -> Result<ObjectId, AdapterError> {
        Ok(identity(profile_value(budget)?))
    }
}
fn execution_error(error: AdapterError) -> ExecutionError {
    match error {
        AdapterError::Limit(_) => ExecutionError::ResourceExhausted,
        AdapterError::BackendUnavailable
        | AdapterError::Storage
        | AdapterError::Io(_)
        | AdapterError::Timeout => ExecutionError::BackendUnavailable,
        _ => ExecutionError::InvalidRequest,
    }
}
impl WorkerBackend for SqlWorker {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Sql
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.binding.validate(request, ResourceClass::Sql)?;
        snapshot_bytes(&self.store, self.binding.security_scope, request.input_root)
            .map_err(execution_error)?;
        query_text(&self.store, request.continuation).map_err(execution_error)?;
        Ok(())
    }
    fn permits_pure_reuse(&self, request: &StepRequest) -> bool {
        self.validate_request(request).is_ok()
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.validate_request(&request)?;
        let sql = query_text(&self.store, request.continuation).map_err(execution_error)?;
        let result = read(
            &self.store,
            self.binding.security_scope,
            request.input_root,
            &sql,
            request.budget,
        )
        .map_err(execution_error)?;
        let object = Object {
            payload: result_value(&result),
            refs: BTreeSet::from([request.input_root, request.continuation]),
        };
        if object
            .encode()
            .map_err(|_| ExecutionError::ResourceExhausted)?
            .len() as u64
            > request.budget.output_bytes
        {
            return Err(ExecutionError::ResourceExhausted);
        }
        let result = self
            .store
            .put_object(
                &format!("sql:result:{}", request.attempt),
                object.payload,
                object.refs,
            )
            .map_err(execution_error)?;
        Ok(StepOutcome {
            attempt: request.attempt,
            profile: request.profile,
            exit: StepExit::Completed {
                result,
                delta: self.delta,
                observation_root: request.observation_root,
            },
        })
    }
}
