//! Hash-checked result and input retention in one local SQLite transaction.
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId};
use chronos_scheduler::{ExecutionTrace, RecordedInputs, ResultRepository, SemanticEvent};
use chronos_state::{
    ObjectData, StateError, StateReader,
    execution::{ExecutionError, StepOutcome, StepRequest},
    verify_object_graph,
};
use chronos_store::{Store, StoreError, StoreReadTransaction, StoreTransaction};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::Path,
    sync::Mutex,
};

pub struct SqliteResultRepository {
    store: Mutex<Store>,
}
impl SqliteResultRepository {
    /// Retain a portable trace and its complete object graph atomically.
    pub fn store_trace(
        &self,
        owner: &str,
        trace: &ExecutionTrace,
    ) -> Result<ObjectId, ExecutionError> {
        if owner.is_empty() || owner.len() > 256 || owner.contains('\0') {
            return Err(ExecutionError::InvalidRequest);
        }
        let bytes = trace.encode().map_err(|_| ExecutionError::InvalidRequest)?;
        let result: Result<ObjectId, RetentionError> = self
            .store
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .transaction_with(|tx| {
                let id = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &bytes)?;
                let reader = CountedReader {
                    transaction: tx,
                    seen: RefCell::new(BTreeSet::new()),
                    bytes: Cell::new(0),
                };
                verify_object_graph(&reader, [id]).map_err(RetentionError::Graph)?;
                tx.pin(owner, id)?;
                Ok(id)
            });
        result.map_err(retention_error)
    }
    /// Reopen a hash-checked trace. This performs no execution or dispatch.
    pub fn load_trace(&self, id: ObjectId) -> Result<ExecutionTrace, ExecutionError> {
        let store = self
            .store
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        let result: Result<ExecutionTrace, RetentionError> = store.read_transaction_with(|tx| {
            let object = tx.get_object(id)?;
            if (object.kind, object.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
                return Err(RetentionError::Graph(StateError::Storage));
            }
            let trace = ExecutionTrace::decode(&object.bytes, &Limits::default())
                .map_err(|error| RetentionError::Store(StoreError::Codec(error)))?;
            verify_object_graph(&SnapshotReader(tx), [id]).map_err(RetentionError::Graph)?;
            Ok(trace)
        });
        result.map_err(retention_error)
    }
    pub fn open(path: &Path) -> Result<Self, ExecutionError> {
        Ok(Self {
            store: Mutex::new(Store::open(path).map_err(storage_error)?),
        })
    }
    /// Export a bounded, consistent read snapshot for the offline replay verifier.
    /// The caller retains the trace itself; this method never invokes a backend.
    pub fn recorded_inputs(
        &self,
        trace: &ExecutionTrace,
    ) -> Result<RecordedInputs, ExecutionError> {
        trace
            .graph
            .validate()
            .map_err(|_| ExecutionError::InvalidRequest)?;
        if trace.events.len() > 16_384 {
            return Err(ExecutionError::ResourceExhausted);
        }
        let mut pending = BTreeSet::new();
        let mut add = |ids: BTreeSet<ObjectId>| -> Result<(), ExecutionError> {
            for id in ids {
                pending.insert(id);
                if pending.len() > chronos_state::MAX_GRAPH_OBJECTS {
                    return Err(ExecutionError::ResourceExhausted);
                }
            }
            Ok(())
        };
        for node in &trace.graph.nodes {
            add(node.request.references())?;
        }
        for event in &trace.events {
            match event {
                SemanticEvent::Scheduled { request, .. } => add(request.references())?,
                SemanticEvent::Completed { outcome, .. } => add(outcome.references())?,
                SemanticEvent::ObservationReplaced {
                    observation_root, ..
                } => add(BTreeSet::from([*observation_root]))?,
                _ => {}
            }
        }
        self.store
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .read_transaction_with(|tx| {
                let mut objects = std::collections::BTreeMap::new();
                let mut bytes = 0_usize;
                while let Some(id) = pending.pop_first() {
                    if objects.contains_key(&id) {
                        continue;
                    }
                    if objects.len() >= chronos_state::MAX_GRAPH_OBJECTS {
                        return Err(StoreError::CollectionBudget("replay object count"));
                    }
                    let data = tx.get_object(id)?;
                    if (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
                        return Err(StoreError::Codec(chronos_codec::CodecError::UnknownSchema));
                    }
                    bytes = bytes
                        .checked_add(data.bytes.len())
                        .ok_or(StoreError::CollectionBudget("replay bytes"))?;
                    if bytes > chronos_state::MAX_GRAPH_BYTES {
                        return Err(StoreError::CollectionBudget("replay bytes"));
                    }
                    let object = Object::decode(&data.bytes, &Limits::default())?;
                    for child in object.refs {
                        if !objects.contains_key(&child) {
                            pending.insert(child);
                        }
                        if pending.len() > chronos_state::MAX_GRAPH_OBJECTS {
                            return Err(StoreError::CollectionBudget("pending replay objects"));
                        }
                    }
                    objects.insert(
                        id,
                        ObjectData {
                            kind: data.kind,
                            schema: data.schema,
                            bytes: data.bytes,
                        },
                    );
                }
                Ok(RecordedInputs { objects })
            })
            .map_err(storage_error)
    }
}

