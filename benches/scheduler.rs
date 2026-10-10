//! Bounded mock CPU and actual WASM/SQLite measurements, reported separately.
#[path = "../tests/support/scheduled_wasm.rs"]
mod scheduled_wasm;
use attow_network::{
    codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id},
    sandbox::{Executor, SqliteSandboxRepository},
    scheduler::{
        ExecutionTrace, GraphNode, RecordedInputs, ResultRepository, ScheduleLimits, Scheduler,
        SqliteResultRepository, WorldlineGraph, replay,
    },
    state::{
        BranchId, ObjectData,
        execution::{
            EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome,
            StepRequest, WorkerBackend,
        },
    },
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io::Read,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const CPU_ROUNDS: usize = 500;
const LIMITS: ScheduleLimits = ScheduleLimits {
    resident_workers: 2,
    queued_branches: 64,
    retained_bytes: 16 * 1024 * 1024,
};
fn id(number: u64) -> ObjectId {
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&number.to_le_bytes());
    ObjectId(bytes)
}
fn output(value: serde_json::Value) {
    println!("{value}");
}
fn command(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
fn executable_hash() -> Result<String> {
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    let mut hash = blake3::Hasher::new();
    let mut chunk = [0; 65_536];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    Ok(hash.finalize().to_hex().to_string())
}
#[derive(Default)]
struct Memory {
    objects: Mutex<BTreeMap<ObjectId, ObjectData>>,
    owners: Mutex<BTreeMap<String, u64>>,
}
impl Memory {
    fn put(&self, payload: Value) -> std::result::Result<ObjectId, ExecutionError> {
        let bytes = Object {
            refs: BTreeSet::new(),
            payload,
        }
        .encode()
        .map_err(|_| ExecutionError::InvalidRequest)?;
        let id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes);
        self.objects
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .insert(
                id,
                ObjectData {
                    kind: OBJECT_KIND,
                    schema: OBJECT_SCHEMA,
                    bytes,
                },
            );
        Ok(id)
    }
}
impl ResultRepository for Memory {
    fn retain(
        &self,
        owner: &str,
        request: &StepRequest,
        outcome: &StepOutcome,
        max_bytes: u64,
    ) -> std::result::Result<u64, ExecutionError> {
        outcome
            .validate_for(request)
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let objects = self
            .objects
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        let refs: BTreeSet<_> = request
            .references()
            .into_iter()
            .chain(outcome.references())
            .collect();
        let mut bytes = (request
            .encode()
            .map_err(|_| ExecutionError::InvalidRequest)?
            .len()
            + outcome
                .encode()
                .map_err(|_| ExecutionError::InvalidRequest)?
                .len()) as u64;
        for id in refs {
            bytes += objects
                .get(&id)
                .ok_or(ExecutionError::MissingObject(id))?
                .bytes
                .len() as u64;
        }
        if bytes > max_bytes {
            return Err(ExecutionError::ResourceExhausted);
        }
        self.owners
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .insert(owner.to_owned(), bytes);
        Ok(bytes)
    }
    fn release(&self, owner: &str) -> std::result::Result<(), ExecutionError> {
        self.owners
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .remove(owner);
        Ok(())
    }
}
struct MockCpu {
    memory: Arc<Memory>,
    calls: Arc<AtomicUsize>,
}
impl WorkerBackend for MockCpu {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(&self, r: &StepRequest) -> std::result::Result<(), ExecutionError> {
        if r.resource_class != ResourceClass::Wasm || r.effect_class != EffectClass::Pure {
            return Err(ExecutionError::InvalidRequest);
        }
        r.budget
            .validate()
            .map_err(|_| ExecutionError::InvalidRequest)
    }
    fn permits_pure_reuse(&self, _: &StepRequest) -> bool {
        true
    }
    fn run_step(&self, r: StepRequest) -> std::result::Result<StepOutcome, ExecutionError> {
        self.validate_request(&r)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut digest = r.input_root.0;
        for _ in 0..CPU_ROUNDS {
            digest = *blake3::hash(std::hint::black_box(&digest)).as_bytes();
        }
        let result = self.memory.put(Value::Bytes(digest.to_vec()))?;
        Ok(StepOutcome {
            attempt: r.attempt,
            profile: r.profile,
            exit: StepExit::Completed {
                result,
                delta: r.continuation,
                observation_root: r.observation_root,
            },
        })
    }
}
enum Recording {
    Memory(Arc<Memory>),
    Sqlite(Arc<SqliteResultRepository>),
}
impl Recording {
    fn read(&self, trace: &ExecutionTrace) -> Result<RecordedInputs> {
        match self {
            Self::Memory(memory) => Ok(RecordedInputs {
                objects: memory
                    .objects
                    .lock()
                    .map_err(|_| "memory poisoned")?
                    .clone(),
            }),
            Self::Sqlite(repo) => Ok(repo.recorded_inputs(trace)?),
        }
    }
}
struct Setup {
    scheduler: Scheduler,
    graph: WorldlineGraph,
    calls: Arc<AtomicUsize>,
    recording: Recording,
    _directory: Option<tempfile::TempDir>,
}
fn graph(requests: Vec<StepRequest>, divergent: bool) -> WorldlineGraph {
    let half = requests.len() / 2;
    WorldlineGraph {
        nodes: requests
            .iter()
            .enumerate()
            .map(|(index, request)| {
                let mut request = if !divergent && index < half {
                    requests[0].clone()
                } else {
                    request.clone()
                };
                request.attempt = id(1000 + index as u64);
                GraphNode {
                    id: id(2000 + index as u64),
                    branch: BranchId(id(3000 + (index % half) as u64).0),
                    request,
                    dependencies: if index >= half {
                        vec![id(2000 + (index - half) as u64)]
                    } else {
                        vec![]
                    },
                }
            })
            .collect(),
    }
}
fn setup(real: bool, nodes: usize, divergent: bool) -> Result<Setup> {
    if real {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("scheduler.sqlite");
        let setup = scheduled_wasm::setup(&path)?;
        let data = Executor::new(Arc::new(SqliteSandboxRepository::open(&path)?))?;
        let mut requests = vec![];
        for index in 0..nodes {
            let mut request = setup.graph.nodes[0].request.clone();
            request.input_root = data.store_data(&[index as u8])?;
            requests.push(request);
        }
        Ok(Setup {
            scheduler: setup.scheduler,
            graph: graph(requests, divergent),
            calls: setup.calls,
            recording: Recording::Sqlite(setup.repository),
            _directory: Some(directory),
        })
    } else {
        let memory = Arc::new(Memory::default());
        let code = memory.put(Value::Text(format!(
            "original mock CPU: {CPU_ROUNDS} BLAKE3 rounds"
        )))?;
        let profile = memory.put(Value::Text(
            "mock profile v1; 2 resident workers; in-memory retention".into(),
        ))?;
        let empty = memory.put(Value::Null)?;
        let mut requests = vec![];
        for index in 0..nodes {
            requests.push(StepRequest {
                attempt: id(1),
                code,
                profile,
                tenant: id(2),
                security_scope: id(3),
                input_root: memory.put(Value::Unsigned(index as u64))?,
                continuation: empty,
                observation_root: empty,
                policy: empty,
                fidelity: empty,
                sample_id: None,
                resource_class: ResourceClass::Wasm,
                effect_class: EffectClass::Pure,
                budget: ResourceBudget {
                    fuel: 50_000,
                    memory_bytes: 65_536,
                    output_bytes: 1024,
                    host_calls: 10,
                },
            });
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let backend: Arc<dyn WorkerBackend> = Arc::new(MockCpu {
            memory: memory.clone(),
            calls: calls.clone(),
        });
        let scheduler = Scheduler::new(
            LIMITS,
            BTreeMap::from([(ResourceClass::Wasm, backend)]),
            memory.clone(),
        )?;
        Ok(Setup {
            scheduler,
            graph: graph(requests, divergent),
            calls,
            recording: Recording::Memory(memory),
            _directory: None,
        })
    }
}
fn percentile(sorted: &[u64], percent: usize) -> u64 {
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}
fn summary(backend: &str, scenario: &str, samples: &[u64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    output(
        json!({"record":"summary","backend":backend,"scenario":scenario,"samples":samples.len(),"method":"nearest rank; descriptive finite samples","p50_ns":percentile(&sorted,50),"p95_ns":percentile(&sorted,95),"p99_ns":percentile(&sorted,99)}),
    );
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let smoke = cfg!(debug_assertions) || args.iter().any(|a| a == "--test");
    let mut nodes = if smoke { 4 } else { 8 };
    let mut samples = if smoke { 1 } else { 3 };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--nodes" => {
                index += 1;
                nodes = args.get(index).ok_or("missing nodes")?.parse()?
            }
            "--samples" => {
                index += 1;
                samples = args.get(index).ok_or("missing samples")?.parse()?
            }
            "--test" | "--bench" => {}
            other => return Err(format!("unknown argument {other}").into()),
        }
        index += 1;
    }
    if !(4..=64).contains(&nodes) || nodes % 2 != 0 || !(1..=20).contains(&samples) {
        return Err("nodes must be even 4..64; samples 1..20".into());
    }
    output(
        json!({"record":"environment","schema":1,"project":"Attow Network","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"logical_cpus":std::thread::available_parallelism().ok().map(|n|n.get()),"cpu_identifier_environment":std::env::var("PROCESSOR_IDENTIFIER").ok(),"rustc":command("rustc",&["+1.95.0","--version","--verbose"]),"git_commit":command("git",&["rev-parse","HEAD"]),"git_dirty":command("git",&["status","--porcelain"]).map(|s|!s.is_empty()),"executable_blake3":executable_hash()?,"harness_blake3":blake3::hash(include_bytes!("scheduler.rs")).to_hex().to_string(),"lock_blake3":blake3::hash(include_bytes!("../Cargo.lock")).to_hex().to_string(),"fixture_blake3":blake3::hash(include_bytes!("../fixtures/original/wasm/arithmetic.wat")).to_hex().to_string(),"debug_assertions":cfg!(debug_assertions),"test_smoke":smoke,"nodes":nodes,"logical_branches":nodes/2,"samples":samples,"resident_limit":LIMITS.resident_workers,"queued_limit":LIMITS.queued_branches,"retained_limit_bytes":LIMITS.retained_bytes,"mock_cpu_rounds":CPU_ROUNDS,"setup_and_code_compilation_timed":false,"replay_record_loading_timed":false,"percentiles_are_latency_guarantees":false}),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    for real in [false, true] {
        let label = if real {
            "actual_wasmtime_component_sqlite_full"
        } else {
            "mock_cpu_blake3_memory_repository"
        };
        for (scenario, reuse, divergent) in [
            ("common_prefix", true, false),
            ("no_sharing", false, false),
            ("high_divergence", true, true),
        ] {
            let mut timings = vec![];
            let mut replay_timings = vec![];
            for sample in 0..samples {
                let setup = setup(real, nodes, divergent)?;
                setup.scheduler.set_reuse_enabled(reuse)?;
                let code = setup.graph.nodes[0].request.code;
                let profile = setup.graph.nodes[0].request.profile;
                let start = Instant::now();
                let (run, report) = runtime.block_on(async {
                    let run = setup.scheduler.submit(setup.graph)?;
                    Ok::<_, attow_network::scheduler::ScheduleError>((
                        run,
                        setup.scheduler.drive(run).await?,
                    ))
                })?;
                let elapsed = start.elapsed().as_nanos() as u64;
                if report.outcomes.len() != nodes
                    || !report.failures.is_empty()
                    || !report.cancelled.is_empty()
                {
                    return Err("incomplete benchmark run".into());
                }
                let calls = setup.calls.load(Ordering::SeqCst);
                let expected = if reuse && !divergent {
                    nodes / 2 + 1
                } else {
                    nodes
                };
                if calls != expected {
                    return Err(format!("unexpected worker count: {calls} != {expected}").into());
                }
                timings.push(elapsed);
                output(
                    json!({"record":"sample","backend":label,"scenario":scenario,"sample":sample,"elapsed_ns":elapsed,"nodes":nodes,"logical_branches":nodes/2,"worker_calls":calls,"peak_resident":report.metrics.peak_resident,"reused":report.metrics.reused,"cache_hits":report.metrics.cache_hits,"shared_hits":report.metrics.shared_hits,"retained_bytes":report.metrics.retained_bytes,"code_object":code.to_string(),"profile_hash":profile.to_string(),"reuse_enabled":reuse,"fresh_scheduler_cache":true}),
                );
                if scenario == "common_prefix" {
                    let recorded = setup.recording.read(&report.trace)?;
                    let before = setup.calls.load(Ordering::SeqCst);
                    let start = Instant::now();
                    let replayed = replay(&report.trace, &recorded)?;
                    let elapsed = start.elapsed().as_nanos() as u64;
                    if replayed.outcomes != report.outcomes
                        || setup.calls.load(Ordering::SeqCst) != before
                    {
                        return Err("replay mismatch or worker invocation".into());
                    }
                    replay_timings.push(elapsed);
                    output(
                        json!({"record":"sample","backend":label,"scenario":"replay","sample":sample,"elapsed_ns":elapsed,"nodes":nodes,"worker_calls":0,"recorded_objects":recorded.objects.len(),"events":replayed.events,"output_hash":replayed.output_hash.to_string()}),
                    );
                }
                setup.scheduler.forget(run)?;
            }
            summary(label, scenario, &timings);
            if !replay_timings.is_empty() {
                summary(label, "replay", &replay_timings);
            }
        }
    }
    Ok(())
}
