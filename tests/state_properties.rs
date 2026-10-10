use std::collections::{BTreeMap, BTreeSet};

use attow_network::state::{Dependency, DomainId, Mutation, StateRoot, Version, fork};
use proptest::prelude::*;

#[path = "support/state_store.rs"]
mod state_store;

use state_store::{StoreAdapter, StoreView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DOMAIN: DomainId = DomainId([7; 32]);

fn initial() -> Result<StateRoot, attow_network::state::StateError> {
    StateRoot::new(DOMAIN).apply(&BTreeMap::from([
        (b"a0".to_vec(), Mutation::Put(vec![10])),
        (b"b1".to_vec(), Mutation::Put(vec![20])),
    ]))
}

fn key(number: u8) -> Vec<u8> {
    vec![if number.is_multiple_of(2) { b'a' } else { b'b' }, number]
}

fn has_key(dependencies: &[Dependency], key: &[u8], version: Version) -> bool {
    dependencies.iter().any(|dependency| matches!(dependency,
        Dependency::Key { key: observed, version: expected } if observed == key && *expected == version))
}

#[test]
fn sibling_changes_and_own_writes_do_not_mutate_shared_parent() -> TestResult {
    let root = initial()?;
    let original = root.object_id()?;
    let mut left = fork(DOMAIN, &root, [1; 32])?;
    let mut right = fork(DOMAIN, &root, [2; 32])?;
    left.put(b"a0".to_vec(), vec![99])?;
    left.delete(b"b1")?;
    right.put(b"new".to_vec(), vec![42])?;
    assert_eq!(left.get(b"a0")?, Some(vec![99]));
    assert_eq!(left.get(b"b1")?, None);
    assert_eq!(right.get(b"a0")?, Some(vec![10]));
    assert_eq!(right.get(b"b1")?, Some(vec![20]));
    assert_eq!(left.get(b"new")?, None);
    assert_eq!(root.get(b"a0")?, Some(vec![10]));
    assert_eq!(root.get(b"new")?, None);
    assert_eq!(root.object_id()?, original);
    assert_ne!(left.id(), right.id());
    Ok(())
}

#[test]
fn frozen_dependencies_include_absence_blind_writes_and_deletes() -> TestResult {
    let root = initial()?;
    let mut branch = fork(DOMAIN, &root, [3; 32])?;
    assert_eq!(branch.get(b"missing")?, None);
    branch.put(b"a0".to_vec(), vec![88])?;
    branch.put(b"new".to_vec(), vec![77])?;
    branch.delete(b"b1")?;
    branch.delete(b"never-seen")?;
    let frozen = branch.freeze()?;
    for name in [b"missing".as_slice(), b"a0", b"new", b"b1", b"never-seen"] {
        assert!(has_key(frozen.dependencies(), name, root.version(name)?));
    }
    assert!(has_key(frozen.dependencies(), b"missing", Version(0)));
    assert_eq!(
        frozen.overlay().get(b"b1".as_slice()),
        Some(&Mutation::Delete)
    );
    assert_eq!(frozen.base().object_id()?, root.object_id()?);
    Ok(())
}

#[test]
fn absent_present_deleted_aba_retains_changed_version() -> TestResult {
    let original = StateRoot::new(DOMAIN);
    let mut reader = fork(DOMAIN, &original, [4; 32])?;
    assert_eq!(reader.get(b"slot")?, None);
    let frozen = reader.freeze()?;
    let present = original.apply(&BTreeMap::from([(
        b"slot".to_vec(),
        Mutation::Put(vec![1]),
    )]))?;
    let deleted = present.apply(&BTreeMap::from([(b"slot".to_vec(), Mutation::Delete)]))?;
    assert_eq!(deleted.get(b"slot")?, None);
    assert!(has_key(frozen.dependencies(), b"slot", Version(0)));
    assert_ne!(original.version(b"slot")?, deleted.version(b"slot")?);
    assert!(deleted.version(b"slot")?.0 > present.version(b"slot")?.0);
    assert_ne!(original.object_id()?, deleted.object_id()?);
    Ok(())
}

#[test]
fn prefix_scans_merge_overlay_and_tombstones_and_track_namespace() -> TestResult {
    let root = StateRoot::new(DOMAIN).apply(&BTreeMap::from([
        (b"a/first".to_vec(), Mutation::Put(vec![1])),
        (b"a/last".to_vec(), Mutation::Put(vec![3])),
        (b"b/other".to_vec(), Mutation::Put(vec![9])),
    ]))?;
    let mut branch = fork(DOMAIN, &root, [5; 32])?;
    branch.delete(b"a/first")?;
    branch.put(b"a/middle".to_vec(), vec![2])?;
    branch.put(b"a/last".to_vec(), vec![4])?;
    assert_eq!(
        branch.scan_prefix(b"a/")?,
        vec![
            (b"a/last".to_vec(), vec![4]),
            (b"a/middle".to_vec(), vec![2])
        ]
    );
    assert_eq!(branch.scan_prefix(b"empty/")?, Vec::new());
    let frozen = branch.freeze()?;
    assert!(frozen.dependencies().contains(&Dependency::Namespace {
        epoch: root.namespace_epoch()
    }));
    let changed = root.apply(&BTreeMap::from([(
        b"a/phantom".to_vec(),
        Mutation::Put(vec![7]),
    )]))?;
    assert_ne!(root.namespace_epoch(), changed.namespace_epoch());
    Ok(())
}

#[test]
fn branch_identity_is_separate_from_state_and_enforces_domain() -> TestResult {
    let root = initial()?;
    let left = fork(DOMAIN, &root, [8; 32])?.freeze()?;
    let right = fork(DOMAIN, &root, [9; 32])?.freeze()?;
    assert_ne!(left.id(), right.id());
    assert_eq!(left.base().object_id()?, right.base().object_id()?);
    assert_eq!(left.overlay(), right.overlay());
    assert!(fork(DomainId([99; 32]), &root, [8; 32]).is_err());
    Ok(())
}

#[test]
fn insertion_order_does_not_change_durable_state_identity() -> TestResult {
    let values = [
        (b"c".to_vec(), vec![3]),
        (b"a".to_vec(), vec![1]),
        (b"b".to_vec(), vec![2]),
    ];
    let forward = values
        .iter()
        .map(|(key, value)| (key.clone(), Mutation::Put(value.clone())))
        .collect();
    let reverse = values
        .iter()
        .rev()
        .map(|(key, value)| (key.clone(), Mutation::Put(value.clone())))
        .collect();
    let a = StateRoot::new(DOMAIN).apply(&forward)?;
    let b = StateRoot::new(DOMAIN).apply(&reverse)?;
    assert_eq!(a.object_id()?, b.object_id()?);
    for (key, _) in values {
        assert_eq!(a.version(&key)?, b.version(&key)?);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 2048, ..ProptestConfig::default() })]

    #[test]
    fn generated_traces_match_btree_model(
        operations in prop::collection::vec((0_u8..4, 0_u8..16, prop::collection::vec(any::<u8>(), 0..16)), 0..96)
    ) {
        let root = initial().map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut branch = fork(DOMAIN, &root, [22; 32]).map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut model = BTreeMap::from([(b"a0".to_vec(), vec![10]), (b"b1".to_vec(), vec![20])]);
        let mut expected_keys = BTreeSet::new();
        let mut scanned = false;
        for (operation, number, value) in operations {
            let selected = key(number);
            match operation {
                0 => {
                    branch.put(selected.clone(), value.clone()).map_err(|error| TestCaseError::fail(error.to_string()))?;
                    model.insert(selected.clone(), value);
                    expected_keys.insert(selected);
                }
                1 => {
                    branch.delete(&selected).map_err(|error| TestCaseError::fail(error.to_string()))?;
                    model.remove(&selected);
                    expected_keys.insert(selected);
                }
                2 => {
                    prop_assert_eq!(branch.get(&selected).map_err(|error| TestCaseError::fail(error.to_string()))?, model.get(&selected).cloned());
                    expected_keys.insert(selected);
                }
                _ => {
                    let prefix = &selected[..1];
                    let expected = model.iter().filter(|(key, _)| key.starts_with(prefix)).map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>();
                    prop_assert_eq!(branch.scan_prefix(prefix).map_err(|error| TestCaseError::fail(error.to_string()))?, expected);
                    scanned = true;
                }
            }
        }
        let frozen = branch.freeze().map_err(|error| TestCaseError::fail(error.to_string()))?;
        for key in expected_keys {
            prop_assert!(has_key(frozen.dependencies(), &key, root.version(&key).map_err(|error| TestCaseError::fail(error.to_string()))?));
        }
        if scanned {
            prop_assert!(frozen.dependencies().contains(&Dependency::Namespace { epoch: root.namespace_epoch() }), "scan dependency is missing");
        }
        let mut actual = BTreeMap::from([(b"a0".to_vec(), vec![10]), (b"b1".to_vec(), vec![20])]);
        for (key, mutation) in frozen.overlay() {
            match mutation {
                Mutation::Put(value) => { actual.insert(key.clone(), value.clone()); }
                Mutation::Delete => { actual.remove(key); }
            }
        }
        prop_assert_eq!(actual, model);
        prop_assert_eq!(root.get(b"a0").map_err(|error| TestCaseError::fail(error.to_string()))?, Some(vec![10]));
    }
}

