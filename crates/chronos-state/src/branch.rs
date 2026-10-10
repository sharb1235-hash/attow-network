use crate::worldline::{data, owner, put_verified, read_envelope, verify_graph};
use crate::{
    BranchId, Dependency, DomainId, MAX_DEPENDENCIES, MAX_KEYS, MAX_STATE_BYTES, Mutation,
    StateError, StateReader, StateRoot, StateStorage, Version, Worldline, array, bytes, check_key,
    check_schema, check_tag, check_value, id_bytes, next, number,
};
use chronos_codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id};
use std::collections::BTreeMap;

/// A mutable, private speculative overlay over an immutable shared base.
pub struct Branch {
    id: BranchId,
    nonce: [u8; 32],
    base: StateRoot,
    reads: BTreeMap<Vec<u8>, Version>,
    scan: bool,
    overlay: BTreeMap<Vec<u8>, Mutation>,
    worldline: Worldline,
}
/// An immutable in-memory proposal. Only `admit` produces a durable pinned root.
#[derive(Clone, Debug)]
pub struct FrozenBranch {
    id: BranchId,
    nonce: [u8; 32],
    base: StateRoot,
    dependencies: Vec<Dependency>,
    overlay: BTreeMap<Vec<u8>, Mutation>,
    worldline: Worldline,
}

