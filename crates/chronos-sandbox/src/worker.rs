use crate::*;
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, Value, object_id};
use chronos_state::execution::{
    ExecutionError, ResourceClass, StepOutcome, StepRequest, WorkerBackend,
};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use wasmtime::Store;

const MAX_CACHED_CODES: usize = 16;
const MAX_CACHED_CODE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ADMISSIONS: usize = 128;

struct WorkerPermit<'a>(&'a std::sync::atomic::AtomicUsize);
impl Drop for WorkerPermit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn object(bytes: Vec<u8>) -> ObjectData {
    ObjectData {
        kind: OBJECT_KIND,
        schema: OBJECT_SCHEMA,
        bytes,
    }
}
fn id(object: &ObjectData) -> ObjectId {
    object_id(object.kind, object.schema, &object.bytes)
}
fn value_id(value: ObjectId) -> Value {
    Value::Bytes(value.0.to_vec())
}
fn optional_id(value: Option<ObjectId>) -> Value {
    value.map_or(Value::Null, value_id)
}

impl Executor {
    pub fn new(repository: Arc<dyn SandboxRepository>) -> Result<Self, SandboxError> {
        Ok(Self {
            engine: admission::engine()?,
            repository,
            cache: Mutex::new(CodeCache {
                codes: BTreeMap::new(),
                bytes: 0,
            }),
            admissions: Mutex::new(Vec::new()),
            active: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    fn read_object(&self, id: ObjectId) -> Result<ObjectData, SandboxError> {
        let data = self.repository.get(id)?;
        if (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA)
            || data.bytes.len() > Limits::default().object_bytes
            || object_id(data.kind, data.schema, &data.bytes) != id
        {
            return Err(SandboxError::InvalidObject);
        }
        Object::decode(&data.bytes, &Limits::default())?;
        Ok(data)
    }
    pub fn admit(&self, wasm: &[u8]) -> Result<AdmittedCode, SandboxError> {
        if wasm.len() > profile::MAX_CODE_BYTES {
            return Err(SandboxError::Admission);
        }
        // Serialize compilation and its source-object allocation together.
        let mut cache = self.cache.lock().map_err(|_| SandboxError::Poisoned)?;
        let stored = object(
            Object {
                refs: BTreeSet::new(),
                payload: Value::Array(vec![
                    Value::Text("attow-wasm-code".into()),
                    Value::Unsigned(1),
                    Value::Bytes(wasm.to_vec()),
                ]),
            }
            .encode()?,
        );
        let id = id(&stored);
        // The cache mutex also bounds local compilation concurrency to one.
        if let Some(code) = cache.codes.get(&id) {
            return Ok(code.clone());
        }
        let next_bytes = cache
            .bytes
            .checked_add(wasm.len())
            .ok_or(SandboxError::Limit("compiled code cache"))?;
        if cache.codes.len() >= MAX_CACHED_CODES || next_bytes > MAX_CACHED_CODE_BYTES {
            return Err(SandboxError::Limit("compiled code cache"));
        }
        let component = admission::compile(&self.engine, wasm)?;
        let code = AdmittedCode { component, id };
        self.repository
            .commit(&format!("sandbox:code:{id}"), &[stored], &[id])?;
        cache.bytes = next_bytes;
        cache.codes.insert(id, code.clone());
        Ok(code)
    }
    /// Read source WASM from the immutable repository and compile it locally.
    /// Native compiled artifacts are never deserialized.
    pub fn load_code(&self, id: ObjectId) -> Result<AdmittedCode, SandboxError> {
        let data = self.read_object(id)?;
        let stored = Object::decode(&data.bytes, &Limits::default())?;
        if !stored.refs.is_empty() {
            return Err(SandboxError::InvalidObject);
        }
        let Value::Array(fields) = stored.payload else {
            return Err(SandboxError::InvalidObject);
        };
        let [Value::Text(tag), Value::Unsigned(1), Value::Bytes(wasm)]: [Value; 3] =
            fields.try_into().map_err(|_| SandboxError::InvalidObject)?
        else {
            return Err(SandboxError::InvalidObject);
        };
        if tag != "attow-wasm-code" {
            return Err(SandboxError::InvalidObject);
        }
        let code = self.admit(&wasm)?;
        if code.id != id {
            return Err(SandboxError::InvalidObject);
        }
        Ok(code)
    }
    pub fn store_data(&self, bytes: &[u8]) -> Result<ObjectId, SandboxError> {
        let data = object(continuation::encode_data(bytes)?);
        let id = id(&data);
        self.repository
            .commit(&format!("sandbox:data:{id}"), &[data], &[id])?;
        Ok(id)
    }
    pub fn read_data(&self, id: ObjectId) -> Result<Vec<u8>, SandboxError> {
        continuation::decode_data(&self.read_object(id)?.bytes)
    }
    pub fn initial_continuation(
        &self,
        code: &AdmittedCode,
        profile: &ExecutionProfile,
        bytes: &[u8],
    ) -> Result<ObjectId, SandboxError> {
        continuation::bounded(bytes)?;
        profile.validate()?;
        if !Engine::same(code.component.engine(), &self.engine) {
            return Err(SandboxError::Admission);
        }
        self.read_object(code.id)?;
        let profile_object = object(profile.encode()?);
        if id(&profile_object) != profile.profile_hash {
            return Err(SandboxError::InvalidProfile);
        }
        let checkpoint = object(
            Continuation {
                code: code.id,
                profile: profile.profile_hash,
                bytes: bytes.to_vec(),
            }
            .encode()?,
        );
        let id = id(&checkpoint);
        self.repository.commit(
            &format!("sandbox:checkpoint:{id}"),
            &[profile_object, checkpoint],
            &[id],
        )?;
        Ok(id)
    }
    fn prepare(
        &self,
        code: &AdmittedCode,
        input: StepInput,
        profile: &ExecutionProfile,
    ) -> Result<[Vec<u8>; 3], SandboxError> {
        profile.validate()?;
        if input.code != code.id || !Engine::same(code.component.engine(), &self.engine) {
            return Err(SandboxError::Admission);
        }
        self.read_object(code.id)?;
        let checkpoint = Continuation::decode(&self.read_object(input.continuation)?.bytes)?;
        if checkpoint.code != code.id || checkpoint.profile != profile.profile_hash {
            return Err(SandboxError::InvalidContinuation);
        }
        let stored_profile =
            ExecutionProfile::decode(&self.read_object(profile.profile_hash)?.bytes)?;
        if stored_profile != *profile {
            return Err(SandboxError::InvalidProfile);
        }
        Ok([
            self.read_data(input.state)?,
            checkpoint.bytes,
            input
                .observation
                .map(|id| self.read_data(id))
                .transpose()?
                .unwrap_or_default(),
        ])
    }
    pub fn run_step(
        &self,
        code: &AdmittedCode,
        input: StepInput,
        profile: &ExecutionProfile,
    ) -> Result<StepExit, SandboxError> {
        self.execute(code, input, profile, None)
    }
    fn execute(
        &self,
        code: &AdmittedCode,
        input: StepInput,
        profile: &ExecutionProfile,
        request: Option<&StepRequest>,
    ) -> Result<StepExit, SandboxError> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < profile::MAX_WORKERS as usize).then_some(active + 1)
            })
            .map_err(|_| SandboxError::Limit("active workers"))?;
        let _permit = WorkerPermit(&self.active);
        let inputs = self.prepare(code, input, profile)?;
        let effect_class = request.map(|request| request.effect_class);
        let host = host::Host::new(
            inputs,
            profile.memory_bytes,
            profile.output_bytes,
            profile::MAX_HOST_CALLS,
            effect_class,
        );
        // Each attempt owns a new store; all exits, including traps and host
        // failures, drop the store before any subsequent guest entry.
        let mut store = Store::new(&self.engine, host);
        store.limiter(|host| &mut host.limits);
        store
            .set_fuel(profile.fuel)
            .map_err(|_| SandboxError::InvalidProfile)?;
        let linker = host::linker(&self.engine)?;
        let instance = linker
            .instantiate(&mut store, &code.component)
            .map_err(host::runtime_error)?;
        let step = instance
            .get_typed_func::<(), (u32,)>(&mut store, "step")
            .map_err(|_| SandboxError::Admission)?;
        // Wasmtime48 call performs component post-return itself.
        let (tag,) = step.call(&mut store, ()).map_err(host::runtime_error)?;
        let host::Host {
            outputs: [result, next, request_bytes, delta],
            ..
        } = store.into_data();
        if tag > 1
            || (tag == 0 && (!next.is_empty() || !request_bytes.is_empty()))
            || (tag == 1 && !result.is_empty())
        {
            return Err(SandboxError::InvalidInput);
        }
        let delta = object(continuation::encode_data(&delta)?);
        let delta_id = id(&delta);
        let mut objects = vec![delta];
        let exit = if tag == 0 {
            let result = object(continuation::encode_data(&result)?);
            let result_id = id(&result);
            objects.push(result);
            StepExit::Done {
                result: result_id,
                delta: delta_id,
            }
        } else {
            let checkpoint = object(
                Continuation {
                    code: code.id,
                    profile: profile.profile_hash,
                    bytes: next,
                }
                .encode()?,
            );
            let checkpoint_id = id(&checkpoint);
            let request_object = object(continuation::encode_data(&request_bytes)?);
            let request_id = id(&request_object);
            objects.extend([checkpoint, request_object]);
            StepExit::Suspended {
                continuation: checkpoint_id,
                request: request_id,
                delta: delta_id,
            }
        };
        let (primary, effect_request) = match exit {
            StepExit::Done { result, .. } => (result, None),
            StepExit::Suspended {
                continuation,
                request,
                ..
            } => (continuation, Some(request)),
        };
        let mut refs = BTreeSet::from([
            code.id,
            profile.profile_hash,
            input.state,
            input.continuation,
            primary,
            delta_id,
        ]);
        refs.extend(input.observation);
        refs.extend(effect_request);
        let evaluation = object(
            Object {
                refs,
                payload: Value::Array(vec![
                    Value::Text("attow-wasm-evaluation".into()),
                    Value::Unsigned(1),
                    value_id(code.id),
                    value_id(profile.profile_hash),
                    value_id(input.state),
                    value_id(input.continuation),
                    optional_id(input.observation),
                    Value::Unsigned(u64::from(tag)),
                    value_id(primary),
                    optional_id(effect_request),
                    value_id(delta_id),
                ]),
            }
            .encode()?,
        );
        let evaluation_id = id(&evaluation);
        objects.push(evaluation);
        let mut roots = vec![evaluation_id];
        if let Some(request) = request {
            let outcome = object(port_outcome(request, &exit).encode()?);
            roots.push(id(&outcome));
            objects.push(outcome);
        }
        self.repository.commit(
            &format!("sandbox:evaluation:{evaluation_id}"),
            &objects,
            &roots,
        )?;
        Ok(exit)
    }
    pub fn register(&self, admission: WorkerAdmission) -> Result<(), SandboxError> {
        if !matches!(
            admission.effect_class,
            EffectClass::Pure
                | EffectClass::ManagedRead
                | EffectClass::ManagedWrite
                | EffectClass::RecordedObservation
                | EffectClass::PreparedExternal
        ) {
            return Err(SandboxError::UnsupportedRequest);
        }
        self.load_code(admission.code)?;
        ExecutionProfile::decode(&self.read_object(admission.profile)?.bytes)?;
        self.read_object(admission.policy)?;
        self.read_object(admission.fidelity)?;
        let mut admissions = self.admissions.lock().map_err(|_| SandboxError::Poisoned)?;
        if admissions.contains(&admission) {
            return Ok(());
        }
        if admissions.len() >= MAX_ADMISSIONS {
            return Err(SandboxError::Limit("worker admissions"));
        }
        admissions.push(admission);
        Ok(())
    }
    fn validate(&self, request: &StepRequest) -> Result<ExecutionProfile, SandboxError> {
        if request.resource_class != ResourceClass::Wasm {
            return Err(SandboxError::UnsupportedRequest);
        }
        request.budget.validate()?;
        let expected = WorkerAdmission {
            tenant: request.tenant,
            security_scope: request.security_scope,
            policy: request.policy,
            fidelity: request.fidelity,
            code: request.code,
            profile: request.profile,
            effect_class: request.effect_class,
        };
        if !self
            .admissions
            .lock()
            .map_err(|_| SandboxError::Poisoned)?
            .contains(&expected)
        {
            return Err(SandboxError::UnsupportedRequest);
        }
        let profile = ExecutionProfile::decode(&self.read_object(request.profile)?.bytes)?;
        if request.budget.fuel != profile.fuel
            || request.budget.memory_bytes != profile.memory_bytes as u64
            || request.budget.output_bytes != profile.output_bytes as u64
            || request.budget.host_calls != profile::MAX_HOST_CALLS
        {
            return Err(SandboxError::InvalidProfile);
        }
        let code = self.load_code(request.code)?;
        self.prepare(
            &code,
            StepInput {
                code: request.code,
                state: request.input_root,
                continuation: request.continuation,
                observation: Some(request.observation_root),
            },
            &profile,
        )?;
        self.read_object(request.policy)?;
        self.read_object(request.fidelity)?;
        Ok(profile)
    }
}