#[test]
fn original_sql_fixture_matches_the_state_model() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = attow_network::store::Store::open(&directory.path().join("fixture.db"))?;
    let values = store.transaction(|transaction| {
        transaction
            .sql()
            .execute_batch(include_str!("../fixtures/original/sql/branch-model.sql"))?;
        let mut statement = transaction
            .sql()
            .prepare("SELECT key, value FROM branch_model_expected ORDER BY key")?;
        let entries = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut mutations = BTreeMap::new();
        for entry in entries {
            let (key, value) = entry?;
            mutations.insert(key, Mutation::Put(value));
        }
        Ok(mutations)
    })?;
    let root = StateRoot::new(DOMAIN).apply(&values)?;
    let mut branch = fork(DOMAIN, &root, [23; 32])?;
    assert_eq!(
        branch.scan_prefix(b"")?,
        vec![(b"a0".to_vec(), vec![10]), (b"b1".to_vec(), vec![20])]
    );
    Ok(())
}

#[test]
fn persisted_roots_and_admitted_branches_survive_gc_and_reopen() -> TestResult {
    use attow_network::{
        codec::{Canonical, Object, Value},
        state::FrozenBranch,
        store::Store,
    };

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("worldlines.db");
    let mut store = Store::open(&path)?;
    let root = initial()?;
    let root_id = root.persist(&mut StoreAdapter(&mut store), "root:active")?;
    assert_eq!(root_id, root.object_id()?);
    let mut branch = fork(DOMAIN, &root, [44; 32])?;
    branch.put(b"a0".to_vec(), vec![55])?;
    branch.get(b"missing")?;
    let frozen = branch.freeze()?;
    let branch_id = frozen.admit(&mut StoreAdapter(&mut store), "branch:active")?;
    let garbage = store.put(
        1,
        1,
        &Object {
            refs: BTreeSet::new(),
            payload: Value::Text("unreferenced fixture".into()),
        }
        .encode()?,
    )?;
    assert_eq!(store.collect_garbage()?.deleted, 1);
    assert!(!store.contains(garbage)?);
    assert!(store.contains(root_id)?);
    assert!(store.contains(branch_id)?);
    drop(store);
    let store = Store::open(&path)?;
    let reopened = StateRoot::reopen(&StoreView(&store), root_id, DOMAIN)?;
    assert_eq!(reopened.object_id()?, root.object_id()?);
    assert_eq!(reopened.domain(), root.domain());
    assert_eq!(reopened.sequence(), root.sequence());
    assert_eq!(reopened.get(b"a0")?, Some(vec![10]));
    let restored = FrozenBranch::reopen(&StoreView(&store), branch_id, DOMAIN)?;
    assert_eq!(restored.id(), frozen.id());
    assert_eq!(restored.overlay(), frozen.overlay());
    assert_eq!(restored.dependencies(), frozen.dependencies());
    assert!(StateRoot::reopen(&StoreView(&store), root_id, DomainId([8; 32])).is_err());
    assert!(FrozenBranch::reopen(&StoreView(&store), branch_id, DomainId([8; 32])).is_err());
    Ok(())
}

