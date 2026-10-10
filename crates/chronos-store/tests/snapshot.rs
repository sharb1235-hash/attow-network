use attow_store::{Store, StoreError};

#[test]
fn read_only_open_never_creates_migrates_or_writes() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("snapshot.sqlite");
    assert!(Store::open_read_only(&path).is_err());
    assert!(!path.exists());
    let mut writer = Store::open(&path).expect("writer");
    let id = writer.put(99, 1, b"original").expect("object");
    let mut reader = Store::open_read_only(&path).expect("reader");
    assert_eq!(reader.get(id).expect("read"), b"original");
    assert!(reader.put(99, 1, b"forbidden").is_err());
    assert!(
        reader
            .transaction(|tx| {
                tx.sql().execute_batch("CREATE TABLE forbidden(x)")?;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(writer.get(id).expect("unchanged"), b"original");
    let empty = directory.path().join("empty.sqlite");
    std::fs::write(&empty, []).expect("empty file");
    assert!(Store::open_read_only(&empty).is_err());
    assert_eq!(std::fs::metadata(empty).expect("metadata").len(), 0);
}

#[test]
fn snapshot_is_consistent_across_a_concurrent_writers_commit() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("snapshot.sqlite");
    let mut writer = Store::open(&path).expect("writer");
    writer
        .transaction(|tx| {
            tx.sql().execute_batch(
                "CREATE TABLE snapshot_test(value INTEGER); INSERT INTO snapshot_test VALUES (1)",
            )?;
            Ok(())
        })
        .expect("schema");
    let reader = Store::open(&path).expect("reader");
    reader
        .read_transaction_with::<_, StoreError>(|snapshot| {
            let before: i64 =
                snapshot
                    .sql()
                    .query_row("SELECT value FROM snapshot_test", [], |row| row.get(0))?;
            writer.transaction(|tx| {
                tx.sql().execute("UPDATE snapshot_test SET value=2", [])?;
                Ok(())
            })?;
            let after: i64 =
                snapshot
                    .sql()
                    .query_row("SELECT value FROM snapshot_test", [], |row| row.get(0))?;
            assert_eq!((before, after), (1, 1));
            Ok(())
        })
        .expect("consistent snapshot");
    reader
        .read_transaction_with::<_, StoreError>(|snapshot| {
            let after: i64 =
                snapshot
                    .sql()
                    .query_row("SELECT value FROM snapshot_test", [], |row| row.get(0))?;
            assert_eq!(after, 2);
            Ok(())
        })
        .expect("fresh snapshot");
}

#[test]
fn snapshot_cannot_commit_an_accidental_sql_mutation() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("snapshot.sqlite");
    let mut store = Store::open(&path).expect("store");
    store
        .transaction(|tx| {
            tx.sql().execute_batch(
                "CREATE TABLE snapshot_test(value INTEGER); INSERT INTO snapshot_test VALUES (1)",
            )?;
            Ok(())
        })
        .expect("schema");
    store
        .read_transaction_with::<_, StoreError>(|snapshot| {
            snapshot
                .sql()
                .execute("UPDATE snapshot_test SET value=99", [])?;
            Ok(())
        })
        .expect("snapshot rollback");
    store
        .read_transaction_with::<_, StoreError>(|snapshot| {
            let value: i64 =
                snapshot
                    .sql()
                    .query_row("SELECT value FROM snapshot_test", [], |row| row.get(0))?;
            assert_eq!(value, 1);
            Ok(())
        })
        .expect("still original");
}
