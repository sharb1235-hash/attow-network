use super::*;
use chronos_adapters::{
    Adapter, AdapterStore, WorkerBinding,
    sql::{SqlAdapter, SqlWorker, store_query},
};
use chronos_local::{
    core::persistence::LocalStateStorage, sandbox::SqliteSandboxRepository,
    scheduler::SqliteResultRepository,
};
use chronos_sandbox::{Executor, WorkerAdmission, profile::MAX_HOST_CALLS};
use chronos_scheduler::{GraphNode, ScheduleLimits, Scheduler, WorldlineGraph};
use chronos_state::{
    Worldline,
    execution::{EffectClass, ResourceBudget, ResourceClass, StepExit, StepRequest, WorkerBackend},
    fork,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const CHECKOUT_SQL: &str = "-- Original owned Attow Network example, MIT OR Apache-2.0.\nCREATE TABLE IF NOT EXISTS offers(cost INTEGER NOT NULL,label TEXT NOT NULL);\nINSERT INTO offers SELECT 1,'owned fixture' WHERE NOT EXISTS(SELECT 1 FROM offers);";
const DATA_SQL: &str = "-- Original noncommerce Attow Network example, MIT OR Apache-2.0.\nCREATE TABLE IF NOT EXISTS sampling(interval INTEGER NOT NULL,label TEXT NOT NULL);\nINSERT INTO sampling SELECT 1,'sensor sampling interval' WHERE NOT EXISTS(SELECT 1 FROM sampling);";

pub(super) async fn prepare(
    ws: &Workspace,
    record: &mut RunRecord,
    authority: &Authority,
    options: &DemoOptions,
) -> Result<(), CliError> {
    let path = worker_path(ws, &record.run_id);
    let store = AdapterStore::open(&path)?;
    let policy = required(&record.policy)?;
    let scope = Scope {
        tenant: derive(&record.run_id, "tenant"),
        domain: ObjectId(authority.domain().0),
    };
    let authority_repo = SqliteObjectRepository::open(&ws.authority_path())?;
    let seed_roots = [policy, required(&record.payload)?];
    authority_repo.grant(scope, &seed_roots)?;
    SqliteObjectRepository::open(&path)?.import(
        scope,
        &seed_roots,
        authority_repo.export(scope, &seed_roots, 8 * 1024 * 1024)?,
        8 * 1024 * 1024,
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
    let model = store.put_object(
        &record.run_id,
        Value::Array(vec![
            Value::Text("attow-demo-model".into()),
            Value::Unsigned(1),
            Value::Text("Model: owned fixture preview; no provider confirmation".into()),
        ]),
        BTreeSet::new(),
    )?;
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
    let wasm_fidelity = executor.store_data(b"attow-original-arithmetic-pure-v1")?;
    let wasm_request = StepRequest {
        attempt: derive(&record.run_id, "wasm-template"),
        code: code.id(),
        profile: profile.profile_hash,
        tenant: scope.tenant,
        security_scope,
        input_root: executor.store_data(&[0])?,
        continuation: executor.initial_continuation(&code, &profile, &[1])?,
        observation_root: executor.store_data(&[])?,
        policy,
        fidelity: wasm_fidelity,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: profile.fuel,
            memory_bytes: profile.memory_bytes as u64,
            output_bytes: profile.output_bytes as u64,
            host_calls: MAX_HOST_CALLS,
        },
    };
    executor.register(WorkerAdmission {
        tenant: scope.tenant,
        security_scope,
        policy,
        fidelity: wasm_fidelity,
        code: code.id(),
        profile: profile.profile_hash,
        effect_class: EffectClass::Pure,
    })?;
    let fixture_path = ws
        .data_dir()
        .join(format!("fixture-{}.sqlite", record.run_id));
    let (ddl, query) = match options.scenario {
        Scenario::Checkout => (
            CHECKOUT_SQL,
            "SELECT cost,label FROM offers ORDER BY cost,label",
        ),
        Scenario::DataChange => (
            DATA_SQL,
            "SELECT interval,label FROM sampling ORDER BY interval,label",
        ),
    };
    {
        let connection = rusqlite::Connection::open(&fixture_path)?;
        connection.execute_batch(ddl)?;
    }
    let mut sql = SqlAdapter::open_owned(&fixture_path, security_scope, store.clone())?;
    let snapshot = sql.snapshot()?;
    let budget = SqlWorker::default_budget();
    let sql_binding = WorkerBinding {
        tenant: scope.tenant,
        security_scope,
        policy,
        code: SqlWorker::code_id(),
        profile: SqlWorker::profile_id(&budget)?,
        fidelity: SqlWorker::fidelity_id(),
        effect_class: EffectClass::Pure,
        budget,
    };
    let sql_worker = Arc::new(SqlWorker::new(store.clone(), sql_binding.clone())?);
    let sql_request = StepRequest {
        attempt: derive(&record.run_id, "sql-template"),
        code: sql_binding.code,
        profile: sql_binding.profile,
        tenant: scope.tenant,
        security_scope,
        input_root: snapshot,
        continuation: store_query(&store, &record.run_id, query)?,
        observation_root: model,
        policy,
        fidelity: sql_binding.fidelity,
        sample_id: None,
        resource_class: ResourceClass::Sql,
        effect_class: EffectClass::Pure,
        budget,
    };
    let base = authority.current_root()?;
    let branches = (0..5)
        .map(|i| {
            fork(
                authority.domain(),
                &base,
                derive(&record.run_id, &format!("branch:{i}")).0,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut registry: BTreeMap<ResourceClass, Arc<dyn WorkerBackend>> = BTreeMap::from([
        (
            ResourceClass::Wasm,
            executor.clone() as Arc<dyn WorkerBackend>,
        ),
        (ResourceClass::Sql, sql_worker as Arc<dyn WorkerBackend>),
    ]);
    let browser_requests = browser_requests(
        options,
        record,
        &store,
        scope,
        security_scope,
        policy,
        model,
        &mut registry,
    )?;
    let scheduler = Scheduler::new(
        ScheduleLimits {
            resident_workers: options.workers,
            queued_branches: 32,
            retained_bytes: 16 * 1024 * 1024,
        },
        registry,
        Arc::new(SqliteResultRepository::open(&path)?),
    )?;
    if options.browser {
        scheduler.set_resource_limit(ResourceClass::Browser, 1)?;
    }
    let mut nodes = Vec::new();
    for (i, branch) in branches.iter().enumerate() {
        let w = derive(&record.run_id, &format!("wasm:{i}"));
        let s = derive(&record.run_id, &format!("sql:{i}"));
        nodes.push(GraphNode {
            id: w,
            branch: branch.id(),
            request: StepRequest {
                attempt: w,
                ..wasm_request.clone()
            },
            dependencies: vec![],
        });
        nodes.push(GraphNode {
            id: s,
            branch: branch.id(),
            request: StepRequest {
                attempt: s,
                ..sql_request.clone()
            },
            dependencies: vec![w],
        });
        if let Some(request) = browser_requests.get(i) {
            nodes.push(GraphNode {
                id: request.attempt,
                branch: branch.id(),
                request: request.clone(),
                dependencies: vec![s],
            });
        }
    }
    let logical_nodes = nodes.len();
    let run = scheduler.submit(WorldlineGraph { nodes })?;
    let report = scheduler.drive(run).await?;
    if !report.failures.is_empty() || !report.cancelled.is_empty() || !report.pending.is_empty() {
        return Err(CliError::infrastructure("incomplete demo graph"));
    }
    let trace = save_trace(&path, &record.run_id, &report.trace)?;
    record.trace_roots.push(trace.to_string());
    record.summary = serde_json::json!({"logical_nodes":logical_nodes,"executed":report.metrics.executed,"reused":report.metrics.reused,"peak_resident":report.metrics.peak_resident});
    record.fidelity.insert("wasm".into(), "Pure".into());
    record
        .fidelity
        .insert("sql".into(), "ImmutableSnapshot".into());
    record
        .fidelity
        .insert("checkout_preview".into(), "Model".into());
    for (i, mut branch) in branches.into_iter().enumerate() {
        let mut refs = BTreeSet::from([trace, snapshot, model]);
        let mut results = Vec::new();
        for resource in ["wasm", "sql", "browser"] {
            if let Some(outcome) = report
                .outcomes
                .get(&derive(&record.run_id, &format!("{resource}:{i}")))
            {
                let StepExit::Completed {
                    result,
                    observation_root,
                    ..
                } = outcome.exit
                else {
                    return Err(CliError::infrastructure("suspended demo"));
                };
                refs.extend([result, observation_root]);
                results.push(Value::Bytes(result.0.to_vec()));
                if resource == "wasm" && executor.read_data(result)? != [1] {
                    return Err(CliError::infrastructure("unexpected WASM result"));
                }
            }
        }
        let observed = browser_requests
            .get(i)
            .and_then(|r| report.outcomes.get(&r.attempt))
            .map(|o| match o.exit {
                StepExit::Completed {
                    observation_root, ..
                }
                | StepExit::Suspended {
                    observation_root, ..
                } => observation_root,
            })
            .unwrap_or(model);
        let bindings = store.put_object(
            &record.run_id,
            Value::Array(vec![
                Value::Text("attow-demo-bindings".into()),
                Value::Unsigned(1),
                Value::Array(results),
            ]),
            refs,
        )?;
        branch.set_worldline(Worldline {
            code: Some(code.id()),
            profile: Some(profile.profile_hash),
            continuation: Some(wasm_request.continuation),
            observation_root: Some(observed),
            policy: Some(policy),
            bindings: Some(bindings),
            security_scope: Some(security_scope),
            ..Worldline::default()
        })?;
        branch.put(
            format!("{}/{}", record.scenario, record.run_id).into_bytes(),
            format!("alternative-{}", i + 1).into_bytes(),
        )?;
        let frozen = branch.freeze()?;
        let mut cas = chronos_store::Store::open(&path)?;
        let state_root = frozen
            .base()
            .apply(frozen.overlay())?
            .persist(&mut LocalStateStorage(&mut cas), &record.run_id)?;
        let descriptor = frozen.admit(&mut LocalStateStorage(&mut cas), &record.run_id)?;
        record.branches.push(crate::BranchRecord {
            branch_id: ObjectId(frozen.id().0).to_string(),
            descriptor: descriptor.to_string(),
            label: format!("alternative-{}", i + 1),
            state_root: state_root.to_string(),
            observation_root: Some(observed.to_string()),
        });
    }
    record.selected_branch = Some(record.branches[0].branch_id.clone());
    event(
        record,
        serde_json::json!({"kind":"graph_completed","trace":trace.to_string(),"logical_nodes":logical_nodes,"executed":report.metrics.executed,"reused":report.metrics.reused}),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn browser_requests(
    options: &DemoOptions,
    record: &mut RunRecord,
    store: &AdapterStore,
    scope: Scope,
    security_scope: ObjectId,
    policy: ObjectId,
    model: ObjectId,
    registry: &mut BTreeMap<ResourceClass, Arc<dyn WorkerBackend>>,
) -> Result<Vec<StepRequest>, CliError> {
    let requests = Vec::new();
    #[cfg(feature = "browser")]
    if options.browser {
        use chronos_adapters::browser::{BROWSER_BUDGET, BrowserAdapter, StorageKind};
        let mut requests = requests;
        let ids = BrowserAdapter::identities(store, &record.run_id)?;
        let binding = WorkerBinding {
            tenant: scope.tenant,
            security_scope,
            policy,
            code: ids.code,
            profile: ids.profile,
            fidelity: ids.fidelity,
            effect_class: EffectClass::FreshObservation,
            budget: BROWSER_BUDGET,
        };
        let mut browser = BrowserAdapter::launch(store.clone(), binding, browser_config()?)?;
        for i in 0..5 {
            browser.set_storage(
                StorageKind::LocalStorage,
                "alternative",
                &format!("owned-alternative-{}", i + 1),
            )?;
            requests.push(StepRequest {
                attempt: derive(&record.run_id, &format!("browser:{i}")),
                code: ids.code,
                profile: ids.profile,
                tenant: scope.tenant,
                security_scope,
                input_root: browser.snapshot()?,
                continuation: BrowserAdapter::store_request(store, &record.run_id, "snapshot")?,
                observation_root: model,
                policy,
                fidelity: ids.fidelity,
                sample_id: None,
                resource_class: ResourceClass::Browser,
                effect_class: EffectClass::FreshObservation,
                budget: BROWSER_BUDGET,
            });
        }
        registry.insert(ResourceClass::Browser, Arc::new(browser));
        record
            .fidelity
            .insert("browser".into(), "OwnedEnvironment".into());
        return Ok(requests);
    }
    let _ = (
        options,
        record,
        store,
        scope,
        security_scope,
        policy,
        model,
        registry,
    );
    Ok(requests)
}

#[cfg(feature = "browser")]
fn browser_config() -> Result<chronos_adapters::browser::BrowserConfig, CliError> {
    let node = std::env::var_os("ATTOW_NODE")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::infrastructure("missing ATTOW_NODE"))?;
    let sidecar = std::env::var_os("ATTOW_BROWSER_SIDECAR")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::infrastructure("missing ATTOW_BROWSER_SIDECAR"))?;
    if !node.is_absolute() || !sidecar.is_absolute() {
        return Err(CliError::input("browser paths must be absolute"));
    }
    Ok(chronos_adapters::browser::BrowserConfig::new(node, sidecar))
}
