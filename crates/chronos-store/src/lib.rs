//! Transactional immutable objects, root pins, verified collection, and online backup.
//!
//! Use [`Store::transaction`] to commit objects, roots, and later authority rows
//! together. A separate call to [`Store::put`] commits its own transaction.

mod gc;
mod migrations;
mod sqlite;

use std::{path::Path, time::Duration};

use chronos_codec::{CodecError, Limits, ObjectId};
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub use gc::{GcLimits, GcStats};

/// Maximum number of distinct reachable objects in a default collection pass.
pub const DEFAULT_GC_OBJECT_LIMIT: usize = 100_000;
/// A contended write returns an error after SQLite's bounded busy wait.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// Persistence and validation failures. Object contents and owner names are not logged.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("operation aborted")]
    Aborted,
    #[error("missing object: {0:?}")]
    MissingObject(ObjectId),
    #[error("corrupt object: {0:?}")]
    CorruptObject(ObjectId),
    #[error("unsupported database version: {0}")]
    UnsupportedVersion(i64),
    #[error("database does not have the expected Attow Network application identity")]
    WrongDatabase,
    #[error("invalid pin owner: require 1 to 256 UTF-8 bytes without NUL")]
    InvalidOwner,
    #[error("object length {actual} exceeds the configured limit {limit}")]
    ObjectTooLarge { actual: usize, limit: usize },
    #[error("garbage collection exceeds the configured object limit {0}")]
    CollectionLimit(usize),
    #[error("garbage collection exceeds its {0} budget")]
    CollectionBudget(&'static str),
    #[error("online backup did not complete within its time budget")]
    BackupTimeout,
    #[error("online backup returned an unsupported state")]
    BackupIncomplete,
    #[error("backup failed ({backup}); incomplete destination cleanup also failed ({cleanup})")]
    BackupCleanup {
        backup: Box<StoreError>,
        cleanup: std::io::Error,
    },
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Verified persisted object metadata and bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredObject {
    pub kind: u16,
    pub schema: u16,
    pub bytes: Vec<u8>,
}

/// A single SQLite connection. The store does not dispatch effects or activate authority.
pub struct Store {
    connection: Connection,
    limits: Limits,
}

/// One immediate SQLite transaction, including immutable objects and root pins.
pub struct StoreTransaction<'connection> {
    transaction: Transaction<'connection>,
    limits: Limits,
}

/// A consistent deferred snapshot. Its SQL accessor is for trusted local readers.
/// The transaction is always rolled back, including after a successful callback.
pub struct StoreReadTransaction<'connection> {
    transaction: Transaction<'connection>,
    limits: Limits,
}

