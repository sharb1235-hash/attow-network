use attow_state::{
    DomainId, FrozenBranch, Mutation, ObjectData, StateError, StateReader, StateRoot, Worldline,
    fork,
};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use std::collections::{BTreeMap, BTreeSet};

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Reader(BTreeMap<ObjectId, ObjectData>);
impl StateReader for Reader {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        self.0.get(&id).cloned().ok_or(StateError::Storage)
    }
}
impl Reader {
    fn insert(&mut self, bytes: Vec<u8>) -> ObjectId {
        let id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes);
        self.0.insert(
            id,
            ObjectData {
                kind: OBJECT_KIND,
                schema: OBJECT_SCHEMA,
                bytes,
            },
        );
        id
    }
}
fn seeded() -> Result<StateRoot, StateError> {
    StateRoot::new(DomainId([1; 32])).apply(&BTreeMap::from([
        (b"a".to_vec(), Mutation::Put(vec![1])),
        (b"b".to_vec(), Mutation::Put(vec![2])),
    ]))
}

#[test]
fn root_rejects_unsorted_duplicate_zero_and_future_versions() -> TestResult {
    let root = seeded()?;
    for variant in 0..4 {
        let mut object = Object::decode(&root.encode()?, &Limits::default())?;
        let Value::Array(fields) = &mut object.payload else {
            return Err("invalid fixture".into());
        };
        let Value::Array(cells) = &mut fields[5] else {
            return Err("invalid fixture".into());
        };
        match variant {
            0 => cells.swap(0, 1),
            1 => cells[1] = cells[0].clone(),
            _ => {
                let Value::Array(fields) = &mut cells[0] else {
                    return Err("invalid fixture".into());
                };
                fields[1] = Value::Unsigned(if variant == 2 { 0 } else { 2 });
            }
        }
        assert!(matches!(
            StateRoot::decode(&object.encode()?, &Limits::default()),
            Err(StateError::InvalidEncoding)
        ));
    }
    Ok(())
}

#[test]
fn frozen_decode_requires_blind_write_dependencies() -> TestResult {
    let root = seeded()?;
    let mut branch = fork(root.domain(), &root, [1; 32])?;
    branch.put(b"new".to_vec(), vec![9])?;
    let frozen = branch.freeze()?;
    let mut object = Object::decode(&frozen.encode()?, &Limits::default())?;
    let Value::Array(fields) = &mut object.payload else {
        return Err("invalid fixture".into());
    };
    fields[7] = Value::Array(Vec::new());
    let mut reader = Reader(BTreeMap::new());
    reader.insert(root.encode()?);
    let id = reader.insert(object.encode()?);
    assert!(matches!(
        FrozenBranch::reopen(&reader, id, root.domain()),
        Err(StateError::InvalidEncoding)
    ));
    Ok(())
}

#[test]
fn metadata_references_must_equal_the_reachable_envelope_set() -> TestResult {
    let root = seeded()?;
    let frozen = fork(root.domain(), &root, [1; 32])?.freeze()?;
    let mut reader = Reader(BTreeMap::new());
    reader.insert(root.encode()?);
    let external = reader.insert(
        Object {
            refs: BTreeSet::new(),
            payload: Value::Null,
        }
        .encode()?,
    );
    let mut object = Object::decode(&frozen.encode()?, &Limits::default())?;
    let Value::Array(fields) = &mut object.payload else {
        return Err("invalid fixture".into());
    };
    let Value::Array(metadata) = &mut fields[9] else {
        return Err("invalid fixture".into());
    };
    metadata[1] = Value::Bytes(external.0.to_vec());
    let id = reader.insert(object.encode()?);
    assert!(matches!(
        FrozenBranch::reopen(&reader, id, root.domain()),
        Err(StateError::CorruptObject)
    ));
    Ok(())
}

#[test]
fn reader_hash_is_not_trusted_and_import_is_data_only() -> TestResult {
    let root = seeded()?;
    let id = root.object_id()?;
    let mut reader = Reader(BTreeMap::new());
    reader.0.insert(
        id,
        ObjectData {
            kind: OBJECT_KIND,
            schema: OBJECT_SCHEMA,
            bytes: vec![0],
        },
    );
    assert!(matches!(
        StateRoot::reopen(&reader, id, root.domain()),
        Err(StateError::CorruptObject)
    ));
    let root = root.apply(&BTreeMap::from([(b"a".to_vec(), Mutation::Delete)]))?;
    let imported = root.data_only_import(DomainId([2; 32]))?;
    assert_eq!(imported.domain(), DomainId([2; 32]));
    assert_eq!(imported.sequence(), 1);
    assert_eq!(imported.version(b"a")?.0, 0);
    assert_eq!(imported.get(b"b")?, Some(vec![2]));
    Ok(())
}

#[test]
fn observation_root_is_reachable_and_descriptor_counts_toward_graph_depth() -> TestResult {
    let root = seeded()?;
    let mut reader = Reader(BTreeMap::new());
    reader.insert(root.encode()?);
    let mut last = reader.insert(
        Object {
            refs: BTreeSet::new(),
            payload: Value::Null,
        }
        .encode()?,
    );
    for _ in 1..64 {
        last = reader.insert(
            Object {
                refs: BTreeSet::from([last]),
                payload: Value::Null,
            }
            .encode()?,
        );
    }
    let mut branch = fork(root.domain(), &root, [2; 32])?;
    branch.set_worldline(Worldline {
        observation_root: Some(last),
        ..Worldline::default()
    })?;
    let frozen = branch.freeze()?;
    let encoded = frozen.encode()?;
    assert!(
        Object::decode(&encoded, &Limits::default())?
            .refs
            .contains(&last)
    );
    let id = reader.insert(encoded);
    assert!(matches!(
        FrozenBranch::reopen(&reader, id, root.domain()),
        Err(StateError::Limit("graph depth"))
    ));
    Ok(())
}

#[test]
fn aggregate_state_limit_is_checked_without_partial_changes() -> TestResult {
    let root = seeded()?;
    let changes = (0..9_u8)
        .map(|key| (vec![key], Mutation::Put(vec![0; 64 * 1024])))
        .collect();
    assert!(matches!(
        root.apply(&changes),
        Err(StateError::Limit("state bytes"))
    ));
    assert_eq!(root.get(b"a")?, Some(vec![1]));
    Ok(())
}
