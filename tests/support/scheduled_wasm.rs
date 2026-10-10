//! Real sandbox composition shared by the integration test and offline example.
use attow_network::{
    ObjectId,
    sandbox::{
        ExecutionProfile, Executor, SqliteSandboxRepository, WorkerAdmission,
        profile::MAX_HOST_CALLS,
    },
    scheduler::{GraphNode, ScheduleLimits, Scheduler, SqliteResultRepository, WorldlineGraph},
    state::{
        BranchId,
        execution::{
            EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepOutcome, StepRequest,
            WorkerBackend,
        },
    },
};
use std::{
    collections::BTreeMap,
    error::Error,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct CountedWasm {
    executor: Executor,
    calls: Arc<AtomicUsize>,
}
impl WorkerBackend for CountedWasm {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.executor.validate_request(request)
    }
    fn permits_pure_reuse(&self, request: &StepRequest) -> bool {
        self.executor.permits_pure_reuse(request)
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        WorkerBackend::run_step(&self.executor, request)
    }
}
pub struct Setup {
    pub scheduler: Scheduler,
    pub repository: Arc<SqliteResultRepository>,
    pub graph: WorldlineGraph,
    pub calls: Arc<AtomicUsize>,
}
pub fn setup(path: &Path) -> Result<Setup, Box<dyn Error>> {
    let executor = Executor::new(Arc::new(SqliteSandboxRepository::open(path)?))?;
    let code = executor.admit(&wat::parse_bytes(include_bytes!(
        "../../fixtures/original/wasm/arithmetic.wat"
    ))?)?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let request = StepRequest {
        attempt: ObjectId([1; 32]),
        code: code.id(),
        profile: profile.profile_hash,
        tenant: ObjectId([10; 32]),
        security_scope: ObjectId([11; 32]),
        input_root: executor.store_data(&[19])?,
        continuation: executor.initial_continuation(&code, &profile, &[23])?,
        observation_root: executor.store_data(&[])?,
        policy: executor.store_data(b"owned-pure-policy")?,
        fidelity: executor.store_data(b"attow-wasm-v1")?,
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
        tenant: request.tenant,
        security_scope: request.security_scope,
        policy: request.policy,
        fidelity: request.fidelity,
        code: request.code,
        profile: request.profile,
        effect_class: EffectClass::Pure,
    })?;
    let calls = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn WorkerBackend> = Arc::new(CountedWasm {
        executor,
        calls: calls.clone(),
    });
    let repository = Arc::new(SqliteResultRepository::open(path)?);
    let scheduler = Scheduler::new(
        ScheduleLimits {
            resident_workers: 2,
            queued_branches: 64,
            retained_bytes: 16 * 1024 * 1024,
        },
        BTreeMap::from([(ResourceClass::Wasm, backend)]),
        repository.clone(),
    )?;
    let graph = WorldlineGraph {
        nodes: (1..=2)
            .map(|number| GraphNode {
                id: ObjectId([number; 32]),
                branch: BranchId([number; 32]),
                request: StepRequest {
                    attempt: ObjectId([number; 32]),
                    ..request.clone()
                },
                dependencies: vec![],
            })
            .collect(),
    };
    Ok(Setup {
        scheduler,
        repository,
        graph,
        calls,
    })
}
