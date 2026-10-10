use std::{
    fs::OpenOptions,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use chronos_codec::{Limits, OBJECT_KIND, OBJECT_SCHEMA, ObjectId, object_id, references};
use rusqlite::{
    Connection, OptionalExtension,
    backup::{Backup, StepResult},
    params,
};

use crate::{StoreError, StoredObject};

pub(crate) fn owner(value: &str) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(StoreError::InvalidOwner);
    }
    Ok(())
}

pub(crate) fn check_size(actual: usize, limits: &Limits) -> Result<(), StoreError> {
    if actual > limits.object_bytes {
        return Err(StoreError::ObjectTooLarge {
            actual,
            limit: limits.object_bytes,
        });
    }
    Ok(())
}

pub(crate) fn get(
    connection: &Connection,
    id: ObjectId,
    limits: &Limits,
) -> Result<StoredObject, StoreError> {
    let max_size = i64::try_from(limits.object_bytes).unwrap_or(i64::MAX);
    // CASE prevents SQLite from handing a too-large blob to Vec allocation.
    let record = connection.query_row(
        "SELECT kind, schema, length(bytes), CASE WHEN length(bytes)<=?2 THEN bytes ELSE NULL END
         FROM objects WHERE id=?1",
        params![&id.0[..], max_size],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<Vec<u8>>>(3)?)),
    ).optional()?.ok_or(StoreError::MissingObject(id))?;
    let size = usize::try_from(record.2).map_err(|_| StoreError::CorruptObject(id))?;
    check_size(size, limits)?;
    let kind = u16::try_from(record.0).map_err(|_| StoreError::CorruptObject(id))?;
    let schema = u16::try_from(record.1).map_err(|_| StoreError::CorruptObject(id))?;
    let bytes = record.3.ok_or(StoreError::CorruptObject(id))?;
    if object_id(kind, schema, &bytes) != id {
        return Err(StoreError::CorruptObject(id));
    }
    if kind == OBJECT_KIND && schema == OBJECT_SCHEMA {
        references(kind, schema, &bytes, limits)?;
    }
    Ok(StoredObject {
        kind,
        schema,
        bytes,
    })
}

pub(crate) fn put(
    connection: &Connection,
    kind: u16,
    schema: u16,
    bytes: &[u8],
    limits: &Limits,
) -> Result<ObjectId, StoreError> {
    check_size(bytes.len(), limits)?;
    if kind == OBJECT_KIND && schema == OBJECT_SCHEMA {
        for child in references(kind, schema, bytes, limits)? {
            get(connection, child, limits)?;
        }
    }
    let id = object_id(kind, schema, bytes);
    let inserted = connection.execute(
        "INSERT INTO objects (id, kind, schema, bytes) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO NOTHING",
        params![&id.0[..], i64::from(kind), i64::from(schema), bytes],
    )?;
    if inserted == 0 {
        let existing = get(connection, id, limits)?;
        if existing.kind != kind || existing.schema != schema || existing.bytes != bytes {
            return Err(StoreError::CorruptObject(id));
        }
    }
    Ok(id)
}

pub(crate) fn backup(source: &Connection, path: &Path) -> Result<(), StoreError> {
    // Exclusively reserve a new destination. SQLite's backup API otherwise overwrites it.
    let reservation = OpenOptions::new().write(true).create_new(true).open(path)?;
    drop(reservation);
    let result = (|| {
        let mut destination =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        destination.busy_timeout(Duration::from_millis(100))?;
        destination.pragma_update(None, "synchronous", "FULL")?;
        {
            let backup = Backup::new(source, &mut destination)?;
            let started = Instant::now();
            loop {
                if started.elapsed() >= Duration::from_secs(30) {
                    return Err(StoreError::BackupTimeout);
                }
                match backup.step(128)? {
                    StepResult::Done => break,
                    StepResult::More => thread::yield_now(),
                    StepResult::Busy | StepResult::Locked => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    _ => return Err(StoreError::BackupIncomplete),
                }
            }
        }
        destination
            .close()
            .map_err(|(_, error)| StoreError::Database(error))?;
        Ok(())
    })();
    if let Err(backup) = result {
        return match std::fs::remove_file(path) {
            Ok(()) => Err(backup),
            Err(cleanup) => Err(StoreError::BackupCleanup {
                backup: Box::new(backup),
                cleanup,
            }),
        };
    }
    Ok(())
}
