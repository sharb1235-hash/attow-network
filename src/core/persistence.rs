//! Compose portable state storage traits with the local transactional object store.
//! This module adds no authority or effect dispatch to state admission.

use chronos_codec::ObjectId;
use chronos_state::{ObjectData, StateError, StateReader, StateStorage, StateTransaction};
use chronos_store::{Store, StoreError, StoreTransaction};

/// Read-only view for verifying and reopening a portable state root or descriptor.
pub struct StoredStateView<'a>(pub &'a Store);

/// Local state admission using the store's existing connection and one transaction.
pub struct LocalStateStorage<'a>(pub &'a mut Store);

struct TransactionView<'a, 'connection>(&'a mut StoreTransaction<'connection>);

enum AdmissionError {
    State(StateError),
    Storage,
}

impl From<StoreError> for AdmissionError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}

impl StateReader for StoredStateView<'_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}

impl StateReader for LocalStateStorage<'_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        StoredStateView(self.0).get_object(id)
    }
}

impl StateReader for TransactionView<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}

impl StateTransaction for TransactionView<'_, '_> {
    fn put_object(&mut self, object: &ObjectData) -> Result<ObjectId, StateError> {
        self.0
            .put(object.kind, object.schema, &object.bytes)
            .map_err(|_| StateError::Storage)
    }

    fn pin_object(&mut self, owner: &str, id: ObjectId) -> Result<(), StateError> {
        self.0.pin(owner, id).map_err(|_| StateError::Storage)
    }
}

impl StateStorage for LocalStateStorage<'_> {
    fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut dyn StateTransaction) -> Result<T, StateError>,
    ) -> Result<T, StateError> {
        let result: Result<T, AdmissionError> = self.0.transaction_with(|transaction| {
            operation(&mut TransactionView(transaction)).map_err(AdmissionError::State)
        });
        result.map_err(|error| match error {
            AdmissionError::State(error) => error,
            AdmissionError::Storage => StateError::Storage,
        })
    }
}
