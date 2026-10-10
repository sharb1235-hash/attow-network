//! Bounded canonical data bundles. No archive paths, databases, activation keys,
//! native compiled artifacts, or implicit execution are accepted by this format.
use crate::{
    CliError, RunRecord, Workspace,
    workspace::{decode_record, validate_id},
};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use chronos_scheduler::{ExecutionTrace, RecordedInputs, replay};
use chronos_state::{
    FrozenBranch, ObjectData, StateError, StateReader, StateRoot, verify_object_graph,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};
const MAX_BYTES: usize = 18 * 1024 * 1024;
const MAX_OBJECTS: usize = 4096;
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
fn limits() -> Limits {
    Limits {
        object_bytes: MAX_BYTES,
        depth: 8,
        items: 32768,
    }
}
fn invalid() -> CliError {
    CliError::input("invalid or incomplete portable data bundle")
}
pub struct Bundle {
    pub record: RunRecord,
    pub objects: BTreeMap<ObjectId, ObjectData>,
}
pub struct BundleReader<'a>(pub &'a BTreeMap<ObjectId, ObjectData>);
impl StateReader for BundleReader<'_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        self.0.get(&id).cloned().ok_or(StateError::CorruptObject)
    }
}
impl Bundle {
    fn roots(&self) -> Result<BTreeSet<ObjectId>, CliError> {
        let record = &self.record;
        let mut roots = BTreeSet::new();
        for id in [
            &record.initial_root,
            &record.managed_root,
            &record.policy,
            &record.payload,
            &record.receipt,
            &record.response_descriptor,
            &record.response_payload,
        ]
        .into_iter()
        .flatten()
        {
            roots.insert(validate_id(id)?);
        }
        for branch in &record.branches {
            roots.insert(validate_id(&branch.descriptor)?);
            roots.insert(validate_id(&branch.state_root)?);
            if let Some(id) = &branch.observation_root {
                roots.insert(validate_id(id)?);
            }
        }
        for id in &record.trace_roots {
            roots.insert(validate_id(id)?);
        }
        for event in &record.events {
            for key in ["receipt", "trace"] {
                if let Some(value) = event.get(key) {
                    roots.insert(validate_id(value.as_str().ok_or_else(invalid)?)?);
                }
            }
        }
        Ok(roots)
    }
    pub fn gather(workspace: &Workspace, record: RunRecord) -> Result<Self, CliError> {
        record.validate()?;
        let mut bundle = Self {
            record,
            objects: BTreeMap::new(),
        };
        let store = chronos_store::Store::open_read_only(&workspace.authority_path())?;
        let roots = bundle.roots()?;
        bundle.objects = store.read_transaction_with::<_, CliError>(|tx| {
            let mut pending = roots;
            let mut objects = BTreeMap::new();
            let mut bytes = 0usize;
            while let Some(id) = pending.pop_first() {
                if objects.contains_key(&id) {
                    continue;
                }
                if objects.len() >= MAX_OBJECTS {
                    return Err(invalid());
                }
                let data = tx.get_object(id)?;
                if (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
                    return Err(invalid());
                }
                bytes = bytes.checked_add(data.bytes.len()).ok_or_else(invalid)?;
                if bytes > MAX_PAYLOAD {
                    return Err(invalid());
                }
                let envelope = Object::decode(&data.bytes, &Limits::default())?;
                for child in envelope.refs {
                    if !objects.contains_key(&child) {
                        pending.insert(child);
                    }
                    if pending.len() > MAX_OBJECTS {
                        return Err(invalid());
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
            Ok(objects)
        })?;
        bundle.verify()?;
        Ok(bundle)
    }
    /// Verifies portable data and recorded semantics without a backend or gateway.
    /// This is consistency checking, not provider attestation or authority activation.
    pub fn verify(&self) -> Result<serde_json::Value, CliError> {
        self.record.validate()?;
        if self.objects.len() > MAX_OBJECTS {
            return Err(invalid());
        }
        let mut bytes = 0usize;
        for (id, data) in &self.objects {
            bytes = bytes.checked_add(data.bytes.len()).ok_or_else(invalid)?;
            if bytes > MAX_PAYLOAD
                || (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA)
                || object_id(data.kind, data.schema, &data.bytes) != *id
            {
                return Err(invalid());
            }
            Object::decode(&data.bytes, &Limits::default())?;
        }
        let reader = BundleReader(&self.objects);
        let roots = self.roots()?;
        verify_object_graph(&reader, roots.iter().copied()).map_err(|_| invalid())?;
        // Reject unreferenced baggage: a bundle is the exact graph closure of its record.
        let mut reached = BTreeSet::new();
        let mut pending = roots.clone();
        while let Some(id) = pending.pop_first() {
            if !reached.insert(id) {
                continue;
            }
            let data = self.objects.get(&id).ok_or_else(invalid)?;
            pending.extend(
                Object::decode(&data.bytes, &Limits::default())?
                    .refs
                    .into_iter()
                    .filter(|id| !reached.contains(id)),
            );
        }
        if reached.len() != self.objects.len() {
            return Err(invalid());
        }
        let initial_id = validate_id(self.record.initial_root.as_deref().ok_or_else(invalid)?)?;
        let initial = StateRoot::decode(
            &self.objects.get(&initial_id).ok_or_else(invalid)?.bytes,
            &Limits::default(),
        )?;
        let mut selected = None;
        for item in &self.record.branches {
            let branch =
                FrozenBranch::reopen(&reader, validate_id(&item.descriptor)?, initial.domain())?;
            if ObjectId(branch.id().0) != validate_id(&item.branch_id)?
                || branch.base().object_id()? != initial_id
                || branch.base().apply(branch.overlay())?.object_id()?
                    != validate_id(&item.state_root)?
                || branch.worldline().observation_root.map(|id| id.to_string())
                    != item.observation_root
            {
                return Err(invalid());
            }
            if self.record.selected_branch.as_ref() == Some(&item.branch_id) {
                selected = Some(branch.base().apply(branch.overlay())?);
            }
        }
        let selected = selected.ok_or_else(invalid)?;
        let mut expected = if matches!(self.record.phase.as_str(), "created" | "prepared") {
            initial_id
        } else {
            selected.object_id()?
        };
        if let Some(id) = &self.record.response_descriptor {
            let response = FrozenBranch::reopen(&reader, validate_id(id)?, initial.domain())?;
            if response.base().object_id()? != selected.object_id()?
                || response
                    .worldline()
                    .observation_root
                    .map(|id| id.to_string())
                    != self.record.receipt
            {
                return Err(invalid());
            }
            if self.record.phase == "completed" {
                expected = response.base().apply(response.overlay())?.object_id()?;
            }
        }
        if Some(expected.to_string()) != self.record.managed_root {
            return Err(invalid());
        }
        StateRoot::decode(
            &self.objects.get(&expected).ok_or_else(invalid)?.bytes,
            &Limits::default(),
        )?;
        if let Some(id) = &self.record.receipt {
            let envelope = Object::decode(
                &self
                    .objects
                    .get(&validate_id(id)?)
                    .ok_or_else(invalid)?
                    .bytes,
                &Limits::default(),
            )?;
            let Value::Array(fields) = envelope.payload else {
                return Err(invalid());
            };
            if fields.len() != 10
                || fields[0] != Value::Text("attow-evidence".into())
                || fields[1] != Value::Unsigned(1)
                || fields[2]
                    != Value::Bytes(
                        validate_id(self.record.intent.as_deref().ok_or_else(invalid)?)?
                            .0
                            .to_vec(),
                    )
                || fields[4]
                    != Value::Bytes(
                        validate_id(self.record.operation.as_deref().ok_or_else(invalid)?)?
                            .0
                            .to_vec(),
                    )
            {
                return Err(invalid());
            }
            let expected_status = match self.record.phase.as_str() {
                "unknown" => Some(2),
                "confirmed" | "response_prepared" | "completed" => Some(3),
                "rejected" => Some(4),
                _ => None,
            };
            if expected_status.is_some_and(|status| fields[8] != Value::Unsigned(status)) {
                return Err(invalid());
            }
        }
        let inputs = RecordedInputs {
            objects: self.objects.clone(),
        };
        let mut traces = Vec::new();
        let mut decoded_traces = BTreeMap::new();
        for id in &self.record.trace_roots {
            let trace = ExecutionTrace::decode(
                &self
                    .objects
                    .get(&validate_id(id)?)
                    .ok_or_else(invalid)?
                    .bytes,
                &Limits::default(),
            )?;
            let report = replay(&trace, &inputs)?;
            if self.record.phase != "created"
                && (!report.failures.is_empty()
                    || !report.cancelled.is_empty()
                    || !report.pending.is_empty()
                    || report.outcomes.len() != trace.graph.nodes.len()
                    || report.outcomes.values().any(|outcome| {
                        !matches!(
                            outcome.exit,
                            chronos_state::execution::StepExit::Completed { .. }
                        )
                    }))
            {
                return Err(invalid());
            }
            traces.push(serde_json::json!({"trace_root":id,"events":report.events,"output_hash":report.output_hash.to_string(),"completed":report.outcomes.len(),"failed":report.failures.len(),"pending":report.pending.len()}));
            decoded_traces.insert(validate_id(id)?, (trace, report));
        }
        if self.record.phase != "created" {
            let mut descriptors: Vec<_> = self
                .record
                .branches
                .iter()
                .map(|branch| branch.descriptor.as_str())
                .collect();
            if let Some(response) = &self.record.response_descriptor {
                descriptors.push(response);
            }
            for descriptor in descriptors {
                let branch =
                    FrozenBranch::reopen(&reader, validate_id(descriptor)?, initial.domain())?;
                let worldline = branch.worldline();
                if worldline.policy.map(|id| id.to_string()) != self.record.policy {
                    return Err(invalid());
                }
                let bindings = Object::decode(
                    &self
                        .objects
                        .get(&worldline.bindings.ok_or_else(invalid)?)
                        .ok_or_else(invalid)?
                        .bytes,
                    &Limits::default(),
                )?;
                let response = self.record.response_descriptor.as_deref() == Some(descriptor);
                let linked = decoded_traces.iter().any(|(id, (trace,report))| {
                    bindings.refs.contains(id)
                        && trace.graph.nodes.iter().any(|node| {
                            node.branch == branch.id()
                                && Some(node.request.code) == worldline.code
                                && Some(node.request.profile) == worldline.profile
                                && Some(node.request.security_scope) == worldline.security_scope
                                && Some(node.request.policy) == worldline.policy
                        })
                        && (!response || (trace.events.iter().any(|event|matches!(event,chronos_scheduler::SemanticEvent::ObservationReplaced{observation_root,..} if Some(*observation_root)==worldline.observation_root))
                            && trace.graph.nodes.iter().filter(|node|node.branch==branch.id()).all(|node|matches!(report.outcomes.get(&node.id).map(|outcome|&outcome.exit),Some(chronos_state::execution::StepExit::Completed{observation_root,..}) if Some(*observation_root)==worldline.observation_root))))
                });
                if !linked {
                    return Err(invalid());
                }
            }
        }
        Ok(
            serde_json::json!({"run_id":self.record.run_id,"recorded_phase":self.record.phase,"fidelity":self.record.fidelity,"intent":self.record.intent,"receipt_root":self.record.receipt,"managed_root":self.record.managed_root,"objects":self.objects.len(),"object_bytes":bytes,"traces":traces,"verification_scope":"portable_data_and_recorded_semantics","provider_attestation":false,"dispatch_calls":0,"worker_calls":0}),
        )
    }
    pub fn encode(&self) -> Result<Vec<u8>, CliError> {
        self.verify()?;
        let entries = self
            .objects
            .iter()
            .map(|(id, data)| {
                Value::Array(vec![
                    Value::Bytes(id.0.to_vec()),
                    Value::Unsigned(u64::from(data.kind)),
                    Value::Unsigned(u64::from(data.schema)),
                    Value::Bytes(data.bytes.clone()),
                ])
            })
            .collect();
        Ok(Value::Array(vec![
            Value::Text("attow-run-bundle".into()),
            Value::Unsigned(1),
            Value::Bytes(serde_json::to_vec(&self.record)?),
            Value::Array(entries),
        ])
        .encode_with_limits(&limits())?)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, CliError> {
        let Value::Array(mut parts) = Value::decode(bytes, &limits()).map_err(|_| invalid())?
        else {
            return Err(invalid());
        };
        if parts.len() != 4
            || parts[0] != Value::Text("attow-run-bundle".into())
            || parts[1] != Value::Unsigned(1)
        {
            return Err(invalid());
        }
        let Value::Array(entries) = parts.pop().ok_or_else(invalid)? else {
            return Err(invalid());
        };
        if entries.len() > MAX_OBJECTS {
            return Err(invalid());
        }
        let Value::Bytes(record) = parts.pop().ok_or_else(invalid)? else {
            return Err(invalid());
        };
        let record = decode_record(&record)?;
        let mut objects = BTreeMap::new();
        let mut last = None;
        let mut total = 0usize;
        for entry in entries {
            let Value::Array(mut fields) = entry else {
                return Err(invalid());
            };
            if fields.len() != 4 {
                return Err(invalid());
            }
            let Value::Bytes(bytes) = fields.pop().ok_or_else(invalid)? else {
                return Err(invalid());
            };
            let schema = match fields.pop() {
                Some(Value::Unsigned(n)) => u16::try_from(n).map_err(|_| invalid())?,
                _ => return Err(invalid()),
            };
            let kind = match fields.pop() {
                Some(Value::Unsigned(n)) => u16::try_from(n).map_err(|_| invalid())?,
                _ => return Err(invalid()),
            };
            let id = match fields.pop() {
                Some(Value::Bytes(bytes)) => ObjectId(bytes.try_into().map_err(|_| invalid())?),
                _ => return Err(invalid()),
            };
            total = total.checked_add(bytes.len()).ok_or_else(invalid)?;
            if total > MAX_PAYLOAD || last.is_some_and(|old| old >= id) {
                return Err(invalid());
            }
            last = Some(id);
            objects.insert(
                id,
                ObjectData {
                    kind,
                    schema,
                    bytes,
                },
            );
        }
        let bundle = Self { record, objects };
        bundle.verify().map_err(|_| invalid())?;
        Ok(bundle)
    }
    pub fn read(path: &Path) -> Result<Self, CliError> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > MAX_BYTES as u64 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        Self::decode(&bytes)
    }
    pub fn import(&self, workspace: &Workspace) -> Result<(), CliError> {
        self.verify()?;
        workspace.reserve_import(&self.record)?;
        let mut store = workspace.store()?;
        let owner = format!("cli-import/{}", self.record.run_id);
        store.transaction_with::<_, CliError>(|tx| {
            let mut pending: BTreeSet<_> = self.objects.keys().copied().collect();
            let mut inserted = BTreeSet::new();
            while !pending.is_empty() {
                let mut progress = false;
                for id in pending.clone() {
                    let data = &self.objects[&id];
                    let object = Object::decode(&data.bytes, &Limits::default())?;
                    if object.refs.iter().all(|child| inserted.contains(child)) {
                        tx.import(id, data.kind, data.schema, &data.bytes)?;
                        inserted.insert(id);
                        pending.remove(&id);
                        progress = true;
                    }
                }
                if !progress {
                    return Err(invalid());
                }
            }
            for root in self.roots()? {
                tx.pin(&owner, root)?;
            }
            Ok(())
        })?;
        workspace.complete_import(&self.record)?;
        Ok(())
    }
    pub fn write(&self, path: &Path) -> Result<(), CliError> {
        let bytes = self.encode()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| CliError::input("export destination exists or is unavailable"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn write_default(&self, workspace: &Workspace) -> Result<(), CliError> {
        self.record.validate()?;
        let bytes = self.encode()?;
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(CliError::infrastructure)?;
        let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory = workspace.data_dir().join("bundles");
        if directory.canonicalize()? != directory {
            return Err(invalid());
        }
        let temporary = directory.join(format!("{name}.tmp"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, workspace.root().join(&self.record.bundle_path))?;
        Ok(())
    }
}
