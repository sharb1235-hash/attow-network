//! Explicit, bounded evidence-to-guest serialization for this original demo.
use super::*;
use chronos_adapters::{AdapterStore, WorkerBinding};
use chronos_local::{
    core::persistence::{LocalStateStorage, StoredStateView},
    sandbox::SqliteSandboxRepository,
    scheduler::{SqliteResultRepository, rerun_confirmed_receipt},
};
use chronos_sandbox::{
    AdmittedCode, ExecutionProfile, Executor, StepInput, profile::MAX_HOST_CALLS,
};
use chronos_scheduler::{GraphNode, ScheduleLimits, Scheduler, WorldlineGraph};
use chronos_state::{
    FrozenBranch, Worldline,
    execution::{
        EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome,
        StepRequest, WorkerBackend,
    },
    fork,
};

struct ReceiptWasm {
    store: AdapterStore,
    executor: Arc<Executor>,
    code: AdmittedCode,
    profile: ExecutionProfile,
    binding: WorkerBinding,
    model: ObjectId,
    receipt: ObjectId,
    intent: ObjectId,
    input: ObjectId,
    continuation: ObjectId,
}
impl WorkerBackend for ReceiptWasm {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.binding.validate(request, ResourceClass::Wasm)?;
        if request.input_root != self.input
            || request.continuation != self.continuation
            || ![self.model, self.receipt].contains(&request.observation_root)
        {
            return Err(ExecutionError::InvalidRequest);
        }
        Ok(())
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.validate_request(&request)?;
        let status = if request.observation_root == self.model {
            0
        } else {
            let object = self
                .store
                .get_object(request.observation_root)
                .map_err(|_| ExecutionError::InvalidRequest)?;
            let Value::Array(fields) = object.payload else {
                return Err(ExecutionError::InvalidRequest);
            };
            if fields.len() != 10
                || fields[0] != Value::Text("attow-evidence".into())
                || fields[1] != Value::Unsigned(1)
                || fields[2] != Value::Bytes(self.intent.0.to_vec())
                || fields[8] != Value::Unsigned(3)
                || !object.refs.is_empty()
            {
                return Err(ExecutionError::InvalidRequest);
            }
            3
        };
        // The exact ledger evidence remains the request/outcome observation root.
        // Only this bounded interpretation is passed to the original WASM guest.
        let state = self
            .executor
            .store_data(&[status])
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        let exit = self
            .executor
            .run_step(
                &self.code,
                StepInput {
                    code: self.code.id(),
                    state,
                    continuation: self.continuation,
                    observation: None,
                },
                &self.profile,
            )
            .map_err(|_| ExecutionError::Trapped)?;
        let chronos_sandbox::StepExit::Done { result, delta } = exit else {
            return Err(ExecutionError::InvalidRequest);
        };
        if self
            .executor
            .read_data(result)
            .map_err(|_| ExecutionError::BackendUnavailable)?
            != [status]
        {
            return Err(ExecutionError::InvalidRequest);
        }
        Ok(StepOutcome {
            attempt: request.attempt,
            profile: request.profile,
            exit: StepExit::Completed {
                result,
                delta,
                observation_root: request.observation_root,
            },
        })
    }
}
pub(super) async fn complete(
    ws: &Workspace,
    record: &mut RunRecord,
    authority: &mut Authority,
    options: &DemoOptions,
) -> Result<(), CliError> {
    let path = ws.authority_path();
    let store = AdapterStore::open(&path)?;
    let policy = required(&record.policy)?;
    let receipt = required(&record.receipt)?;
    let intent = required(&record.intent)?;
    let response_id = derive(&record.run_id, "response-request");
    let previous = authority.promotion_record(response_id)?;
    let expected_base = crate::workspace::validate_id(&record.branches[0].state_root)?;
    if previous.is_none() && authority.current_root()?.object_id()? != expected_base {
        return Err(CliError::Conflict("managed state changed while checkout was pending; confirmation is retained, but the dependent workflow must be prepared again".into()));
    }
    if previous.is_some() && record.response_descriptor.is_none() {
        return Err(CliError::input(
            "response history lacks its durable preparation record",
        ));
    }
    let existing = if let Some(id) = &record.response_descriptor {
        Some(FrozenBranch::reopen(
            &StoredStateView(&ws.store()?),
            crate::workspace::validate_id(id)?,
            authority.domain(),
        )?)
    } else {
        None
    };
    let mut branch = if let Some(existing) = &existing {
        fork(
            authority.domain(),
            existing.base(),
            derive(&record.run_id, "response-branch").0,
        )?
    } else {
        fork(
            authority.domain(),
            &authority.current_root()?,
            derive(&record.run_id, "response-branch").0,
        )?
    };
    let executor = Arc::new(Executor::new(Arc::new(SqliteSandboxRepository::open(
        &path,
    )?))?);
    let code = executor.admit(
        &wat::parse_bytes(include_bytes!(
            "../../../../fixtures/original/wasm/arithmetic.wat"
        ))
        .map_err(CliError::infrastructure)?,
    )?;
    let profile = execution_profile()?;
    let continuation = executor.initial_continuation(&code, &profile, &[0])?;
    let input = executor.store_data(&[0])?;
    let model = store.put_object(
        &record.run_id,
        Value::Array(vec![
            Value::Text("attow-demo-response-model".into()),
            Value::Unsigned(1),
            Value::Text("Model: no provider confirmation".into()),
        ]),
        BTreeSet::new(),
    )?;
    let security_scope = store.put_object(
        &record.run_id,
        Value::Array(vec![
            Value::Text("attow-demo-scope".into()),
            Value::Unsigned(1),
            Value::Text(record.run_id.clone()),
        ]),
        BTreeSet::new(),
    )?;
    let fidelity = executor.store_data(b"attow-owned-receipt-wasm-v1:RecordedObservation")?;
    let binding = WorkerBinding {
        tenant: derive(&record.run_id, "tenant"),
        security_scope,
        policy,
        code: code.id(),
        profile: profile.profile_hash,
        fidelity,
        effect_class: EffectClass::RecordedObservation,
        budget: ResourceBudget {
            fuel: profile.fuel,
            memory_bytes: profile.memory_bytes as u64,
            output_bytes: profile.output_bytes as u64,
            host_calls: MAX_HOST_CALLS,
        },
    };
    let node = derive(&record.run_id, "response-node");
    let leaf = derive(&record.run_id, "response-leaf");
    let request = StepRequest {
        attempt: node,
        code: binding.code,
        profile: binding.profile,
        tenant: binding.tenant,
        security_scope,
        input_root: input,
        continuation,
        observation_root: model,
        policy,
        fidelity,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: binding.effect_class,
        budget: binding.budget,
    };
    let worker = ReceiptWasm {
        store: store.clone(),
        executor,
        code,
        profile,
        binding,
        model,
        receipt,
        intent,
        input,
        continuation,
    };
    let repository = Arc::new(SqliteResultRepository::open(&path)?);
    let scheduler = Scheduler::new(
        ScheduleLimits {
            resident_workers: options.workers,
            queued_branches: 4,
            retained_bytes: 8 * 1024 * 1024,
        },
        BTreeMap::from([(
            ResourceClass::Wasm,
            Arc::new(worker) as Arc<dyn WorkerBackend>,
        )]),
        repository.clone(),
    )?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![
            GraphNode {
                id: node,
                branch: branch.id(),
                request: request.clone(),
                dependencies: vec![],
            },
            GraphNode {
                id: leaf,
                branch: branch.id(),
                request: StepRequest {
                    attempt: leaf,
                    ..request.clone()
                },
                dependencies: vec![node],
            },
        ],
    })?;
    let baseline = scheduler.drive(run).await?;
    if baseline.outcomes.len() != 2 || !baseline.failures.is_empty() {
        return Err(CliError::infrastructure("model response did not complete"));
    }
    let confirmed = rerun_confirmed_receipt(authority, &scheduler, run, node, intent)
        .await
        .map_err(CliError::infrastructure)?;
    let trace = repository.store_trace(&record.run_id, &confirmed.report().trace)?;
    if !record.trace_roots.contains(&trace.to_string()) {
        record.trace_roots.push(trace.to_string());
    }
    let frozen = if let Some(existing) = existing {
        existing
    } else {
        let bindings = store.put_object(
            &record.run_id,
            Value::Array(vec![
                Value::Text("attow-demo-confirmed-bindings".into()),
                Value::Unsigned(1),
            ]),
            BTreeSet::from([trace, receipt]),
        )?;
        branch.set_worldline(Worldline {
            code: Some(request.code),
            profile: Some(request.profile),
            continuation: Some(continuation),
            observation_root: Some(receipt),
            policy: Some(policy),
            bindings: Some(bindings),
            security_scope: Some(security_scope),
            ..Worldline::default()
        })?;
        branch.put(
            format!("receipt/{}", record.run_id).into_bytes(),
            receipt.0.to_vec(),
        )?;
        branch.freeze()?
    };
    let mut cas = ws.store()?;
    let root = frozen
        .base()
        .apply(frozen.overlay())?
        .persist(&mut LocalStateStorage(&mut cas), &record.run_id)?;
    let descriptor = frozen.admit(&mut LocalStateStorage(&mut cas), &record.run_id)?;
    let decision = derive(&record.run_id, "response-decision");
    ensure_generation(authority, decision)?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: None,
    })?;
    record.response_descriptor = Some(descriptor.to_string());
    record.response_payload = Some(payload.to_string());
    record.response_decision = Some(decision.to_string());
    record.response_generation = Some(0);
    record.response_request_id = Some(derive(&record.run_id, "response-request").to_string());
    record.phase = "response_prepared".into();
    ws.save_run(record)?;
    boundary(options, DemoBoundary::AfterResponsePrepared);
    let executed = confirmed.report().metrics.executed;
    let outcome = confirmed
        .promote(
            authority,
            chronos_ledger::PromotionRequest {
                id: required(&record.response_request_id)?,
                payload,
                branch: frozen,
                decision,
                generation: 0,
            },
        )
        .await
        .map_err(CliError::infrastructure)?;
    let chronos_ledger::PromotionOutcome::LocalCommitted { seq } = outcome else {
        return Err(CliError::Conflict(
            "confirmed response conflicts with managed state".into(),
        ));
    };
    boundary(options, DemoBoundary::AfterResponseCommitted);
    if authority
        .promotion_record(response_id)?
        .ok_or_else(|| CliError::infrastructure("missing response history"))?
        .state_root
        != root
    {
        return Err(CliError::infrastructure("managed response root mismatch"));
    }
    record.managed_root = Some(root.to_string());
    record.phase = "completed".into();
    record.fidelity.insert(
        "response".into(),
        "RecordedObservation: confirmed evidence interpreted by actual WASM".into(),
    );
    event(
        record,
        serde_json::json!({"kind":"receipt_rerun","receipt":receipt.to_string(),"trace":trace.to_string(),"affected_nodes":2,"executed":executed,"sequence":seq}),
    );
    Ok(())
}
