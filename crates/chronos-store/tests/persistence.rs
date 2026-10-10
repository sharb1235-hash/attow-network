use std::{error::Error, sync::mpsc, thread, time::Duration};

use attow_store::{GcLimits, Store, StoreError};
use chronos_codec::ObjectId;
use rusqlite::{Connection, params};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug, thiserror::Error)]
enum BusinessError {
    #[error("business rule rejected the transaction")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

// The Stage 2 Object envelope: [0x4154544f, 1, sorted references, payload].
fn leaf(value: u8) -> Vec<u8> {
    vec![0x84, 0x1a, 0x41, 0x54, 0x54, 0x4f, 0x01, 0x80, 0x41, value]
}

fn parent(child: ObjectId) -> Vec<u8> {
    let mut bytes = vec![0x84, 0x1a, 0x41, 0x54, 0x54, 0x4f, 0x01, 0x81, 0x58, 0x20];
    bytes.extend_from_slice(&child.0);
    bytes.push(0xf6);
    bytes
}

#[test]
fn put_get_reopen_and_migration_are_idempotent() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    let bytes = leaf(7);
    let id = store.put(1, 1, &bytes)?;
    assert_eq!(store.put(1, 1, &bytes)?, id);
    store.pin("branch:a", id)?;
    drop(store);
    let mut reopened = Store::open(&path)?;
    assert_eq!(reopened.get(id)?, bytes);
    assert_eq!(reopened.collect_garbage()?.deleted, 0);
    reopened.transaction(|tx| {
        let version: i64 = tx
            .sql()
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let foreign_keys: i64 = tx
            .sql()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        let sync: i64 = tx
            .sql()
            .query_row("PRAGMA synchronous", [], |row| row.get(0))?;
        let journal: String = tx
            .sql()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!((version, foreign_keys, sync), (1, 1, 2));
        assert_eq!(journal, "wal");
        Ok(())
    })?;
    Ok(())
}

#[test]
fn corrupted_bytes_or_kind_fail_hash_verification_and_collision_reuse() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    let id = store.put(1, 1, &leaf(7))?;
    let raw = Connection::open(&path)?;
    raw.execute(
        "UPDATE objects SET bytes=?1 WHERE id=?2",
        params![leaf(8), &id.0[..]],
    )?;
    assert!(matches!(store.get(id), Err(StoreError::CorruptObject(_))));
    assert!(matches!(
        store.put(1, 1, &leaf(7)),
        Err(StoreError::CorruptObject(_))
    ));
    raw.execute(
        "UPDATE objects SET bytes=?1, kind=2 WHERE id=?2",
        params![leaf(7), &id.0[..]],
    )?;
    assert!(matches!(store.get(id), Err(StoreError::CorruptObject(_))));
    Ok(())
}

#[test]
fn transaction_failure_before_and_after_insertion_rolls_everything_back() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let before: Result<(), StoreError> = store.transaction(|_| Err(StoreError::Aborted));
    assert!(matches!(before, Err(StoreError::Aborted)));
    let mut inserted = None;
    let after: Result<(), StoreError> = store.transaction(|tx| {
        let id = tx.put(1, 1, &leaf(9))?;
        inserted = Some(id);
        tx.pin("branch:failed", id)?;
        tx.sql()
            .execute("CREATE TABLE ledger_fixture (value INTEGER)", [])?;
        tx.sql()
            .execute("INSERT INTO ledger_fixture VALUES (1)", [])?;
        Err(StoreError::Aborted)
    });
    assert!(matches!(after, Err(StoreError::Aborted)));
    let id = inserted.ok_or("test transaction did not insert its object")?;
    assert!(matches!(store.get(id), Err(StoreError::MissingObject(_))));
    store.transaction(|tx| {
        let pins: i64 = tx
            .sql()
            .query_row("SELECT count(*) FROM root_pins", [], |row| row.get(0))?;
        let tables: i64 = tx.sql().query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='ledger_fixture'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!((pins, tables), (0, 0));
        Ok(())
    })?;
    Ok(())
}

#[test]
fn typed_business_error_survives_shared_transaction_rollback() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let mut inserted = None;
    let result: Result<(), BusinessError> = store.transaction_with(|tx| {
        let id = tx.put(1, 1, &leaf(42))?;
        inserted = Some(id);
        tx.pin("business:rejected", id)?;
        Err(BusinessError::Conflict)
    });
    assert!(matches!(result, Err(BusinessError::Conflict)));
    assert!(!store.contains(inserted.ok_or("transaction was not invoked")?)?);
    store.transaction(|tx| {
        assert_eq!(
            tx.sql()
                .query_row("SELECT count(*) FROM root_pins", [], |row| row
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })?;
    Ok(())
}

#[test]
fn missing_reference_and_missing_pin_never_create_a_root() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let missing = ObjectId([3; 32]);
    assert!(matches!(
        store.put(1, 1, &parent(missing)),
        Err(StoreError::MissingObject(_))
    ));
    assert!(matches!(
        store.pin("branch:missing", missing),
        Err(StoreError::MissingObject(_))
    ));
    assert_eq!(store.collect_garbage()?.deleted, 0);
    Ok(())
}