#[test]
fn durable_reopen_rejects_unknown_object_schema() -> TestResult {
    use attow_network::codec::{Canonical, Object, Value};

    let directory = tempfile::tempdir()?;
    let mut store = attow_network::store::Store::open(&directory.path().join("future.db"))?;
    let future = store.put(1, 99, b"future state format")?;
    assert!(StateRoot::reopen(&StoreView(&store), future, DOMAIN).is_err());
    assert!(
        attow_network::state::FrozenBranch::reopen(&StoreView(&store), future, DOMAIN).is_err()
    );
    let root = initial()?;
    let id = root.persist(&mut StoreAdapter(&mut store), "root:current")?;
    let mut object = Object::decode(&store.get(id)?, &attow_network::codec::Limits::default())?;
    let Value::Array(fields) = &mut object.payload else {
        return Err("root payload is not an array".into());
    };
    *fields.get_mut(1).ok_or("root schema field is absent")? = Value::Unsigned(999);
    let future_payload = store.put(1, 1, &object.encode()?)?;
    assert!(StateRoot::reopen(&StoreView(&store), future_payload, DOMAIN).is_err());
    Ok(())
}

#[test]
fn failed_durable_admission_rolls_back_the_new_base_and_descriptor() -> TestResult {
    use attow_network::{
        codec::ObjectId,
        state::{ObjectData, StateError, StateReader, StateStorage, StateTransaction},
    };

    struct RefusePin<'a>(&'a mut dyn StateTransaction);
    impl StateReader for RefusePin<'_> {
        fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
            self.0.get_object(id)
        }
    }
    impl StateTransaction for RefusePin<'_> {
        fn put_object(&mut self, object: &ObjectData) -> Result<ObjectId, StateError> {
            self.0.put_object(object)
        }
        fn pin_object(&mut self, _: &str, _: ObjectId) -> Result<(), StateError> {
            Err(StateError::Storage)
        }
    }
    struct FailingStorage<'a>(StoreAdapter<'a>);
    impl StateStorage for FailingStorage<'_> {
        fn transaction<T>(
            &mut self,
            operation: impl FnOnce(&mut dyn StateTransaction) -> Result<T, StateError>,
        ) -> Result<T, StateError> {
            self.0
                .transaction(|transaction| operation(&mut RefusePin(transaction)))
        }
    }

    let directory = tempfile::tempdir()?;
    let mut store = attow_network::store::Store::open(&directory.path().join("rollback.db"))?;
    let root = initial()?;
    let frozen = fork(DOMAIN, &root, [45; 32])?.freeze()?;
    assert!(
        frozen
            .admit(
                &mut FailingStorage(StoreAdapter(&mut store)),
                "branch:failed"
            )
            .is_err()
    );
    assert!(!store.contains(root.object_id()?)?);
    store.transaction(|transaction| {
        let objects: i64 =
            transaction
                .sql()
                .query_row("SELECT count(*) FROM objects", [], |row| row.get(0))?;
        let pins: i64 =
            transaction
                .sql()
                .query_row("SELECT count(*) FROM root_pins", [], |row| row.get(0))?;
        assert_eq!((objects, pins), (0, 0));
        Ok(())
    })?;
    Ok(())
}