fn port_outcome(request: &StepRequest, exit: &StepExit) -> StepOutcome {
    use chronos_state::execution::StepExit as PortExit;
    StepOutcome {
        attempt: request.attempt,
        profile: request.profile,
        exit: match *exit {
            StepExit::Done { result, delta } => PortExit::Completed {
                result,
                delta,
                observation_root: request.observation_root,
            },
            StepExit::Suspended {
                continuation,
                request: proposal,
                delta,
            } => PortExit::Suspended {
                continuation,
                request: proposal,
                delta,
                observation_root: request.observation_root,
            },
        },
    }
}
fn execution_error(error: SandboxError) -> ExecutionError {
    match error {
        SandboxError::MissingObject(id) => ExecutionError::MissingObject(id),
        SandboxError::InvalidProfile => ExecutionError::UnsupportedProfile,
        SandboxError::Fuel
        | SandboxError::HostCall
        | SandboxError::Output
        | SandboxError::Limit(_) => ExecutionError::ResourceExhausted,
        SandboxError::Trap => ExecutionError::Trapped,
        SandboxError::Storage | SandboxError::Poisoned => ExecutionError::BackendUnavailable,
        _ => ExecutionError::InvalidRequest,
    }
}
impl WorkerBackend for Executor {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.validate(request).map(|_| ()).map_err(execution_error)
    }
    fn permits_pure_reuse(&self, request: &StepRequest) -> bool {
        request.effect_class == EffectClass::Pure && self.validate(request).is_ok()
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        let profile = self.validate(&request).map_err(execution_error)?;
        let code = self.load_code(request.code).map_err(execution_error)?;
        let input = StepInput {
            code: request.code,
            state: request.input_root,
            continuation: request.continuation,
            observation: Some(request.observation_root),
        };
        let exit = self
            .execute(&code, input, &profile, Some(&request))
            .map_err(execution_error)?;
        Ok(port_outcome(&request, &exit))
    }
}
