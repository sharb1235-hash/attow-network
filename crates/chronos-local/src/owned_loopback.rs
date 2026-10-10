//! Owned same-host demonstration fence. Not a distributed deployment fencer.
//!
//! Keep this directory separate from authority snapshots and export bundles.
//! An OS file lock excludes other local processes; SQLite FULL commits anchor
//! the exact current head before the ledger returns success. Copying this
//! directory, network filesystems, cross-host writers and automatic handoff are
//! unsupported. This fixture does not authenticate remote deployment credentials
//! and must not enable a general remote authority service.
use chronos_codec::ObjectId;
use chronos_ledger::{AuthorityCheckpoint, FenceRequest, LedgerError, WriterFence, WriterLease};
use chronos_state::DomainId;
use rusqlite::{Connection, OpenFlags, params};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::Mutex,
};

const CHECKPOINT_BYTES: usize = 139;
pub struct OwnedLoopbackFence {
    path: PathBuf,
    writer: ObjectId,
}
struct OwnedLease {
    _lock: File,
    database: Mutex<Connection>,
    writer: ObjectId,
}
impl OwnedLoopbackFence {
    /// One-time enrollment of an unfederated owned demo authority. Existing
    /// external state is never inferred from a restored authority snapshot.
    pub fn enroll(
        path: &Path,
        checkpoint: &AuthorityCheckpoint,
        writer: ObjectId,
    ) -> Result<Self, LedgerError> {
        if checkpoint.writer.is_some() || checkpoint.next_writer.is_some() {
            return Err(LedgerError::Fenced);
        }
        std::fs::create_dir(path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("exclusive.lock"))?;
        lock.try_lock().map_err(|_| LedgerError::Fenced)?;
        lock.sync_all()?;
        let database = Connection::open(path.join("anchor.sqlite"))?;
        configure(&database)?;
        database.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE anchor (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                schema INTEGER NOT NULL CHECK(schema = 1),
                writer BLOB NOT NULL CHECK(length(writer) = 32),
                checkpoint BLOB NOT NULL CHECK(length(checkpoint) = 139)
             );",
        )?;
        database.execute(
            "INSERT INTO anchor VALUES (1,1,?1,?2)",
            params![writer.0.as_slice(), encode(checkpoint)],
        )?;
        database.execute_batch("COMMIT")?;
        drop(database);
        drop(lock);
        Self::open(path, writer)
    }
    /// Open existing external demo state, without creating files or directories.
    pub fn open(path: &Path, writer: ObjectId) -> Result<Self, LedgerError> {
        let path = path.canonicalize().map_err(|_| LedgerError::Fenced)?;
        for name in ["exclusive.lock", "anchor.sqlite"] {
            let metadata =
                std::fs::symlink_metadata(path.join(name)).map_err(|_| LedgerError::Fenced)?;
            if !metadata.file_type().is_file() {
                return Err(LedgerError::Fenced);
            }
        }
        let database = Connection::open_with_flags(
            path.join("anchor.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|_| LedgerError::Fenced)?;
        read(&database, writer)?;
        Ok(Self { path, writer })
    }
}
impl WriterFence for OwnedLoopbackFence {
    fn acquire(&self, request: &FenceRequest) -> Result<Box<dyn WriterLease>, LedgerError> {
        let cp = &request.checkpoint;
        if request.writer != self.writer
            || request.epoch != cp.epoch
            || cp.next_writer.is_some()
            || cp.writer.is_some_and(|w| w != self.writer)
        {
            return Err(LedgerError::Fenced);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path.join("exclusive.lock"))
            .map_err(|_| LedgerError::Fenced)?;
        lock.try_lock().map_err(|_| LedgerError::Fenced)?;
        let database = Connection::open_with_flags(
            self.path.join("anchor.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|_| LedgerError::Fenced)?;
        configure(&database)?;
        if read(&database, self.writer)? != *cp {
            return Err(LedgerError::Fenced);
        }
        Ok(Box::new(OwnedLease {
            _lock: lock,
            database: Mutex::new(database),
            writer: self.writer,
        }))
    }
}
impl WriterLease for OwnedLease {
    fn check(&self, checkpoint: &AuthorityCheckpoint) -> Result<(), LedgerError> {
        let database = self.database.lock().map_err(|_| LedgerError::Fenced)?;
        if read(&database, self.writer)? != *checkpoint {
            return Err(LedgerError::Fenced);
        }
        Ok(())
    }
    fn advance(
        &self,
        previous: &AuthorityCheckpoint,
        next: &AuthorityCheckpoint,
    ) -> Result<(), LedgerError> {
        if previous.domain != next.domain
            || previous.epoch != next.epoch
            || previous.next_writer.is_some()
            || next.next_writer.is_some()
            || previous.writer.is_some_and(|w| w != self.writer)
            || next.writer != Some(self.writer)
        {
            return Err(LedgerError::Fenced);
        }
        let mut database = self.database.lock().map_err(|_| LedgerError::Fenced)?;
        let transaction =
            database.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if read(&transaction, self.writer)? != *previous {
            return Err(LedgerError::Fenced);
        }
        let changed = transaction.execute(
            "UPDATE anchor SET checkpoint=?1 WHERE singleton=1 AND checkpoint=?2",
            params![encode(next), encode(previous)],
        )?;
        if changed != 1 {
            return Err(LedgerError::Fenced);
        }
        transaction.commit()?;
        Ok(())
    }
}
fn configure(database: &Connection) -> Result<(), LedgerError> {
    database.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;",
    )?;
    database.busy_timeout(std::time::Duration::from_secs(1))?;
    Ok(())
}
fn read(database: &Connection, writer: ObjectId) -> Result<AuthorityCheckpoint, LedgerError> {
    let bytes: Vec<u8> = database.query_row(
        "SELECT checkpoint FROM anchor WHERE singleton=1 AND schema=1 AND writer=?1 AND length(checkpoint)=139",
        [writer.0.as_slice()], |row| row.get(0),
    ).map_err(|_| LedgerError::Fenced)?;
    decode(&bytes)
}
fn encode(cp: &AuthorityCheckpoint) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CHECKPOINT_BYTES);
    bytes.push(1);
    bytes.extend_from_slice(&cp.domain.0);
    option(&mut bytes, cp.writer);
    bytes.extend_from_slice(&cp.epoch.to_be_bytes());
    bytes.extend_from_slice(&cp.head.0);
    option(&mut bytes, cp.next_writer);
    bytes
}
fn option(bytes: &mut Vec<u8>, value: Option<ObjectId>) {
    bytes.push(u8::from(value.is_some()));
    bytes.extend_from_slice(&value.unwrap_or(ObjectId([0; 32])).0);
}
fn decode(bytes: &[u8]) -> Result<AuthorityCheckpoint, LedgerError> {
    if bytes.len() != CHECKPOINT_BYTES || bytes[0] != 1 {
        return Err(LedgerError::Fenced);
    }
    let id = |range: std::ops::Range<usize>| -> Result<[u8; 32], LedgerError> {
        bytes[range].try_into().map_err(|_| LedgerError::Fenced)
    };
    let optional = |start: usize| -> Result<Option<ObjectId>, LedgerError> {
        let value = ObjectId(id(start + 1..start + 33)?);
        match bytes[start] {
            0 if value == ObjectId([0; 32]) => Ok(None),
            1 => Ok(Some(value)),
            _ => Err(LedgerError::Fenced),
        }
    };
    Ok(AuthorityCheckpoint {
        domain: DomainId(id(1..33)?),
        writer: optional(33)?,
        epoch: u64::from_be_bytes(bytes[66..74].try_into().map_err(|_| LedgerError::Fenced)?),
        head: ObjectId(id(74..106)?),
        next_writer: optional(106)?,
    })
}
