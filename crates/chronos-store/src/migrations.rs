use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;

const APPLICATION_ID: i64 = 0x4154_5457;
const SCHEMA_VERSION: i64 = 1;

pub(crate) fn check_identity(connection: &Connection) -> Result<i64, StoreError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::UnsupportedVersion(version));
    }
    let application: i64 = connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    if version == 0 {
        let tables: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if application != 0 || tables != 0 {
            return Err(StoreError::WrongDatabase);
        }
    } else if application != APPLICATION_ID {
        return Err(StoreError::WrongDatabase);
    }
    Ok(version)
}

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-read after taking the write lock: another opener may have migrated first.
    let version = check_identity(&transaction)?;
    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE objects (
                id BLOB PRIMARY KEY NOT NULL CHECK(length(id)=32),
                kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 65535),
                schema INTEGER NOT NULL CHECK(schema BETWEEN 0 AND 65535),
                bytes BLOB NOT NULL
             ) STRICT, WITHOUT ROWID;
             CREATE TABLE root_pins (
                owner TEXT NOT NULL CHECK(length(CAST(owner AS BLOB)) BETWEEN 1 AND 256),
                id BLOB NOT NULL REFERENCES objects(id) ON DELETE RESTRICT,
                PRIMARY KEY(owner, id)
             ) STRICT, WITHOUT ROWID;
             CREATE INDEX root_pins_by_id ON root_pins(id);",
        )?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    // Fail on missing or incompatible expected columns rather than creating partial tables.
    transaction.prepare("SELECT id, kind, schema, bytes FROM objects LIMIT 0")?;
    transaction.prepare("SELECT owner, id FROM root_pins LIMIT 0")?;
    transaction.commit()?;
    Ok(())
}
