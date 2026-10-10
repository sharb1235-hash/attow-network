use crate::{
    DomainId, MAX_KEYS, MAX_STATE_BYTES, Mutation, StateError, Version, array, bytes, check_key,
    check_schema, check_tag, check_value, id_bytes, next, number,
};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use rpds::HashTrieMapSync;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};

/// A positive version persists even when the value is deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub version: Version,
    pub value: Option<Vec<u8>>,
}

/// Immutable persistent map. Clones share nodes and cached durable bytes.
#[derive(Clone, Debug)]
pub struct StateRoot {
    domain: DomainId,
    sequence: u64,
    epoch: u64,
    pub(crate) cells: HashTrieMapSync<Vec<u8>, Cell>,
    encoded: Arc<OnceLock<Vec<u8>>>,
    identity: Arc<OnceLock<ObjectId>>,
}

impl StateRoot {
    pub fn new(domain: DomainId) -> Self {
        Self {
            domain,
            sequence: 0,
            epoch: 0,
            cells: HashTrieMapSync::new_sync(),
            encoded: Arc::new(OnceLock::new()),
            identity: Arc::new(OnceLock::new()),
        }
    }
    pub fn domain(&self) -> DomainId {
        self.domain
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn namespace_epoch(&self) -> u64 {
        self.epoch
    }
    pub fn cell(&self, key: &[u8]) -> Result<Option<Cell>, StateError> {
        check_key(key)?;
        Ok(self.cells.get(key).cloned())
    }
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError> {
        Ok(self.cell(key)?.and_then(|cell| cell.value))
    }
    pub fn version(&self, key: &[u8]) -> Result<Version, StateError> {
        check_key(key)?;
        Ok(self.cells.get(key).map_or(Version(0), |cell| cell.version))
    }

    /// Mechanically apply a batch with a fresh version. This does not authorize
    /// the batch: an authority must validate its own invariants before calling it.
    pub fn apply(&self, changes: &BTreeMap<Vec<u8>, Mutation>) -> Result<Self, StateError> {
        if changes.is_empty() {
            return Ok(self.clone());
        }
        if changes.len() > MAX_KEYS {
            return Err(StateError::Limit("keys"));
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(StateError::VersionExhausted)?;
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(StateError::VersionExhausted)?;
        // Count the final data before cloning any supplied value into the trie.
        let mut total = 0usize;
        let mut count = self.cells.size();
        for (key, cell) in &self.cells {
            if !changes.contains_key(key) {
                total += key.len() + cell.value.as_ref().map_or(0, Vec::len);
            }
        }
        for (key, mutation) in changes {
            check_key(key)?;
            if !self.cells.contains_key(key) {
                count += 1;
            }
            if count > MAX_KEYS {
                return Err(StateError::Limit("keys including tombstones"));
            }
            let value_bytes = match mutation {
                Mutation::Put(value) => {
                    check_value(value)?;
                    value.len()
                }
                Mutation::Delete => 0,
            };
            total = total
                .checked_add(key.len())
                .and_then(|n| n.checked_add(value_bytes))
                .ok_or(StateError::Limit("state bytes"))?;
            if total > MAX_STATE_BYTES {
                return Err(StateError::Limit("state bytes"));
            }
        }
        let mut cells = self.cells.clone();
        for (key, mutation) in changes {
            check_key(key)?;
            let value = match mutation {
                Mutation::Put(value) => {
                    check_value(value)?;
                    Some(value.clone())
                }
                Mutation::Delete => None,
            };
            cells.insert_mut(
                key.clone(),
                Cell {
                    version: Version(sequence),
                    value,
                },
            );
            if cells.size() > MAX_KEYS {
                return Err(StateError::Limit("keys including tombstones"));
            }
        }
        let root = Self {
            domain: self.domain,
            sequence,
            epoch,
            cells,
            encoded: Arc::new(OnceLock::new()),
            identity: Arc::new(OnceLock::new()),
        };
        root.validate()?;
        root.object_id()?;
        Ok(root)
    }

    fn validate(&self) -> Result<(), StateError> {
        if self.cells.size() > MAX_KEYS {
            return Err(StateError::Limit("keys including tombstones"));
        }
        if self.epoch != self.sequence {
            return Err(StateError::InvalidEncoding);
        }
        let mut total = 0usize;
        for (key, cell) in &self.cells {
            check_key(key)?;
            if cell.version.0 == 0 || cell.version.0 > self.sequence {
                return Err(StateError::InvalidEncoding);
            }
            if let Some(value) = &cell.value {
                check_value(value)?;
            }
            total = total
                .checked_add(key.len())
                .and_then(|n| n.checked_add(cell.value.as_ref().map_or(0, Vec::len)))
                .ok_or(StateError::Limit("state bytes"))?;
            if total > MAX_STATE_BYTES {
                return Err(StateError::Limit("state bytes"));
            }
        }
        Ok(())
    }

    fn object(&self) -> Object {
        let sorted: BTreeMap<_, _> = self.cells.iter().collect();
        let cells = sorted
            .into_iter()
            .map(|(key, cell)| {
                Value::Array(vec![
                    Value::Bytes(key.clone()),
                    Value::Unsigned(cell.version.0),
                    cell.value
                        .as_ref()
                        .map_or(Value::Null, |value| Value::Bytes(value.clone())),
                ])
            })
            .collect();
        Object {
            refs: BTreeSet::new(),
            payload: Value::Array(vec![
                Value::Text("attow-state-root".into()),
                Value::Unsigned(1),
                Value::Bytes(self.domain.0.to_vec()),
                Value::Unsigned(self.sequence),
                Value::Unsigned(self.epoch),
                Value::Array(cells),
            ]),
        }
    }

    pub fn object_id(&self) -> Result<ObjectId, StateError> {
        if let Some(id) = self.identity.get() {
            return Ok(*id);
        }
        let encoded = self.object().encode()?;
        let id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &encoded);
        let _ = self.encoded.set(encoded);
        let _ = self.identity.set(id);
        Ok(id)
    }

    pub fn encode(&self) -> Result<Vec<u8>, StateError> {
        self.object_id()?;
        self.encoded
            .get()
            .cloned()
            .ok_or(StateError::InvalidEncoding)
    }

    pub fn decode(encoded: &[u8], limits: &Limits) -> Result<Self, StateError> {
        let object = Object::decode(encoded, limits)?;
        if !object.refs.is_empty() {
            return Err(StateError::CorruptObject);
        }
        let mut fields = array(object.payload, 6)?.into_iter();
        check_tag(next(&mut fields)?, "attow-state-root")?;
        check_schema(next(&mut fields)?)?;
        let domain = DomainId(id_bytes(next(&mut fields)?)?);
        let sequence = number(next(&mut fields)?)?;
        let epoch = number(next(&mut fields)?)?;
        let Value::Array(values) = next(&mut fields)? else {
            return Err(StateError::InvalidEncoding);
        };
        if values.len() > MAX_KEYS {
            return Err(StateError::Limit("keys"));
        }
        let mut cells = HashTrieMapSync::new_sync();
        let mut previous = None;
        for value in values {
            let mut values = array(value, 3)?.into_iter();
            let key = bytes(next(&mut values)?)?;
            if previous
                .as_ref()
                .is_some_and(|previous: &Vec<u8>| previous >= &key)
            {
                return Err(StateError::InvalidEncoding);
            }
            previous = Some(key.clone());
            let version = Version(number(next(&mut values)?)?);
            let value = match next(&mut values)? {
                Value::Null => None,
                Value::Bytes(value) => Some(value),
                _ => return Err(StateError::InvalidEncoding),
            };
            cells.insert_mut(key, Cell { version, value });
        }
        let root = Self {
            domain,
            sequence,
            epoch,
            cells,
            encoded: Arc::new(OnceLock::new()),
            identity: Arc::new(OnceLock::new()),
        };
        root.validate()?;
        if root.encode()? != encoded {
            return Err(StateError::InvalidEncoding);
        }
        Ok(root)
    }

    /// Explicit data-only import: source sequence, tombstones, and domain identity
    /// are discarded. This never copies authority rights or dispatcher activation.
    pub fn data_only_import(&self, domain: DomainId) -> Result<Self, StateError> {
        let changes = self
            .cells
            .iter()
            .filter_map(|(key, cell)| {
                cell.value
                    .as_ref()
                    .map(|value| (key.clone(), Mutation::Put(value.clone())))
            })
            .collect();
        Self::new(domain).apply(&changes)
    }
}
