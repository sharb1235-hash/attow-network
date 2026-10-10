//! Atomic checkpoint persistence through the production SQLite facade.
use attow_network::{
    ObjectId,
    codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, Value, object_id},
    sandbox::{SqliteSandboxRepository, runtime::SandboxRepository},
    state::ObjectData,
    store::Store,
};
use std::collections::BTreeSet;

fn data(value: u64, refs: BTreeSet<ObjectId>) -> ObjectData {
    ObjectData {
        kind: OBJECT_KIND,
        schema: OBJECT_SCHEMA,
        bytes: Object {
            refs,
            payload: Value::Unsigned(value),
        }
        .encode()
        .expect("encode"),
    }
}
fn id(object: &ObjectData) -> ObjectId {
    object_id(object.kind, object.schema, &object.bytes)
}

#[test]
fn committed_checkpoints_survive_reopen_and_collection() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("objects.sqlite");
    let child = data(1, BTreeSet::new());
    let root = data(2, BTreeSet::from([id(&child)]));
    let repository = SqliteSandboxRepository::open(&path).expect("repository");
    repository
        .commit("checkpoint", &[child.clone(), root.clone()], &[id(&root)])
        .expect("atomic checkpoint");
    drop(repository);
    Store::open(&path)
        .expect("store")
        .collect_garbage()
        .expect("collection");
    let reopened = SqliteSandboxRepository::open(&path).expect("reopened");
    assert_eq!(reopened.get(id(&child)).expect("child"), child);
    assert_eq!(reopened.get(id(&root)).expect("root"), root);
}

#[test]
fn invalid_checkpoint_rolls_back_every_new_object_and_pin() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("objects.sqlite");
    let repository = SqliteSandboxRepository::open(&path).expect("repository");
    let child = data(1, BTreeSet::new());
    let dangling = data(2, BTreeSet::from([ObjectId([99; 32])]));
    assert!(
        repository
            .commit(
                "checkpoint",
                &[child.clone(), dangling.clone()],
                &[id(&dangling)]
            )
            .is_err()
    );
    assert!(repository.get(id(&child)).is_err());
    assert!(
        repository
            .commit(
                "checkpoint",
                std::slice::from_ref(&child),
                &[ObjectId([88; 32])]
            )
            .is_err()
    );
    assert!(repository.get(id(&child)).is_err());
    let mut store = Store::open(&path).expect("store");
    let kept = store
        .put(child.kind, child.schema, &child.bytes)
        .expect("new child");
    store.collect_garbage().expect("collection");
    assert!(
        !store
            .contains(kept)
            .expect("unpinned after failed checkpoint")
    );
}