fn identity(domain: DomainId, root: ObjectId, nonce: [u8; 32]) -> BranchId {
    let mut input = [0u8; 96];
    input[..32].copy_from_slice(&domain.0);
    input[32..64].copy_from_slice(&root.0);
    input[64..].copy_from_slice(&nonce);
    BranchId(object_id(0x4252, 1, &input).0)
}
pub fn fork(domain: DomainId, root: &StateRoot, nonce: [u8; 32]) -> Result<Branch, StateError> {
    if domain != root.domain() {
        return Err(StateError::DomainMismatch);
    }
    Ok(Branch {
        id: identity(domain, root.object_id()?, nonce),
        nonce,
        base: root.clone(),
        reads: BTreeMap::new(),
        scan: false,
        overlay: BTreeMap::new(),
        worldline: Worldline::default(),
    })
}
pub(crate) fn check_overlay(overlay: &BTreeMap<Vec<u8>, Mutation>) -> Result<(), StateError> {
    if overlay.len() > MAX_KEYS {
        return Err(StateError::Limit("overlay keys"));
    }
    let mut total = 0usize;
    for (key, mutation) in overlay {
        check_key(key)?;
        let len = match mutation {
            Mutation::Put(value) => {
                check_value(value)?;
                value.len()
            }
            Mutation::Delete => 0,
        };
        total = total
            .checked_add(key.len())
            .and_then(|n| n.checked_add(len))
            .ok_or(StateError::Limit("overlay bytes"))?;
        if total > MAX_STATE_BYTES {
            return Err(StateError::Limit("overlay bytes"));
        }
    }
    Ok(())
}
impl Branch {
    pub fn id(&self) -> BranchId {
        self.id
    }
    pub fn domain(&self) -> DomainId {
        self.base.domain()
    }
    pub fn base(&self) -> &StateRoot {
        &self.base
    }
    pub fn set_worldline(&mut self, worldline: Worldline) -> Result<(), StateError> {
        worldline.validate()?;
        self.worldline = worldline;
        Ok(())
    }
    fn track(&mut self, key: &[u8]) -> Result<(), StateError> {
        check_key(key)?;
        if !self.reads.contains_key(key) {
            if self.reads.len() + usize::from(self.scan) >= MAX_DEPENDENCIES {
                return Err(StateError::Limit("dependencies"));
            }
            self.reads.insert(key.to_vec(), self.base.version(key)?);
        }
        Ok(())
    }
    pub fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>, StateError> {
        self.track(key)?;
        match self.overlay.get(key) {
            Some(Mutation::Put(value)) => Ok(Some(value.clone())),
            Some(Mutation::Delete) => Ok(None),
            None => self.base.get(key),
        }
    }
    fn mutate(&mut self, key: Vec<u8>, mutation: Mutation) -> Result<(), StateError> {
        check_key(&key)?;
        if let Mutation::Put(value) = &mutation {
            check_value(value)?;
        }
        let previous = self.overlay.insert(key.clone(), mutation);
        if let Err(error) = check_overlay(&self.overlay) {
            if let Some(previous) = previous {
                self.overlay.insert(key, previous);
            } else {
                self.overlay.remove(&key);
            }
            return Err(error);
        }
        if let Err(error) = self.track(&key) {
            if let Some(previous) = previous {
                self.overlay.insert(key, previous);
            } else {
                self.overlay.remove(&key);
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), StateError> {
        self.mutate(key, Mutation::Put(value))
    }
    pub fn delete(&mut self, key: &[u8]) -> Result<(), StateError> {
        self.mutate(key.to_vec(), Mutation::Delete)
    }
    pub fn scan_prefix(&mut self, prefix: &[u8]) -> Result<crate::ScanRows, StateError> {
        check_key(prefix)?;
        if !self.scan && self.reads.len() >= MAX_DEPENDENCIES {
            return Err(StateError::Limit("dependencies"));
        }
        self.scan = true;
        let mut values: BTreeMap<_, _> = self
            .base
            .cells
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .filter_map(|(key, cell)| {
                cell.value
                    .as_ref()
                    .map(|value| (key.clone(), value.clone()))
            })
            .collect();
        for (key, mutation) in self
            .overlay
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
        {
            match mutation {
                Mutation::Put(value) => {
                    values.insert(key.clone(), value.clone());
                }
                Mutation::Delete => {
                    values.remove(key);
                }
            }
        }
        if values.len() > MAX_KEYS {
            return Err(StateError::Limit("scan results"));
        }
        Ok(values.into_iter().collect())
    }
    pub fn freeze(self) -> Result<FrozenBranch, StateError> {
        self.worldline.validate()?;
        check_overlay(&self.overlay)?;
        let mut dependencies: Vec<_> = self
            .reads
            .into_iter()
            .map(|(key, version)| Dependency::Key { key, version })
            .collect();
        if self.scan {
            dependencies.push(Dependency::Namespace {
                epoch: self.base.namespace_epoch(),
            });
        }
        let frozen = FrozenBranch {
            id: self.id,
            nonce: self.nonce,
            base: self.base,
            dependencies,
            overlay: self.overlay,
            worldline: self.worldline,
        };
        frozen.encode()?;
        Ok(frozen)
    }
}
impl FrozenBranch {
    pub fn id(&self) -> BranchId {
        self.id
    }
    pub fn domain(&self) -> DomainId {
        self.base.domain()
    }
    pub fn base(&self) -> &StateRoot {
        &self.base
    }
    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }
    pub fn overlay(&self) -> &BTreeMap<Vec<u8>, Mutation> {
        &self.overlay
    }
    pub fn worldline(&self) -> &Worldline {
        &self.worldline
    }
    pub fn object_id(&self) -> Result<ObjectId, StateError> {
        Ok(object_id(OBJECT_KIND, OBJECT_SCHEMA, &self.encode()?))
    }
    pub fn encode(&self) -> Result<Vec<u8>, StateError> {
        let base = self.base.object_id()?;
        let mut refs = self.worldline.references();
        refs.insert(base);
        let dependencies = self
            .dependencies
            .iter()
            .map(|dependency| match dependency {
                Dependency::Key { key, version } => Value::Array(vec![
                    Value::Unsigned(0),
                    Value::Bytes(key.clone()),
                    Value::Unsigned(version.0),
                ]),
                Dependency::Namespace { epoch } => {
                    Value::Array(vec![Value::Unsigned(1), Value::Unsigned(*epoch)])
                }
            })
            .collect();
        let overlay = self
            .overlay
            .iter()
            .map(|(key, mutation)| {
                let (tag, value) = match mutation {
                    Mutation::Put(value) => (0, Value::Bytes(value.clone())),
                    Mutation::Delete => (1, Value::Null),
                };
                Value::Array(vec![Value::Bytes(key.clone()), Value::Unsigned(tag), value])
            })
            .collect();
        Ok(Object {
            refs,
            payload: Value::Array(vec![
                Value::Text("attow-frozen-branch".into()),
                Value::Unsigned(1),
                Value::Bytes(self.domain().0.to_vec()),
                Value::Bytes(self.id.0.to_vec()),
                Value::Bytes(self.nonce.to_vec()),
                Value::Bytes(base.0.to_vec()),
                Value::Unsigned(self.base.sequence()),
                Value::Array(dependencies),
                Value::Array(overlay),
                self.worldline.to_value(),
            ]),
        }
        .encode()?)
    }
    /// Validate the complete object graph and atomically persist/pin this proposal.
    pub fn admit(
        &self,
        storage: &mut impl StateStorage,
        pin_owner: &str,
    ) -> Result<ObjectId, StateError> {
        owner(pin_owner)?;
        let base = data(self.base.encode()?);
        let descriptor = data(self.encode()?);
        storage.transaction(|tx| {
            put_verified(tx, &base)?;
            let id = put_verified(tx, &descriptor)?;
            verify_graph(tx, [id])?;
            tx.pin_object(pin_owner, id)?;
            Ok(id)
        })
    }
    pub fn reopen(
        reader: &(impl StateReader + ?Sized),
        id: ObjectId,
        expected_domain: DomainId,
    ) -> Result<Self, StateError> {
        verify_graph(reader, [id])?;
        let object = read_envelope(reader, id)?;
        let mut fields = array(object.payload, 10)?.into_iter();
        check_tag(next(&mut fields)?, "attow-frozen-branch")?;
        check_schema(next(&mut fields)?)?;
        let domain = DomainId(id_bytes(next(&mut fields)?)?);
        if domain != expected_domain {
            return Err(StateError::DomainMismatch);
        }
        let branch_id = BranchId(id_bytes(next(&mut fields)?)?);
        let nonce = id_bytes(next(&mut fields)?)?;
        let base_id = ObjectId(id_bytes(next(&mut fields)?)?);
        let sequence = number(next(&mut fields)?)?;
        let base = StateRoot::reopen(reader, base_id, domain)?;
        if sequence != base.sequence() || branch_id != identity(domain, base_id, nonce) {
            return Err(StateError::InvalidEncoding);
        }
        let Value::Array(encoded_dependencies) = next(&mut fields)? else {
            return Err(StateError::InvalidEncoding);
        };
        if encoded_dependencies.len() > MAX_DEPENDENCIES {
            return Err(StateError::Limit("dependencies"));
        }
        let mut dependencies = Vec::new();
        let mut keys = BTreeMap::new();
        let mut seen_namespace = false;
        for encoded in encoded_dependencies {
            let Value::Array(values) = encoded else {
                return Err(StateError::InvalidEncoding);
            };
            let mut values = values.into_iter();
            match number(next(&mut values)?)? {
                0 => {
                    if seen_namespace {
                        return Err(StateError::InvalidEncoding);
                    }
                    let key = bytes(next(&mut values)?)?;
                    check_key(&key)?;
                    let version = Version(number(next(&mut values)?)?);
                    if values.next().is_some()
                        || keys
                            .last_key_value()
                            .is_some_and(|(previous, _)| previous >= &key)
                        || base.version(&key)? != version
                    {
                        return Err(StateError::InvalidEncoding);
                    }
                    keys.insert(key.clone(), version);
                    dependencies.push(Dependency::Key { key, version });
                }
                1 => {
                    let epoch = number(next(&mut values)?)?;
                    if seen_namespace || values.next().is_some() || epoch != base.namespace_epoch()
                    {
                        return Err(StateError::InvalidEncoding);
                    }
                    seen_namespace = true;
                    dependencies.push(Dependency::Namespace { epoch });
                }
                _ => return Err(StateError::InvalidEncoding),
            }
        }
        let Value::Array(encoded_overlay) = next(&mut fields)? else {
            return Err(StateError::InvalidEncoding);
        };
        if encoded_overlay.len() > MAX_KEYS {
            return Err(StateError::Limit("overlay keys"));
        }
        let mut overlay = BTreeMap::new();
        for encoded in encoded_overlay {
            let mut values = array(encoded, 3)?.into_iter();
            let key = bytes(next(&mut values)?)?;
            if overlay
                .last_key_value()
                .is_some_and(|(previous, _)| previous >= &key)
                || !keys.contains_key(&key)
            {
                return Err(StateError::InvalidEncoding);
            }
            let tag = number(next(&mut values)?)?;
            let value = next(&mut values)?;
            let mutation = match (tag, value) {
                (0, Value::Bytes(value)) => Mutation::Put(value),
                (1, Value::Null) => Mutation::Delete,
                _ => return Err(StateError::InvalidEncoding),
            };
            overlay.insert(key, mutation);
        }
        check_overlay(&overlay)?;
        let worldline = Worldline::from_value(next(&mut fields)?)?;
        let mut expected_refs = worldline.references();
        expected_refs.insert(base_id);
        if object.refs != expected_refs {
            return Err(StateError::CorruptObject);
        }
        let branch = Self {
            id: branch_id,
            nonce,
            base,
            dependencies,
            overlay,
            worldline,
        };
        if branch.object_id()? != id {
            return Err(StateError::CorruptObject);
        }
        Ok(branch)
    }
}
