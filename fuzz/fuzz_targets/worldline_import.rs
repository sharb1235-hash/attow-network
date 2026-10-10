#![no_main]

use attow_codec::{Canonical, Limits, ObjectId, Value, object_id};
use attow_state::{DomainId, FrozenBranch, ObjectData, StateError, StateReader, StateRoot};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

// This data-only importer has no backend, network, authority or execution port.
// Input is canonical [domain, descriptor/root ID or null, [[kind, schema, bytes], ...]].
// A null root selects the first object, making valid seeds mutation-friendly.
struct RecordedObjects(BTreeMap<ObjectId, ObjectData>);
impl StateReader for RecordedObjects {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        self.0.get(&id).cloned().ok_or(StateError::Storage)
    }
}

fn identifier(value: Value) -> Option<[u8; 32]> {
    let Value::Bytes(bytes) = value else {
        return None;
    };
    bytes.try_into().ok()
}

fn exercise(bytes: &[u8]) -> Option<()> {
    if bytes.len() > 262_144 {
        return None;
    }
    let limits = Limits {
        object_bytes: 262_144,
        depth: 8,
        items: 4096,
    };
    let Value::Array(values) = Value::decode(bytes, &limits).ok()? else {
        return None;
    };
    if values.len() != 3 {
        return None;
    }
    let mut values = values.into_iter();
    let domain = DomainId(identifier(values.next()?)?);
    let root = match values.next()? {
        Value::Null => None,
        value => Some(ObjectId(identifier(value)?)),
    };
    let Value::Array(objects) = values.next()? else {
        return None;
    };
    if objects.len() > 32 {
        return None;
    }
    let mut reader = RecordedObjects(BTreeMap::new());
    for object in objects {
        let Value::Array(parts) = object else {
            return None;
        };
        if parts.len() != 3 {
            return None;
        }
        let mut parts = parts.into_iter();
        let Value::Unsigned(kind) = parts.next()? else {
            return None;
        };
        let Value::Unsigned(schema) = parts.next()? else {
            return None;
        };
        let Value::Bytes(bytes) = parts.next()? else {
            return None;
        };
        if bytes.len() > 65_536 {
            return None;
        }
        let object = ObjectData {
            kind: kind.try_into().ok()?,
            schema: schema.try_into().ok()?,
            bytes,
        };
        let id = object_id(object.kind, object.schema, &object.bytes);
        if reader.0.insert(id, object).is_some() {
            return None;
        }
    }
    let root = root.or_else(|| reader.0.keys().next().copied())?;
    if let Ok(branch) = FrozenBranch::reopen(&reader, root, domain) {
        assert_eq!(branch.object_id().expect("valid branch ID"), root);
        assert_eq!(
            branch.encode().expect("valid branch"),
            reader.0[&root].bytes
        );
    }
    if let Ok(state) = StateRoot::reopen(&reader, root, domain) {
        assert_eq!(state.object_id().expect("valid root ID"), root);
        assert_eq!(state.encode().expect("valid root"), reader.0[&root].bytes);
    }
    Some(())
}

fuzz_target!(|bytes: &[u8]| {
    let _ = exercise(bytes);
});