#[test]
fn conflicts_report_aba_and_scan_phantoms_and_join_checks_reads() -> TestResult {
    use attow_network::state::{Conflict, JoinOutcome, join};

    let root = initial()?;
    let mut left = fork(DOMAIN, &root, [61; 32])?;
    left.get(b"missing")?;
    let left = left.freeze()?;
    let after_insert = root.apply(&BTreeMap::from([(
        b"missing".to_vec(),
        Mutation::Put(vec![1]),
    )]))?;
    let after_delete =
        after_insert.apply(&BTreeMap::from([(b"missing".to_vec(), Mutation::Delete)]))?;
    assert!(left.conflicts(&after_delete)?.iter().any(|conflict| matches!(conflict, Conflict::Key { key, expected: Version(0), actual } if key == b"missing" && actual.0 > 0)));

    let mut scanner = fork(DOMAIN, &root, [62; 32])?;
    scanner.scan_prefix(b"empty/")?;
    let scanner = scanner.freeze()?;
    assert!(
        scanner
            .conflicts(&after_insert)?
            .iter()
            .any(|conflict| matches!(conflict, Conflict::Namespace { .. }))
    );

    let mut writer = fork(DOMAIN, &root, [63; 32])?;
    writer.put(b"missing".to_vec(), vec![3])?;
    let writer = writer.freeze()?;
    assert!(matches!(join(&left, &writer)?, JoinOutcome::Conflicts(_)));
    assert!(matches!(
        join(&scanner, &writer)?,
        JoinOutcome::Conflicts(_)
    ));
    let mut disjoint = fork(DOMAIN, &root, [64; 32])?;
    disjoint.put(b"separate".to_vec(), vec![4])?;
    assert!(matches!(
        join(&writer, &disjoint.freeze()?)?,
        JoinOutcome::Compatible(_)
    ));
    Ok(())
}

#[test]
fn structural_diff_retains_tombstone_versions() -> TestResult {
    use attow_network::state::diff;

    let root = initial()?;
    let changed = root.apply(&BTreeMap::from([
        (b"a0".to_vec(), Mutation::Delete),
        (b"new".to_vec(), Mutation::Put(vec![3])),
    ]))?;
    let changes = diff(&root, &changed)?;
    assert_eq!(changes.len(), 2);
    let deleted = changes
        .iter()
        .find(|change| change.key == b"a0")
        .ok_or("missing delete diff")?;
    assert_eq!(
        deleted.before.as_ref().and_then(|cell| cell.value.as_ref()),
        Some(&vec![10])
    );
    assert_eq!(
        deleted.after.as_ref().and_then(|cell| cell.value.as_ref()),
        None
    );
    assert!(
        deleted.after.as_ref().ok_or("missing tombstone")?.version.0
            > deleted.before.as_ref().ok_or("missing old cell")?.version.0
    );
    assert!(diff(&root, &root)?.is_empty());
    Ok(())
}