#[test]
fn mark_sweep_keeps_transitive_children_until_last_pin_is_removed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let child = store.put(1, 1, &leaf(3))?;
    let root = store.put(1, 1, &parent(child))?;
    let garbage = store.put(1, 1, &leaf(4))?;
    store.pin("branch:a", root)?;
    store.pin("export:b", root)?;
    let stats = store.collect_garbage()?;
    assert_eq!((stats.retained, stats.deleted), (2, 1));
    assert!(matches!(
        store.get(garbage),
        Err(StoreError::MissingObject(_))
    ));
    assert_eq!(store.get(child)?, leaf(3));
    assert!(store.unpin("branch:a", root)?);
    assert_eq!(store.collect_garbage()?.deleted, 0);
    assert!(store.unpin("export:b", root)?);
    assert_eq!(store.collect_garbage()?.deleted, 2);
    Ok(())
}

#[test]
fn reachable_unknown_schema_aborts_collection_without_sweeping() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let opaque = store.put(1, 99, b"future object may contain references")?;
    store.pin("future:root", opaque)?;
    let garbage = store.put(1, 1, &leaf(5))?;
    assert!(store.collect_garbage().is_err());
    assert_eq!(store.get(garbage)?, leaf(5));
    assert_eq!(store.get(opaque)?, b"future object may contain references");
    Ok(())
}

#[test]
fn collection_waits_for_pin_transaction_and_preserves_its_root() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    let id = store.put(1, 1, &leaf(6))?;
    let mut collector = Store::open(&path)?;
    let (started_tx, started_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let mut worker = None;
    store.transaction(|tx| {
        tx.pin("branch:concurrent", id)?;
        worker = Some(thread::spawn(move || {
            let _ = started_tx.send(());
            let _ = result_tx.send(collector.collect_garbage());
        }));
        started_rx.recv().map_err(|_| StoreError::Aborted)?;
        assert!(matches!(
            result_rx.recv_timeout(Duration::from_millis(40)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        Ok(())
    })?;
    assert_eq!(result_rx.recv_timeout(Duration::from_secs(3))??.deleted, 0);
    worker
        .ok_or("worker missing")?
        .join()
        .map_err(|_| "worker panic")?;
    assert_eq!(store.get(id)?, leaf(6));
    Ok(())
}

#[test]
fn pin_after_collection_cannot_commit_a_dangling_root() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    let id = store.put(1, 1, &leaf(10))?;
    let mut pinner = Store::open(&path)?;
    let (started_tx, started_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let mut worker = None;
    store.transaction(|tx| {
        assert_eq!(tx.collect_garbage()?.deleted, 1);
        worker = Some(thread::spawn(move || {
            let _ = started_tx.send(());
            let _ = result_tx.send(pinner.pin("branch:late", id));
        }));
        started_rx.recv().map_err(|_| StoreError::Aborted)?;
        assert!(matches!(
            result_rx.recv_timeout(Duration::from_millis(40)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        Ok(())
    })?;
    assert!(matches!(
        result_rx.recv_timeout(Duration::from_secs(3))?,
        Err(StoreError::MissingObject(_))
    ));
    worker
        .ok_or("worker missing")?
        .join()
        .map_err(|_| "worker panic")?;
    Ok(())
}

#[test]
fn online_backup_contains_committed_wal_data_and_excludes_uncommitted_data() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let backup = dir.path().join("backup.db");
    let mut store = Store::open(&path)?;
    let id = store.put(1, 1, &leaf(11))?;
    store.pin("export:backup", id)?;
    let writer = Connection::open(&path)?;
    writer.execute_batch("BEGIN IMMEDIATE")?;
    writer.execute(
        "INSERT INTO objects (id, kind, schema, bytes) VALUES (?1, 1, 1, ?2)",
        params![&[99_u8; 32][..], leaf(12)],
    )?;
    store.backup_to(&backup)?;
    writer.execute_batch("ROLLBACK")?;
    let mut restored = Store::open(&backup)?;
    assert_eq!(restored.get(id)?, leaf(11));
    assert!(matches!(
        restored.get(ObjectId([99; 32])),
        Err(StoreError::MissingObject(_))
    ));
    assert_eq!(restored.collect_garbage()?.retained, 1);
    assert!(
        store.backup_to(&backup).is_err(),
        "backup must not overwrite an existing destination"
    );
    Ok(())
}

#[test]
fn future_database_version_is_rejected_without_downgrade() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("future.db");
    let raw = Connection::open(&path)?;
    raw.pragma_update(None, "user_version", 999)?;
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::UnsupportedVersion(999))
    ));
    let version: i64 = raw.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(version, 999);
    Ok(())
}

#[test]
fn known_noncanonical_objects_and_invalid_owners_are_rejected() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    assert!(store.put(1, 1, &[0x9f, 0xff]).is_err());
    let id = store.put(1, 1, &leaf(1))?;
    assert!(matches!(store.pin("", id), Err(StoreError::InvalidOwner)));
    assert!(matches!(
        store.pin(&"x".repeat(257), id),
        Err(StoreError::InvalidOwner)
    ));
    Ok(())
}