enum RetentionError {
    Store(StoreError),
    Graph(StateError),
}
impl From<StoreError> for RetentionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
fn retention_error(error: RetentionError) -> ExecutionError {
    match error {
        RetentionError::Store(error) => storage_error(error),
        RetentionError::Graph(StateError::Limit(_)) => ExecutionError::ResourceExhausted,
        RetentionError::Graph(StateError::Storage) => ExecutionError::BackendUnavailable,
        RetentionError::Graph(_) => ExecutionError::InvalidRequest,
    }
}
fn storage_error(error: StoreError) -> ExecutionError {
    match error {
        StoreError::MissingObject(id) => ExecutionError::MissingObject(id),
        StoreError::ObjectTooLarge { .. }
        | StoreError::CollectionBudget(_)
        | StoreError::CollectionLimit(_) => ExecutionError::ResourceExhausted,
        StoreError::Codec(_) | StoreError::CorruptObject(_) => ExecutionError::InvalidRequest,
        _ => ExecutionError::BackendUnavailable,
    }
}
struct CountedReader<'a, 'connection> {
    transaction: &'a StoreTransaction<'connection>,
    seen: RefCell<BTreeSet<ObjectId>>,
    bytes: Cell<u64>,
}
struct SnapshotReader<'a, 'connection>(&'a StoreReadTransaction<'connection>);
impl StateReader for SnapshotReader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}
impl StateReader for CountedReader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self
            .transaction
            .get_object(id)
            .map_err(|_| StateError::Storage)?;
        if self.seen.borrow_mut().insert(id) {
            self.bytes.set(
                self.bytes
                    .get()
                    .checked_add(object.bytes.len() as u64)
                    .ok_or(StateError::Limit("retained graph bytes"))?,
            );
        }
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}

impl ResultRepository for SqliteResultRepository {
    fn retain(
        &self,
        owner: &str,
        request: &StepRequest,
        outcome: &StepOutcome,
        max_bytes: u64,
    ) -> Result<u64, ExecutionError> {
        outcome
            .validate_for(request)
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let request_bytes = request
            .encode()
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let outcome_bytes = outcome
            .encode()
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let result: Result<u64, RetentionError> = self
            .store
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .transaction_with(|tx| {
                let request_id = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &request_bytes)?;
                let outcome_id = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &outcome_bytes)?;
                let reader = CountedReader {
                    transaction: tx,
                    seen: RefCell::new(BTreeSet::new()),
                    bytes: Cell::new(0),
                };
                verify_object_graph(&reader, [request_id, outcome_id])
                    .map_err(RetentionError::Graph)?;
                let bytes = reader.bytes.get();
                if bytes > max_bytes {
                    return Err(RetentionError::Graph(StateError::Limit("retention budget")));
                }
                tx.pin(owner, request_id)?;
                tx.pin(owner, outcome_id)?;
                Ok(bytes)
            });
        result.map_err(|error| match error {
            RetentionError::Store(error) => storage_error(error),
            RetentionError::Graph(StateError::Limit(_)) => ExecutionError::ResourceExhausted,
            RetentionError::Graph(StateError::Storage) => ExecutionError::BackendUnavailable,
            RetentionError::Graph(_) => ExecutionError::InvalidRequest,
        })
    }
    fn release(&self, owner: &str) -> Result<(), ExecutionError> {
        if owner.is_empty() || owner.len() > 256 || owner.contains('\0') {
            return Err(ExecutionError::InvalidRequest);
        }
        self.store
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .transaction(|tx| {
                tx.sql()
                    .execute("DELETE FROM root_pins WHERE owner = ?1", [owner])?;
                Ok(())
            })
            .map_err(storage_error)
    }
}
