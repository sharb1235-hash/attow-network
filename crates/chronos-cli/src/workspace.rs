use crate::{CliError, RunRecord};
use chronos_codec::ObjectId;
use chronos_ledger::Authority;
use chronos_local::owned_loopback::OwnedLoopbackFence;
use chronos_store::Store;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
const APP_ID: i64 = 0x4154434c;
#[derive(Clone, Debug)]
pub struct Workspace {
    root: PathBuf,
}
pub fn validate_id(id: &str) -> Result<ObjectId, CliError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(CliError::input(
            "expected a lowercase 64-digit object identifier",
        ));
    }
    id.parse()
        .map_err(|_| CliError::input("invalid object identifier"))
}
impl Workspace {
    pub fn init(path: &Path) -> Result<Self, CliError> {
        std::fs::create_dir_all(path)?;
        let workspace = Self {
            root: path.canonicalize()?,
        };
        std::fs::create_dir(workspace.data_dir())
            .map_err(|_| CliError::input("workspace already exists or cannot be initialized"))?;
        std::fs::create_dir(workspace.data_dir().join("bundles"))?;
        let mut random = [0; 32];
        getrandom::fill(&mut random).map_err(CliError::infrastructure)?;
        let writer = ObjectId(random);
        let mut authority = Authority::open(&workspace.authority_path())?;
        let fence =
            OwnedLoopbackFence::enroll(&workspace.fence_path(), &authority.checkpoint()?, writer)?;
        authority.initialize_federation(writer, &fence)?;
        drop(authority);
        let catalog = Connection::open(workspace.catalog_path())?;
        catalog.pragma_update(None, "journal_mode", "WAL")?;
        catalog.pragma_update(None, "synchronous", "FULL")?;
        catalog.execute_batch("CREATE TABLE metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1), writer TEXT NOT NULL); CREATE TABLE runs(id TEXT PRIMARY KEY, native INTEGER NOT NULL CHECK(native IN(0,1)), record BLOB NOT NULL, ready INTEGER NOT NULL DEFAULT 1 CHECK(ready IN(0,1)));")?;
        catalog.execute("INSERT INTO metadata VALUES(1,?1)", [writer.to_string()])?;
        catalog.pragma_update(None, "application_id", APP_ID)?;
        catalog.pragma_update(None, "user_version", 1)?;
        Ok(workspace)
    }
    pub fn open(path: &Path) -> Result<Self, CliError> {
        let root = path
            .canonicalize()
            .map_err(|_| CliError::input("workspace directory is unavailable"))?;
        let workspace = Self { root };
        if !workspace.data_dir().is_dir()
            || workspace.data_dir().canonicalize()? != workspace.data_dir()
        {
            return Err(CliError::input("directory is not an initialized workspace"));
        }
        workspace.catalog()?;
        Ok(workspace)
    }
    pub fn discover(path: &Path) -> Result<Self, CliError> {
        let start = path.canonicalize()?;
        for parent in start.ancestors() {
            if parent.join(".attow").exists() {
                return Self::open(parent);
            }
        }
        Err(CliError::input(
            "no initialized workspace found; run attow init PATH",
        ))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn data_dir(&self) -> PathBuf {
        self.root.join(".attow")
    }
    pub fn authority_path(&self) -> PathBuf {
        self.data_dir().join("authority.sqlite")
    }
    pub fn provider_path(&self) -> PathBuf {
        self.data_dir().join("provider.sqlite")
    }
    pub fn fence_path(&self) -> PathBuf {
        self.data_dir().join("writer-fence")
    }
    fn catalog_path(&self) -> PathBuf {
        self.data_dir().join("catalog.sqlite")
    }
    fn catalog(&self) -> Result<Connection, CliError> {
        let connection = Connection::open_with_flags(
            self.catalog_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|_| CliError::input("workspace catalog is unavailable"))?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        if connection.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))? != APP_ID
            || connection.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? != 1
        {
            return Err(CliError::input("unsupported workspace catalog"));
        }
        Ok(connection)
    }
    pub fn writer_id(&self) -> Result<ObjectId, CliError> {
        let writer: String = self.catalog()?.query_row(
            "SELECT writer FROM metadata WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        validate_id(&writer)
    }
    pub fn store(&self) -> Result<Store, CliError> {
        Ok(Store::open(&self.authority_path())?)
    }
    pub fn save_run(&self, run: &RunRecord) -> Result<(), CliError> {
        run.validate()?;
        let mut connection = self.catalog()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<(bool, Vec<u8>)> = transaction
            .query_row(
                "SELECT native,record FROM runs WHERE id=?1 AND length(record)<=?2",
                params![&run.run_id, MAX_RECORD_BYTES as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((native, bytes)) = existing {
            if !native {
                return Err(CliError::input(
                    "imported data cannot be resumed or promoted",
                ));
            }
            let old = decode_record(&bytes)?;
            for (old, new) in [
                (&old.initial_root, &run.initial_root),
                (&old.policy, &run.policy),
                (&old.decision, &run.decision),
                (&old.request_id, &run.request_id),
                (&old.payload, &run.payload),
                (&old.operation, &run.operation),
                (&old.intent, &run.intent),
                (&old.response_descriptor, &run.response_descriptor),
                (&old.response_request_id, &run.response_request_id),
                (&old.response_payload, &run.response_payload),
                (&old.response_decision, &run.response_decision),
            ] {
                if old.is_some() && old != new {
                    return Err(CliError::input("durable run identity cannot be replaced"));
                }
            }
            if old.scenario != run.scenario
                || old.generation != run.generation
                || (old.response_generation.is_some()
                    && old.response_generation != run.response_generation)
                || (old.selected_branch.is_some() && old.selected_branch != run.selected_branch)
                || old
                    .fidelity
                    .iter()
                    .any(|(key, value)| run.fidelity.get(key) != Some(value))
                || (!old.branches.is_empty() && old.branches != run.branches)
                || !run.events.starts_with(&old.events)
                || !run.trace_roots.starts_with(&old.trace_roots)
            {
                return Err(CliError::input("durable run history cannot be rewritten"));
            }
        } else {
            let collision: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1)",
                [&run.run_id],
                |r| r.get(0),
            )?;
            if collision {
                return Err(CliError::input("existing run record is invalid"));
            }
            let count: i64 =
                transaction.query_row("SELECT count(*) FROM runs", [], |r| r.get(0))?;
            if count >= 256 {
                return Err(CliError::input("workspace run limit reached"));
            }
        }
        transaction.execute(
            "INSERT INTO runs(id,native,record,ready) VALUES(?1,1,?2,1) ON CONFLICT(id) DO UPDATE SET record=excluded.record",
            params![run.run_id, serde_json::to_vec(run)?],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn load_run(&self, id: &str) -> Result<RunRecord, CliError> {
        validate_id(id)?;
        let bytes: Option<Vec<u8>> = self
            .catalog()?
            .query_row(
                "SELECT record FROM runs WHERE id=?1 AND ready=1 AND length(record)<=?2",
                params![id, MAX_RECORD_BYTES as i64],
                |r| r.get(0),
            )
            .optional()?;
        decode_record(&bytes.ok_or_else(|| CliError::input("run is absent or exceeds limits"))?)
    }
    pub fn is_native(&self, id: &str) -> Result<bool, CliError> {
        validate_id(id)?;
        self.catalog()?
            .query_row(
                "SELECT native FROM runs WHERE id=?1 AND ready=1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| CliError::input("run is absent"))
    }
    pub fn list_runs(&self) -> Result<Vec<RunRecord>, CliError> {
        let connection = self.catalog()?;
        let mut query = connection.prepare(
            "SELECT record FROM runs WHERE ready=1 AND length(record)<=?1 ORDER BY id LIMIT 257",
        )?;
        let mut runs = Vec::new();
        for row in query.query_map([MAX_RECORD_BYTES as i64], |r| r.get::<_, Vec<u8>>(0))? {
            if runs.len() >= 256 {
                return Err(CliError::input("workspace run limit exceeded"));
            }
            runs.push(decode_record(&row?)?);
        }
        Ok(runs)
    }
    pub(crate) fn reserve_import(&self, run: &RunRecord) -> Result<(), CliError> {
        run.validate()?;
        let mut connection = self.catalog()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous: Option<(bool, Vec<u8>)> = tx
            .query_row(
                "SELECT native,record FROM runs WHERE id=?1 AND length(record)<=?2",
                params![&run.run_id, MAX_RECORD_BYTES as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((native, bytes)) = previous {
            if native || decode_record(&bytes)? != *run {
                return Err(CliError::input("import cannot replace an existing run"));
            }
            return Ok(());
        }
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1)",
            [&run.run_id],
            |r| r.get(0),
        )?;
        if collision {
            return Err(CliError::input("existing run record is invalid"));
        }
        let count: i64 = tx.query_row("SELECT count(*) FROM runs", [], |r| r.get(0))?;
        if count >= 256 {
            return Err(CliError::input("workspace run limit reached"));
        }
        tx.execute(
            "INSERT INTO runs(id,native,record,ready) VALUES(?1,0,?2,0)",
            params![run.run_id, serde_json::to_vec(run)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Publish only after CAS and pins have committed. Pending reservations count
    /// against capacity and accept retries only for identical immutable data.
    pub(crate) fn complete_import(&self, run: &RunRecord) -> Result<(), CliError> {
        let connection = self.catalog()?;
        let changed = connection.execute(
            "UPDATE runs SET ready=1 WHERE id=?1 AND native=0 AND record=?2",
            params![run.run_id, serde_json::to_vec(run)?],
        )?;
        if changed != 1 {
            return Err(CliError::input("import reservation is absent or changed"));
        }
        Ok(())
    }
}
pub(crate) fn decode_record(bytes: &[u8]) -> Result<RunRecord, CliError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(CliError::input("run record exceeds byte limit"));
    }
    let record: RunRecord =
        serde_json::from_slice(bytes).map_err(|_| CliError::input("invalid run record"))?;
    record.validate()?;
    if serde_json::to_vec(&record)? != bytes {
        return Err(CliError::input("run record JSON is not canonical"));
    }
    Ok(record)
}
