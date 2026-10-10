//! Atomic object and pin persistence for the sandbox's trusted storage port.
use std::{path::Path, sync::Mutex};

use chronos_codec::{Limits, OBJECT_KIND, OBJECT_SCHEMA, ObjectId};
use chronos_sandbox::{SandboxError, SandboxRepository};
use chronos_state::{
    MAX_GRAPH_BYTES, MAX_GRAPH_OBJECTS, ObjectData, StateError, StateReader, verify_object_graph,
};
use chronos_store::{Store, StoreError, StoreTransaction};

/// One serialized SQLite connection. Commits require children before parents;
/// graph verification and every root pin share the object insertion transaction.
pub struct SqliteSandboxRepository {
    store: Mutex<Store>,
}

impl SqliteSandboxRepository {
    pub fn open(path: &Path) -> Result<Self, SandboxError> {
        Ok(Self {
            store: Mutex::new(Store::open(path).map_err(storage_error)?),
        })
    }
}

enum CommitError {
    Store(StoreError),
    Graph(StateError),
}
impl From<StoreError> for CommitError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

struct TransactionReader<'a, 'connection>(&'a StoreTransaction<'connection>);
impl StateReader for TransactionReader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let stored = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: stored.kind,
            schema: stored.schema,
            bytes: stored.bytes,
        })
    }
}

fn storage_error(error: StoreError) -> SandboxError {
    match error {
        StoreError::MissingObject(id) => SandboxError::MissingObject(id),
        StoreError::Codec(error) => SandboxError::Codec(error),
        StoreError::ObjectTooLarge { .. } => SandboxError::Limit("checkpoint object bytes"),
        _ => SandboxError::Storage,
    }
}

impl SandboxRepository for SqliteSandboxRepository {
    fn get(&self, id: ObjectId) -> Result<ObjectData, SandboxError> {
        let object = self
            .store
            .lock()
            .map_err(|_| SandboxError::Poisoned)?
            .get_object(id)
            .map_err(storage_error)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }

    fn commit(
        &self,
        owner: &str,
        objects: &[ObjectData],
        roots: &[ObjectId],
    ) -> Result<(), SandboxError> {
        if owner.is_empty() || owner.len() > 256 || owner.contains('\0') {
            return Err(SandboxError::Limit("pin owner"));
        }
        if objects.len() > MAX_GRAPH_OBJECTS || roots.is_empty() || roots.len() > MAX_GRAPH_OBJECTS
        {
            return Err(SandboxError::Limit("checkpoint objects"));
        }
        let mut bytes = 0_usize;
        for object in objects {
            if (object.kind, object.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
                return Err(SandboxError::InvalidObject);
            }
            if object.bytes.len() > Limits::default().object_bytes {
                return Err(SandboxError::Limit("checkpoint object bytes"));
            }
            bytes = bytes
                .checked_add(object.bytes.len())
                .ok_or(SandboxError::Limit("checkpoint bytes"))?;
            if bytes > MAX_GRAPH_BYTES {
                return Err(SandboxError::Limit("checkpoint bytes"));
            }
        }
        let result: Result<(), CommitError> = self
            .store
            .lock()
            .map_err(|_| SandboxError::Poisoned)?
            .transaction_with(|tx| {
                for object in objects {
                    tx.put(object.kind, object.schema, &object.bytes)?;
                }
                verify_object_graph(&TransactionReader(tx), roots.iter().copied())
                    .map_err(CommitError::Graph)?;
                for id in roots {
                    tx.pin(owner, *id)?;
                }
                Ok(())
            });
        result.map_err(|error| match error {
            CommitError::Store(error) => storage_error(error),
            CommitError::Graph(StateError::Limit(limit)) => SandboxError::Limit(limit),
            CommitError::Graph(StateError::Codec(error)) => SandboxError::Codec(error),
            CommitError::Graph(_) => SandboxError::Storage,
        })
    }
}
