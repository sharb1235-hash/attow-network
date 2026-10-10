use crate::{
    DomainId, MAX_ANCESTRY, MAX_GRAPH_BYTES, MAX_GRAPH_DEPTH, MAX_GRAPH_OBJECTS, StateError,
    StateRoot, array, check_schema, id_bytes, next,
};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id, references,
};
use std::collections::{BTreeMap, BTreeSet};

/// Portable logical roots. Default is an explicitly data-only branch with no
/// executable code or backend credentials. Opaque native handles do not belong here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worldline {
    pub schema: u16,
    pub code: Option<ObjectId>,
    pub profile: Option<ObjectId>,
    pub continuation: Option<ObjectId>,
    pub observation_root: Option<ObjectId>,
    pub policy: Option<ObjectId>,
    pub bindings: Option<ObjectId>,
    pub security_scope: Option<ObjectId>,
    pub ancestry: Vec<ObjectId>,
}

impl Default for Worldline {
    fn default() -> Self {
        Self {
            schema: 1,
            code: None,
            profile: None,
            continuation: None,
            observation_root: None,
            policy: None,
            bindings: None,
            security_scope: None,
            ancestry: Vec::new(),
        }
    }
}

impl Worldline {
    pub fn validate(&self) -> Result<(), StateError> {
        if self.schema != 1 {
            return Err(StateError::UnsupportedSchema);
        }
        if self.ancestry.len() > MAX_ANCESTRY {
            return Err(StateError::Limit("ancestry"));
        }
        if self.ancestry.iter().collect::<BTreeSet<_>>().len() != self.ancestry.len() {
            return Err(StateError::InvalidEncoding);
        }
        Ok(())
    }
    pub fn references(&self) -> BTreeSet<ObjectId> {
        [
            self.code,
            self.profile,
            self.continuation,
            self.observation_root,
            self.policy,
            self.bindings,
            self.security_scope,
        ]
        .into_iter()
        .flatten()
        .chain(self.ancestry.iter().copied())
        .collect()
    }
    pub(crate) fn to_value(&self) -> Value {
        let mut values = vec![Value::Unsigned(u64::from(self.schema))];
        values.extend(
            [
                self.code,
                self.profile,
                self.continuation,
                self.observation_root,
                self.policy,
                self.bindings,
                self.security_scope,
            ]
            .into_iter()
            .map(|id| id.map_or(Value::Null, |id| Value::Bytes(id.0.to_vec()))),
        );
        values.push(Value::Array(
            self.ancestry
                .iter()
                .map(|id| Value::Bytes(id.0.to_vec()))
                .collect(),
        ));
        Value::Array(values)
    }
    pub(crate) fn from_value(value: Value) -> Result<Self, StateError> {
        let mut values = array(value, 9)?.into_iter();
        check_schema(next(&mut values)?)?;
        fn optional(value: Value) -> Result<Option<ObjectId>, StateError> {
            match value {
                Value::Null => Ok(None),
                value => Ok(Some(ObjectId(id_bytes(value)?))),
            }
        }
        let code = optional(next(&mut values)?)?;
        let profile = optional(next(&mut values)?)?;
        let continuation = optional(next(&mut values)?)?;
        let observation_root = optional(next(&mut values)?)?;
        let policy = optional(next(&mut values)?)?;
        let bindings = optional(next(&mut values)?)?;
        let security_scope = optional(next(&mut values)?)?;
        let Value::Array(parents) = next(&mut values)? else {
            return Err(StateError::InvalidEncoding);
        };
        if parents.len() > MAX_ANCESTRY {
            return Err(StateError::Limit("ancestry"));
        }
        let ancestry = parents
            .into_iter()
            .map(|value| Ok(ObjectId(id_bytes(value)?)))
            .collect::<Result<_, StateError>>()?;
        let value = Self {
            schema: 1,
            code,
            profile,
            continuation,
            observation_root,
            policy,
            bindings,
            security_scope,
            ancestry,
        };
        value.validate()?;
        Ok(value)
    }
}

/// Backend-neutral object data. Reader implementations must bound allocations;
/// admission independently checks metadata, hash, schema, and graph limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectData {
    pub kind: u16,
    pub schema: u16,
    pub bytes: Vec<u8>,
}
pub trait StateReader {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError>;
}
/// All operations must use the same protected storage transaction.
pub trait StateTransaction: StateReader {
    fn put_object(&mut self, object: &ObjectData) -> Result<ObjectId, StateError>;
    fn pin_object(&mut self, owner: &str, id: ObjectId) -> Result<(), StateError>;
}
/// Implementations must roll back on error/unwind and serialize object/pin/GC
/// changes. Returning success means the complete callback committed durably.
pub trait StateStorage {
    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn StateTransaction) -> Result<T, StateError>,
    ) -> Result<T, StateError>;
}