#[test]
fn imported_identity_and_read_size_limits_are_checked() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    assert!(matches!(
        store.import(ObjectId([33; 32]), 1, 1, &leaf(7)),
        Err(StoreError::CorruptObject(_))
    ));
    let bytes = leaf(7);
    let id = store.put(1, 1, &bytes)?;
    let limited = Store::open_with_limits(
        &path,
        chronos_codec::Limits {
            object_bytes: bytes.len() - 1,
            depth: 64,
            items: 100,
        },
    )?;
    assert!(matches!(
        limited.get(id),
        Err(StoreError::ObjectTooLarge { .. })
    ));
    let raw = Connection::open(&path)?;
    raw.execute(
        "UPDATE objects SET bytes=zeroblob(1048576) WHERE id=?1",
        [&id.0[..]],
    )?;
    assert!(matches!(
        limited.get(id),
        Err(StoreError::ObjectTooLarge {
            actual: 1048576,
            ..
        })
    ));
    Ok(())
}

#[test]
fn every_gc_budget_failure_preserves_unreachable_objects() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("store.db"))?;
    let child = store.put(1, 1, &leaf(20))?;
    let root = store.put(1, 1, &parent(child))?;
    let garbage = store.put(1, 1, &leaf(21))?;
    store.pin("branch:bounded", root)?;
    assert!(matches!(
        store.collect_garbage_with_limit(1),
        Err(StoreError::CollectionLimit(1))
    ));
    assert!(matches!(
        store.collect_garbage_with_limits(GcLimits {
            max_depth: 0,
            ..GcLimits::default()
        }),
        Err(StoreError::CollectionBudget("depth"))
    ));
    assert!(matches!(
        store.collect_garbage_with_limits(GcLimits {
            max_bytes: 1,
            ..GcLimits::default()
        }),
        Err(StoreError::CollectionBudget("bytes"))
    ));
    assert_eq!(store.get(garbage)?, leaf(21));
    assert_eq!(store.collect_garbage()?.deleted, 1);
    Ok(())
}

#[test]
fn dangling_or_corrupt_reachable_children_abort_before_sweep() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path)?;
    let child = store.put(1, 1, &leaf(30))?;
    let root = store.put(1, 1, &parent(child))?;
    let garbage = store.put(1, 1, &leaf(31))?;
    store.pin("branch:damaged", root)?;
    let raw = Connection::open(&path)?;
    raw.execute("DELETE FROM objects WHERE id=?1", [&child.0[..]])?;
    assert!(matches!(
        store.collect_garbage(),
        Err(StoreError::MissingObject(_))
    ));
    assert_eq!(store.get(garbage)?, leaf(31));
    Ok(())
}

#[test]
fn an_unrelated_database_is_not_adopted_or_overwritten() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("unrelated.db");
    let raw = Connection::open(&path)?;
    raw.execute("CREATE TABLE original (value INTEGER)", [])?;
    raw.execute("INSERT INTO original VALUES (42)", [])?;
    assert!(matches!(Store::open(&path), Err(StoreError::WrongDatabase)));
    assert_eq!(
        raw.query_row("SELECT value FROM original", [], |row| row.get::<_, i64>(0))?,
        42
    );
    Ok(())
}

#[test]
fn invalid_limits_fail_before_creating_a_database_and_opaque_bytes_obey_caps() -> TestResult {
    let dir = tempfile::tempdir()?;
    let invalid_path = dir.path().join("invalid.db");
    assert!(matches!(
        Store::open_with_limits(
            &invalid_path,
            chronos_codec::Limits {
                object_bytes: usize::MAX,
                ..chronos_codec::Limits::default()
            }
        ),
        Err(StoreError::Codec(chronos_codec::CodecError::InvalidLimits))
    ));
    assert!(!invalid_path.exists());
    let path = dir.path().join("bounded.db");
    let limits = chronos_codec::Limits {
        object_bytes: 5,
        ..chronos_codec::Limits::default()
    };
    let mut limited = Store::open_with_limits(&path, limits)?;
    assert!(matches!(
        limited.put(9, 9, b"sixsix"),
        Err(StoreError::ObjectTooLarge { .. })
    ));
    assert!(matches!(
        limited.import(ObjectId([0; 32]), 9, 9, b"sixsix"),
        Err(StoreError::ObjectTooLarge { .. })
    ));
    let mut normal = Store::open(&path)?;
    let id = normal.put(9, 9, b"sixsix")?;
    assert!(matches!(
        limited.get(id),
        Err(StoreError::ObjectTooLarge { .. })
    ));
    Ok(())
}