impl Store {
    /// Inspect an existing store without creation, migration, or write capability.
    /// An empty, unfamiliar, or unsupported database is rejected unchanged.
    pub fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "query_only", true)?;
        let version = migrations::check_identity(&connection)?;
        if version != 1 {
            return Err(StoreError::UnsupportedVersion(version));
        }
        connection.prepare("SELECT id, kind, schema, bytes FROM objects LIMIT 0")?;
        connection.prepare("SELECT owner, id FROM root_pins LIMIT 0")?;
        Ok(Self {
            connection,
            limits: Limits::default(),
        })
    }

    /// Open or initialize a store using the codec's default object limits.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::open_with_limits(path, Limits::default())
    }

    /// Open with explicit decode/size limits. This does not change stored data.
    pub fn open_with_limits(path: &Path, limits: Limits) -> Result<Self, StoreError> {
        limits.validate()?;
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        // Reject unknown application/schema identities before changing journal settings.
        migrations::check_identity(&connection)?;
        let journal: String =
            connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if journal != "wal" {
            return Err(StoreError::WrongDatabase);
        }
        connection.pragma_update(None, "synchronous", "FULL")?;
        migrations::migrate(&mut connection)?;
        Ok(Self { connection, limits })
    }

    /// Store immutable bytes. Known schemas are decoded and their direct children verified.
    pub fn put(&mut self, kind: u16, schema: u16, bytes: &[u8]) -> Result<ObjectId, StoreError> {
        self.transaction(|tx| tx.put(kind, schema, bytes))
    }

    /// Import bytes only if their independently expected identity matches.
    pub fn import(
        &mut self,
        id: ObjectId,
        kind: u16,
        schema: u16,
        bytes: &[u8],
    ) -> Result<(), StoreError> {
        self.transaction(|tx| tx.import(id, kind, schema, bytes))
    }

    /// Read bytes after checking size, kind, schema, and the complete object hash.
    pub fn get(&self, id: ObjectId) -> Result<Vec<u8>, StoreError> {
        Ok(self.get_object(id)?.bytes)
    }

    /// Read verified bytes and their domain-separated metadata.
    pub fn get_object(&self, id: ObjectId) -> Result<StoredObject, StoreError> {
        sqlite::get(&self.connection, id, &self.limits)
    }

    /// Existence check that does not conceal a corrupt object as absent.
    pub fn contains(&self, id: ObjectId) -> Result<bool, StoreError> {
        match self.get_object(id) {
            Ok(_) => Ok(true),
            Err(StoreError::MissingObject(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Retain an object graph for an owner. Repeated identical pins are idempotent.
    pub fn pin(&mut self, owner: &str, id: ObjectId) -> Result<(), StoreError> {
        self.transaction(|tx| tx.pin(owner, id))
    }

    /// Remove one owner's pin, returning whether it existed.
    pub fn unpin(&mut self, owner: &str, id: ObjectId) -> Result<bool, StoreError> {
        self.transaction(|tx| tx.unpin(owner, id))
    }

    /// Atomically collect objects unreachable from a frozen, verified pin set.
    pub fn collect_garbage(&mut self) -> Result<GcStats, StoreError> {
        self.collect_garbage_with_limit(DEFAULT_GC_OBJECT_LIMIT)
    }

    /// Collect with an explicit cap on distinct reachable objects.
    pub fn collect_garbage_with_limit(
        &mut self,
        max_objects: usize,
    ) -> Result<GcStats, StoreError> {
        self.collect_garbage_with_limits(GcLimits {
            max_objects,
            ..GcLimits::default()
        })
    }

    /// Collect with explicit object count, traversal depth, and payload byte budgets.
    pub fn collect_garbage_with_limits(&mut self, limits: GcLimits) -> Result<GcStats, StoreError> {
        self.transaction(|tx| tx.collect_garbage_with_limits(limits))
    }

    /// Produce a consistent online backup at a new path; never overwrite a destination.
    pub fn backup_to(&self, destination: &Path) -> Result<(), StoreError> {
        sqlite::backup(&self.connection, destination)
    }

    /// Commit only after the callback succeeds. Errors and unwinding roll back all changes.
    ///
    /// All concurrent store writers, including pinning and GC, use `BEGIN IMMEDIATE`.
    pub fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut StoreTransaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.transaction_with(operation)
    }

    /// Run a shared transaction while preserving a caller's typed business errors.
    pub fn transaction_with<T, E: From<StoreError>>(
        &mut self,
        operation: impl FnOnce(&mut StoreTransaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)?;
        let mut context = StoreTransaction {
            transaction,
            limits: self.limits,
        };
        let result = operation(&mut context)?;
        context.transaction.commit().map_err(StoreError::from)?;
        Ok(result)
    }

    /// Read related objects and application rows from one SQLite snapshot.
    ///
    /// The first read fixes the snapshot until this callback ends. Concurrent WAL
    /// writers may commit without changing subsequent reads in this transaction.
    /// Nested transactions are rejected. Errors and unwinding roll back; even a
    /// successful callback rolls back so an accidental SQL mutation cannot persist.
    pub fn read_transaction_with<T, E: From<StoreError>>(
        &self,
        operation: impl FnOnce(&StoreReadTransaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
        let context = StoreReadTransaction {
            transaction,
            limits: self.limits,
        };
        let result = operation(&context)?;
        context.transaction.rollback().map_err(StoreError::from)?;
        Ok(result)
    }
}

impl<'connection> StoreReadTransaction<'connection> {
    /// Trusted local read-only SQL. Do not issue transaction control, change
    /// pragmas, mutate data, or dispatch effects through this composition port.
    pub fn sql(&self) -> &Transaction<'connection> {
        &self.transaction
    }

    pub fn get(&self, id: ObjectId) -> Result<Vec<u8>, StoreError> {
        Ok(self.get_object(id)?.bytes)
    }

    pub fn get_object(&self, id: ObjectId) -> Result<StoredObject, StoreError> {
        sqlite::get(&self.transaction, id, &self.limits)
    }
}

impl<'connection> StoreTransaction<'connection> {
    /// Trusted local modules can write their tables through this same transaction.
    ///
    /// Callers must not issue transaction-control SQL, alter store tables, change
    /// pragmas, or dispatch external effects. This is an integration boundary,
    /// not an interface for untrusted SQL or remote callers.
    pub fn sql(&self) -> &Transaction<'connection> {
        &self.transaction
    }

    pub fn put(&self, kind: u16, schema: u16, bytes: &[u8]) -> Result<ObjectId, StoreError> {
        sqlite::put(&self.transaction, kind, schema, bytes, &self.limits)
    }

    pub fn import(
        &self,
        id: ObjectId,
        kind: u16,
        schema: u16,
        bytes: &[u8],
    ) -> Result<(), StoreError> {
        sqlite::check_size(bytes.len(), &self.limits)?;
        if chronos_codec::object_id(kind, schema, bytes) != id {
            return Err(StoreError::CorruptObject(id));
        }
        self.put(kind, schema, bytes)?;
        Ok(())
    }

    pub fn get(&self, id: ObjectId) -> Result<Vec<u8>, StoreError> {
        Ok(self.get_object(id)?.bytes)
    }

    pub fn get_object(&self, id: ObjectId) -> Result<StoredObject, StoreError> {
        sqlite::get(&self.transaction, id, &self.limits)
    }

    pub fn pin(&self, owner: &str, id: ObjectId) -> Result<(), StoreError> {
        sqlite::owner(owner)?;
        self.get_object(id)?;
        self.transaction.execute(
            "INSERT INTO root_pins (owner, id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            rusqlite::params![owner, &id.0[..]],
        )?;
        Ok(())
    }

    pub fn unpin(&self, owner: &str, id: ObjectId) -> Result<bool, StoreError> {
        sqlite::owner(owner)?;
        Ok(self.transaction.execute(
            "DELETE FROM root_pins WHERE owner=?1 AND id=?2",
            rusqlite::params![owner, &id.0[..]],
        )? != 0)
    }

    pub fn collect_garbage(&self) -> Result<GcStats, StoreError> {
        self.collect_garbage_with_limit(DEFAULT_GC_OBJECT_LIMIT)
    }

    pub fn collect_garbage_with_limit(&self, max_objects: usize) -> Result<GcStats, StoreError> {
        self.collect_garbage_with_limits(GcLimits {
            max_objects,
            ..GcLimits::default()
        })
    }

    pub fn collect_garbage_with_limits(&self, limits: GcLimits) -> Result<GcStats, StoreError> {
        gc::collect(&self.transaction, &self.limits, limits)
    }
}
