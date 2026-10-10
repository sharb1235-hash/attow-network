use crate::AdapterError;
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value};
use chronos_state::{ObjectData, StateError, StateReader, verify_object_graph};
use chronos_store::{Store, StoreError, StoreTransaction};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct AdapterStore {
    store: Arc<Mutex<Store>>,
}
impl AdapterStore {
    pub fn open(path: &Path) -> Result<Self, AdapterError> {
        Ok(Self {
            store: Arc::new(Mutex::new(Store::open(path)?)),
        })
    }
    /// The host owns this store. Backend restore paths additionally check the
    /// owner/scope embedded in each snapshot before materializing its data.
    pub fn put_object(
        &self,
        owner: &str,
        payload: Value,
        refs: BTreeSet<ObjectId>,
    ) -> Result<ObjectId, AdapterError> {
        if refs.len() > chronos_state::MAX_GRAPH_OBJECTS {
            return Err(AdapterError::Limit("references"));
        }
        let bytes = Object { refs, payload }.encode()?;
        self.store
            .lock()
            .map_err(|_| AdapterError::Storage)?
            .transaction_with(|tx| {
                let id = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &bytes)?;
                verify_object_graph(&Reader(tx), [id])
                    .map_err(|_| AdapterError::InvalidSnapshot)?;
                tx.pin(owner, id)?;
                Ok(id)
            })
    }
    pub fn get_object(&self, id: ObjectId) -> Result<Object, AdapterError> {
        let data = self
            .store
            .lock()
            .map_err(|_| AdapterError::Storage)?
            .get_object(id)?;
        if (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
            return Err(AdapterError::InvalidSnapshot);
        }
        Ok(Object::decode(&data.bytes, &Limits::default())?)
    }
    /// Explicitly release all roots owned by this operation. Shared descendants
    /// remain reachable through other roots until the normal store GC runs.
    pub fn release(&self, owner: &str) -> Result<(), AdapterError> {
        if owner.is_empty() || owner.len() > 256 || owner.contains('\0') {
            return Err(AdapterError::InvalidRequest);
        }
        self.store
            .lock()
            .map_err(|_| AdapterError::Storage)?
            .transaction_with(|tx| {
                tx.sql()
                    .execute("DELETE FROM root_pins WHERE owner=?1", [owner])?;
                Ok(())
            })
    }
}
impl From<StoreError> for AdapterError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}
struct Reader<'a, 'connection>(&'a StoreTransaction<'connection>);
impl StateReader for Reader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronos_store::Store;
    #[test]
    fn adapter_results_retain_verified_dependencies_and_release_explicitly()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("adapter.sqlite");
        let store = AdapterStore::open(&path)?;
        let input = store.put_object("input", Value::Text("fixture".into()), BTreeSet::new())?;
        let result = store.put_object("result", Value::Unsigned(42), BTreeSet::from([input]))?;
        store.release("input")?;
        Store::open(&path)?.collect_garbage()?;
        assert_eq!(store.get_object(result)?.refs, BTreeSet::from([input]));
        assert_eq!(
            store.get_object(input)?.payload,
            Value::Text("fixture".into())
        );
        store.release("result")?;
        Store::open(&path)?.collect_garbage()?;
        assert!(!Store::open(&path)?.contains(result)?);
        assert!(!Store::open(&path)?.contains(input)?);
        Ok(())
    }
    #[test]
    fn dangling_dependency_does_not_publish_a_partial_adapter_object()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("adapter.sqlite");
        let store = AdapterStore::open(&path)?;
        assert!(
            store
                .put_object("bad", Value::Null, BTreeSet::from([ObjectId([8; 32])]))
                .is_err()
        );
        let count: i64 = Store::open(&path)?.read_transaction_with(|tx| {
            Ok::<_, chronos_store::StoreError>(tx.sql().query_row(
                "SELECT count(*) FROM objects",
                [],
                |row| row.get(0),
            )?)
        })?;
        assert_eq!(count, 0);
        Ok(())
    }
}