pub(crate) fn data(bytes: Vec<u8>) -> ObjectData {
    ObjectData {
        kind: OBJECT_KIND,
        schema: OBJECT_SCHEMA,
        bytes,
    }
}
pub(crate) fn read_verified(
    reader: &(impl StateReader + ?Sized),
    id: ObjectId,
) -> Result<ObjectData, StateError> {
    let object = reader.get_object(id)?;
    if object.bytes.len() > Limits::default().object_bytes {
        return Err(StateError::Limit("object bytes"));
    }
    if object_id(object.kind, object.schema, &object.bytes) != id {
        return Err(StateError::CorruptObject);
    }
    Ok(object)
}
pub(crate) fn read_envelope(
    reader: &(impl StateReader + ?Sized),
    id: ObjectId,
) -> Result<Object, StateError> {
    let object = read_verified(reader, id)?;
    if (object.kind, object.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
        return Err(StateError::UnsupportedSchema);
    }
    Ok(Object::decode(&object.bytes, &Limits::default())?)
}
/// Validate a complete immutable graph using the same bounds as branch admission.
/// The reader must preserve a consistent snapshot for the duration of this call.
pub fn verify_graph(
    reader: &(impl StateReader + ?Sized),
    roots: impl IntoIterator<Item = ObjectId>,
) -> Result<(), StateError> {
    let mut pending: Vec<_> = roots.into_iter().map(|id| (id, 1usize)).collect();
    let mut visited = BTreeMap::new();
    let mut reads = 0usize;
    let mut bytes = 0usize;
    while let Some((id, depth)) = pending.pop() {
        if depth > MAX_GRAPH_DEPTH {
            return Err(StateError::Limit("graph depth"));
        }
        if visited.get(&id).is_some_and(|previous| *previous >= depth) {
            continue;
        }
        if reads >= MAX_GRAPH_OBJECTS {
            return Err(StateError::Limit("graph objects"));
        }
        reads += 1;
        let object = read_verified(reader, id)?;
        bytes = bytes
            .checked_add(object.bytes.len())
            .ok_or(StateError::Limit("graph bytes"))?;
        if bytes > MAX_GRAPH_BYTES {
            return Err(StateError::Limit("graph bytes"));
        }
        let children = references(
            object.kind,
            object.schema,
            &object.bytes,
            &Limits::default(),
        )?;
        if pending
            .len()
            .checked_add(children.len())
            .is_none_or(|count| count > MAX_GRAPH_OBJECTS)
        {
            return Err(StateError::Limit("pending graph objects"));
        }
        visited.insert(id, depth);
        pending.extend(children.into_iter().map(|child| (child, depth + 1)));
    }
    Ok(())
}
pub(crate) fn put_verified(
    tx: &mut dyn StateTransaction,
    object: &ObjectData,
) -> Result<ObjectId, StateError> {
    let expected = object_id(object.kind, object.schema, &object.bytes);
    let actual = tx.put_object(object)?;
    if expected != actual {
        return Err(StateError::CorruptObject);
    }
    Ok(actual)
}
pub(crate) fn owner(owner: &str) -> Result<(), StateError> {
    if owner.is_empty() || owner.len() > 256 || owner.contains('\0') {
        return Err(StateError::Limit("pin owner"));
    }
    Ok(())
}

impl StateRoot {
    pub fn persist(
        &self,
        storage: &mut impl StateStorage,
        pin_owner: &str,
    ) -> Result<ObjectId, StateError> {
        owner(pin_owner)?;
        let data = data(self.encode()?);
        storage.transaction(|tx| {
            let id = put_verified(tx, &data)?;
            read_envelope(tx, id)?;
            tx.pin_object(pin_owner, id)?;
            Ok(id)
        })
    }
    pub fn reopen(
        reader: &(impl StateReader + ?Sized),
        id: ObjectId,
        expected_domain: DomainId,
    ) -> Result<Self, StateError> {
        let object = read_verified(reader, id)?;
        if (object.kind, object.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
            return Err(StateError::UnsupportedSchema);
        }
        let root = Self::decode(&object.bytes, &Limits::default())?;
        if root.domain() != expected_domain {
            return Err(StateError::DomainMismatch);
        }
        Ok(root)
    }
}
