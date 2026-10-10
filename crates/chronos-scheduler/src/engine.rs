use crate::budget::{CACHE_METADATA_BYTES, EVENT_BYTES, MAX_RUNS, MAX_TRACE_EVENTS};
use crate::queue::{FairQueue, Task};
use crate::*;
use chronos_state::execution::StepExit;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use tokio::runtime::Handle;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

const MAX_TOTAL_NODES: usize = 16_384;
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Clone)]
enum Status {
    Pending,
    Ready,
    Running,
    Sharing,
    Complete(StepOutcome),
    Failed(ExecutionError),
    Cancelled,
}
struct Node {
    graph: GraphNode,
    status: Status,
}
struct Owned {
    owner: String,
    bytes: u64,
}
struct Run {
    nodes: BTreeMap<ObjectId, Node>,
    trace: ExecutionTrace,
    revision: u64,
    replacement_cost: usize,
    remaining_replacement_work: usize,
    event_capacity: usize,
    metadata: u64,
    owned: BTreeMap<String, Owned>,
    metrics: SchedulerMetrics,
    forgetting: bool,
}
impl Run {
    fn event(&mut self, event: SemanticEvent) {
        self.trace.events.push(event);
    }
    fn quiescent(&self) -> bool {
        self.nodes.values().all(|node| match node.status {
            Status::Ready | Status::Running | Status::Sharing => false,
            Status::Pending => {
                let failed = node.graph.dependencies.iter().any(|id| {
                    self.nodes.get(id).is_some_and(|node| {
                        matches!(node.status, Status::Failed(_) | Status::Cancelled)
                    })
                });
                let ready = node.graph.dependencies.iter().all(|id| {
                    self.nodes.get(id).is_some_and(|node| {
                        matches!(
                            node.status,
                            Status::Complete(StepOutcome {
                                exit: StepExit::Completed { .. },
                                ..
                            })
                        )
                    })
                });
                !failed && !ready
            }
            _ => true,
        })
    }
    fn report(&self, state: &State) -> RunReport {
        let mut report = RunReport {
            outcomes: BTreeMap::new(),
            failures: BTreeMap::new(),
            cancelled: BTreeSet::new(),
            pending: BTreeSet::new(),
            trace: self.trace.clone(),
            metrics: self.metrics.clone(),
        };
        for (id, node) in &self.nodes {
            match &node.status {
                Status::Complete(outcome) => {
                    report.outcomes.insert(*id, outcome.clone());
                }
                Status::Failed(error) => {
                    report.failures.insert(*id, *error);
                }
                Status::Cancelled => {
                    report.cancelled.insert(*id);
                }
                _ => {
                    report.pending.insert(*id);
                }
            }
        }
        report.metrics.peak_resident = state.metrics.peak_resident;
        report.metrics.retained_bytes = state.retained;
        report
    }
}
struct Flight {
    request: StepRequest,
    key: Option<EvaluationKey>,
    consumers: BTreeSet<Task>,
    _global: OwnedSemaphorePermit,
    _class: OwnedSemaphorePermit,
}
struct Completion {
    flight: u64,
    result: Result<StepOutcome, ExecutionError>,
}
struct Cached {
    outcome: StepOutcome,
    owned: Owned,
}
struct State {
    runs: BTreeMap<RunId, Run>,
    ready: FairQueue,
    flights: BTreeMap<u64, Flight>,
    shared: BTreeMap<EvaluationKey, u64>,
    cache: BTreeMap<EvaluationKey, Cached>,
    cache_order: VecDeque<EvaluationKey>,
    classes: BTreeMap<ResourceClass, Arc<Semaphore>>,
    completed: VecDeque<Completion>,
    next_run: u64,
    next_flight: u64,
    next_cache: u64,
    total_nodes: usize,
    queued: usize,
    retained: u64,
    repository_fault: Option<ExecutionError>,
    failed_cleanup: BTreeSet<String>,
    pumping: bool,
    reuse: bool,
    metrics: SchedulerMetrics,
}
struct Inner {
    limits: ScheduleLimits,
    backends: BackendRegistry,
    repository: Arc<dyn ResultRepository>,
    instance: ObjectId,
    global: Arc<Semaphore>,
    state: Mutex<State>,
    wake: Condvar,
    changes: watch::Sender<u64>,
}
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

impl Scheduler {
    pub fn new(
        limits: ScheduleLimits,
        backends: BackendRegistry,
        repository: Arc<dyn ResultRepository>,
    ) -> Result<Self, ScheduleError> {
        budget::validate(limits)?;
        if backends.is_empty()
            || backends
                .iter()
                .any(|(class, backend)| *class != backend.resource_class())
        {
            return Err(ScheduleError::InvalidLimits);
        }
        let mut random = [0; 32];
        getrandom::fill(&mut random).map_err(|_| ScheduleError::Random)?;
        let (changes, _) = watch::channel(0);
        let classes = backends
            .keys()
            .map(|class| (*class, Arc::new(Semaphore::new(limits.resident_workers))))
            .collect();
        Ok(Self {
            inner: Arc::new(Inner {
                limits,
                backends,
                repository,
                instance: ObjectId(random),
                global: Arc::new(Semaphore::new(limits.resident_workers)),
                changes,
                wake: Condvar::new(),
                state: Mutex::new(State {
                    runs: BTreeMap::new(),
                    ready: FairQueue::default(),
                    flights: BTreeMap::new(),
                    shared: BTreeMap::new(),
                    cache: BTreeMap::new(),
                    cache_order: VecDeque::new(),
                    classes,
                    completed: VecDeque::new(),
                    next_run: 1,
                    next_flight: 1,
                    next_cache: 1,
                    total_nodes: 0,
                    queued: 0,
                    retained: 0,
                    repository_fault: None,
                    failed_cleanup: BTreeSet::new(),
                    pumping: false,
                    reuse: true,
                    metrics: SchedulerMetrics::default(),
                }),
            }),
        })
    }
    pub fn submit(&self, graph: WorldlineGraph) -> Result<RunId, ScheduleError> {
        graph.validate()?;
        if graph.nodes.iter().any(|node| {
            !self
                .inner
                .backends
                .contains_key(&node.request.resource_class)
        }) {
            return Err(ScheduleError::Backend(ExecutionError::UnsupportedResource));
        }
        let capacity = graph
            .nodes
            .len()
            .checked_mul(2)
            .ok_or(ScheduleError::Limit("trace"))?;
        if capacity > MAX_TRACE_EVENTS {
            return Err(ScheduleError::Limit("trace"));
        }
        let metadata = graph
            .retained_bytes()?
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(capacity as u64 * EVENT_BYTES + 1024))
            .ok_or(ScheduleError::Limit("metadata"))?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        if state.runs.len() >= MAX_RUNS
            || state.total_nodes + graph.nodes.len() > MAX_TOTAL_NODES
            || state.queued + graph.nodes.len() > self.inner.limits.queued_branches
        {
            return Err(ScheduleError::Limit("queued branches"));
        }
        self.inner.make_space(&mut state, metadata, None)?;
        let id = RunId(state.next_run);
        state.next_run = state
            .next_run
            .checked_add(1)
            .ok_or(ScheduleError::Limit("run identity"))?;
        let count = graph.nodes.len();
        let replacement_cost = count
            + graph
                .nodes
                .iter()
                .map(|node| node.dependencies.len())
                .sum::<usize>();
        let nodes = graph
            .nodes
            .iter()
            .cloned()
            .map(|graph| {
                (
                    graph.id,
                    Node {
                        graph,
                        status: Status::Pending,
                    },
                )
            })
            .collect();
        state.runs.insert(
            id,
            Run {
                nodes,
                trace: ExecutionTrace {
                    graph,
                    events: Vec::with_capacity(capacity),
                },
                revision: 0,
                replacement_cost,
                remaining_replacement_work: MAX_REPLACEMENT_WORK,
                event_capacity: capacity,
                metadata,
                owned: BTreeMap::new(),
                metrics: SchedulerMetrics {
                    submitted_nodes: count,
                    ..SchedulerMetrics::default()
                },
                forgetting: false,
            },
        );
        state.total_nodes += count;
        state.queued += count;
        state.retained += metadata;
        state.metrics.submitted_nodes += count;
        self.inner.wake.notify_all();
        Ok(id)
    }
    pub fn set_reuse_enabled(&self, enabled: bool) -> Result<(), ScheduleError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        if !state.runs.is_empty() || !state.flights.is_empty() {
            return Err(ScheduleError::Busy);
        }
        while self.inner.evict(&mut state, None)? {}
        state.reuse = enabled;
        Ok(())
    }
    pub fn set_resource_limit(
        &self,
        class: ResourceClass,
        cap: usize,
    ) -> Result<(), ScheduleError> {
        if cap == 0 || cap > self.inner.limits.resident_workers {
            return Err(ScheduleError::InvalidLimits);
        }
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        if !state.runs.is_empty() || !state.flights.is_empty() {
            return Err(ScheduleError::Busy);
        }
        if !self.inner.backends.contains_key(&class) {
            return Err(ScheduleError::Backend(ExecutionError::UnsupportedResource));
        }
        state.classes.insert(class, Arc::new(Semaphore::new(cap)));
        Ok(())
    }
    fn start(&self) -> Result<(), ScheduleError> {
        let handle = Handle::try_current().map_err(|_| ScheduleError::RuntimeUnavailable)?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        if let Some(error) = state.repository_fault {
            return Err(ScheduleError::Repository(error));
        }
        if !state.pumping {
            state.pumping = true;
            let inner = self.inner.clone();
            let worker_runtime = handle.clone();
            handle.spawn_blocking(move || inner.pump(worker_runtime));
        }
        self.inner.wake.notify_all();
        Ok(())
    }
    pub async fn drive(&self, run: RunId) -> Result<RunReport, ScheduleError> {
        let mut changed = self.inner.changes.subscribe();
        self.start()?;
        loop {
            {
                let state = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| ScheduleError::Poisoned)?;
                if let Some(error) = state.repository_fault {
                    return Err(ScheduleError::Repository(error));
                }
                let run = state.runs.get(&run).ok_or(ScheduleError::UnknownRun)?;
                if run.forgetting {
                    return Err(ScheduleError::UnknownRun);
                }
                if run.quiescent() {
                    return Ok(run.report(&state));
                }
            }
            changed
                .changed()
                .await
                .map_err(|_| ScheduleError::RuntimeUnavailable)?;
        }
    }
    pub async fn cancel(&self, id: RunId) -> Result<CancelReport, ScheduleError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        let run = state.runs.get_mut(&id).ok_or(ScheduleError::UnknownRun)?;
        if run.forgetting {
            return Err(ScheduleError::UnknownRun);
        }
        let mut cancelled = Vec::new();
        let mut queued = 0;
        let mut in_flight = 0;
        for (node_id, node) in &mut run.nodes {
            match node.status {
                Status::Pending | Status::Ready => queued += 1,
                Status::Sharing => {
                    queued += 1;
                    in_flight += 1;
                }
                Status::Running => in_flight += 1,
                _ => continue,
            }
            cancelled.push(*node_id);
            node.status = Status::Cancelled;
        }
        for node in &cancelled {
            run.event(SemanticEvent::Cancelled { node: *node });
        }
        run.metrics.cancelled += cancelled.len();
        for flight in state.flights.values_mut() {
            flight.consumers.retain(|task| task.run != id);
        }
        state.ready.remove_run(id);
        state.queued -= queued;
        state.metrics.cancelled += cancelled.len();
        self.inner.changed();
        self.inner.wake.notify_all();
        Ok(CancelReport {
            cancelled: cancelled.len(),
            in_flight,
            effects_revoked: 0,
        })
    }
    pub fn replace_observation(
        &self,
        id: RunId,
        node: ObjectId,
        observation_root: ObjectId,
    ) -> Result<Vec<ObjectId>, ScheduleError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        let run = state.runs.get(&id).ok_or(ScheduleError::UnknownRun)?;
        if run.forgetting {
            return Err(ScheduleError::UnknownRun);
        }
        if !run.quiescent() {
            return Err(ScheduleError::Busy);
        }
        let remaining_replacement_work = run
            .remaining_replacement_work
            .checked_sub(run.replacement_cost)
            .ok_or(ScheduleError::Limit("replacement work"))?;
        let graph = WorldlineGraph {
            nodes: run.nodes.values().map(|node| node.graph.clone()).collect(),
        };
        let affected = invalidate::descendants(&graph, node)?;
        let revision = run
            .revision
            .checked_add(1)
            .ok_or(ScheduleError::Limit("observation revisions"))?;
        let added_events = affected.len() * 2 + 1;
        if run.event_capacity + added_events > MAX_TRACE_EVENTS {
            return Err(ScheduleError::Limit("trace"));
        }
        let new_queued = affected
            .iter()
            .filter(|node| {
                !matches!(
                    run.nodes.get(node).map(|node| &node.status),
                    Some(Status::Pending)
                )
            })
            .count();
        if state.queued + new_queued > self.inner.limits.queued_branches {
            return Err(ScheduleError::Limit("queued branches"));
        }
        let added_bytes = added_events as u64 * EVENT_BYTES;
        self.inner.make_space(&mut state, added_bytes, None)?;
        // Earlier outcomes remain part of the trace. Their distinct revision
        // owners and byte charges survive replacement until explicit forget.
        let run = state.runs.get_mut(&id).ok_or(ScheduleError::UnknownRun)?;
        run.revision = revision;
        run.remaining_replacement_work = remaining_replacement_work;
        run.event_capacity += added_events;
        run.metadata += added_bytes;
        run.event(SemanticEvent::ObservationReplaced {
            node,
            observation_root,
            revision,
            invalidated: affected.clone(),
        });
        for id in &affected {
            let node = run.nodes.get_mut(id).ok_or(ScheduleError::UnknownNode)?;
            node.graph.request.attempt = invalidate::replacement_attempt(
                node.graph.request.attempt,
                observation_root,
                revision,
            )?;
            node.graph.request.observation_root = observation_root;
            node.status = Status::Pending;
        }
        state.retained += added_bytes;
        state.queued += new_queued;
        self.inner.changed();
        self.inner.wake.notify_all();
        Ok(affected)
    }
    pub fn forget(&self, id: RunId) -> Result<(), ScheduleError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        let run = state.runs.get_mut(&id).ok_or(ScheduleError::UnknownRun)?;
        if !run.quiescent() {
            return Err(ScheduleError::Busy);
        }
        run.forgetting = true;
        let owners: Vec<_> = run.owned.keys().cloned().collect();
        for owner in owners {
            self.inner
                .repository
                .release(&owner)
                .map_err(ScheduleError::Repository)?;
            if let Some(owned) = state
                .runs
                .get_mut(&id)
                .and_then(|run| run.owned.remove(&owner))
            {
                state.retained -= owned.bytes;
            }
        }
        let run = state.runs.remove(&id).ok_or(ScheduleError::UnknownRun)?;
        state.queued -= run
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.status,
                    Status::Pending | Status::Ready | Status::Sharing
                )
            })
            .count();
        state.total_nodes -= run.nodes.len();
        state.retained -= run.metadata;
        state.ready.remove_run(id);
        self.inner.changed();
        self.inner.wake.notify_all();
        Ok(())
    }
    pub fn clear_cache(&self) -> Result<(), ScheduleError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        while let Some(owner) = state.failed_cleanup.first().cloned() {
            self.inner
                .repository
                .release(&owner)
                .map_err(ScheduleError::Repository)?;
            state.failed_cleanup.remove(&owner);
        }
        while self.inner.evict(&mut state, None)? {}
        state.repository_fault = None;
        Ok(())
    }
    pub fn metrics(&self) -> Result<SchedulerMetrics, ScheduleError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ScheduleError::Poisoned)?;
        let mut metrics = state.metrics.clone();
        metrics.retained_bytes = state.retained;
        Ok(metrics)
    }
}

impl Inner {
    fn changed(&self) {
        self.changes
            .send_modify(|version| *version = version.wrapping_add(1));
    }
    fn evict(
        &self,
        state: &mut State,
        protected: Option<&EvaluationKey>,
    ) -> Result<bool, ScheduleError> {
        let selected = state
            .cache_order
            .iter()
            .position(|key| protected != Some(key));
        let Some(index) = selected else {
            return Ok(false);
        };
        let key = state
            .cache_order
            .get(index)
            .cloned()
            .ok_or(ScheduleError::Poisoned)?;
        let cached = state.cache.get(&key).ok_or(ScheduleError::Poisoned)?;
        self.repository
            .release(&cached.owned.owner)
            .map_err(ScheduleError::Repository)?;
        let cached = state.cache.remove(&key).ok_or(ScheduleError::Poisoned)?;
        state.cache_order.remove(index);
        state.retained -= cached.owned.bytes + CACHE_METADATA_BYTES;
        Ok(true)
    }
    fn make_space(
        &self,
        state: &mut State,
        bytes: u64,
        protected: Option<&EvaluationKey>,
    ) -> Result<(), ScheduleError> {
        if let Some(error) = state.repository_fault {
            return Err(ScheduleError::Repository(error));
        }
        while bytes > self.limits.retained_bytes.saturating_sub(state.retained) {
            if !self.evict(state, protected)? {
                return Err(ScheduleError::Limit("retained bytes"));
            }
        }
        Ok(())
    }
    fn fail(&self, state: &mut State, task: Task, error: ExecutionError) {
        if let Some(run) = state.runs.get_mut(&task.run) {
            if let Some(node) = run.nodes.get_mut(&task.node) {
                node.status = Status::Failed(error);
            }
            run.event(SemanticEvent::Failed {
                node: task.node,
                error,
            });
        }
    }
    fn retain(
        &self,
        state: &mut State,
        task: Task,
        mut outcome: StepOutcome,
        protected: Option<&EvaluationKey>,
    ) -> Result<(), ExecutionError> {
        if let Some(error) = state.repository_fault {
            return Err(error);
        }
        let run = state
            .runs
            .get(&task.run)
            .ok_or(ExecutionError::Interrupted)?;
        let request = &run
            .nodes
            .get(&task.node)
            .ok_or(ExecutionError::Interrupted)?
            .graph
            .request;
        outcome.attempt = request.attempt;
        outcome.profile = request.profile;
        outcome
            .validate_for(request)
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let owner = format!(
            "scheduler:{}:run:{}:node:{}:revision:{}",
            self.instance, task.run.0, task.node, run.revision
        );
        let request = request.clone();
        let bytes = loop {
            let available = self.limits.retained_bytes.saturating_sub(state.retained);
            match self
                .repository
                .retain(&owner, &request, &outcome, available)
            {
                Ok(bytes) if bytes <= available => break bytes,
                Ok(_) => {
                    self.reject_oversized_retention(state, owner);
                    return Err(ExecutionError::ResourceExhausted);
                }
                Err(ExecutionError::ResourceExhausted) => {
                    if !self
                        .evict(state, protected)
                        .map_err(|_| ExecutionError::BackendUnavailable)?
                    {
                        return Err(ExecutionError::ResourceExhausted);
                    }
                }
                Err(error) => return Err(error),
            }
        };
        state.retained += bytes;
        let run = state
            .runs
            .get_mut(&task.run)
            .ok_or(ExecutionError::Interrupted)?;
        run.owned.insert(owner.clone(), Owned { owner, bytes });
        let node = run
            .nodes
            .get_mut(&task.node)
            .ok_or(ExecutionError::Interrupted)?;
        node.status = Status::Complete(outcome.clone());
        run.event(SemanticEvent::Completed {
            node: task.node,
            outcome,
        });
        Ok(())
    }
    fn prepare_ready(&self, state: &mut State) -> bool {
        let mut ready = Vec::new();
        let mut cancelled = Vec::new();
        for (run_id, run) in &state.runs {
            if run.forgetting {
                continue;
            }
            for (id, node) in &run.nodes {
                if !matches!(node.status, Status::Pending) {
                    continue;
                }
                if node.graph.dependencies.iter().any(|id| {
                    run.nodes.get(id).is_some_and(|node| {
                        matches!(node.status, Status::Failed(_) | Status::Cancelled)
                    })
                }) {
                    cancelled.push(Task {
                        run: *run_id,
                        node: *id,
                    });
                } else if node.graph.dependencies.iter().all(|id| {
                    run.nodes.get(id).is_some_and(|node| {
                        matches!(
                            node.status,
                            Status::Complete(StepOutcome {
                                exit: StepExit::Completed { .. },
                                ..
                            })
                        )
                    })
                }) {
                    ready.push((
                        node.graph.request.tenant,
                        Task {
                            run: *run_id,
                            node: *id,
                        },
                    ));
                }
            }
        }
        let progress = !ready.is_empty() || !cancelled.is_empty();
        for (tenant, task) in ready {
            if let Some(node) = state
                .runs
                .get_mut(&task.run)
                .and_then(|run| run.nodes.get_mut(&task.node))
            {
                node.status = Status::Ready;
            }
            state.ready.push(tenant, task);
        }
        for task in cancelled {
            if let Some(run) = state.runs.get_mut(&task.run) {
                if let Some(node) = run.nodes.get_mut(&task.node) {
                    node.status = Status::Cancelled;
                }
                run.event(SemanticEvent::Cancelled { node: task.node });
                run.metrics.cancelled += 1;
            }
            state.queued -= 1;
            state.metrics.cancelled += 1;
        }
        progress
    }
    fn cache_result(
        &self,
        state: &mut State,
        key: EvaluationKey,
        request: &StepRequest,
        outcome: &StepOutcome,
    ) {
        if state.repository_fault.is_some()
            || state.cache.contains_key(&key)
            || !matches!(outcome.exit, StepExit::Completed { .. })
        {
            return;
        }
        if state.cache.len() >= MAX_CACHE_ENTRIES && !matches!(self.evict(state, None), Ok(true)) {
            return;
        }
        let available = self.limits.retained_bytes.saturating_sub(state.retained);
        if available <= CACHE_METADATA_BYTES {
            return;
        }
        let Some(next) = state.next_cache.checked_add(1) else {
            return;
        };
        let owner = format!("scheduler:{}:cache:{}", self.instance, state.next_cache);
        if let Ok(bytes) =
            self.repository
                .retain(&owner, request, outcome, available - CACHE_METADATA_BYTES)
        {
            if bytes > available - CACHE_METADATA_BYTES {
                self.reject_oversized_retention(state, owner);
                return;
            }
            state.next_cache = next;
            state.retained += bytes + CACHE_METADATA_BYTES;
            state.cache_order.push_back(key.clone());
            state.cache.insert(
                key,
                Cached {
                    outcome: outcome.clone(),
                    owned: Owned { owner, bytes },
                },
            );
        }
    }
    fn reject_oversized_retention(&self, state: &mut State, owner: String) {
        // A trusted port has already violated its pre-publication byte bound.
        // Stop further admission. Keep a failed release visible for explicit
        // clear_cache retry; never silently abandon its durable pins.
        state.repository_fault = Some(ExecutionError::ResourceExhausted);
        if let Err(error) = self.repository.release(&owner) {
            state.failed_cleanup.insert(owner);
            state.repository_fault = Some(error);
        }
    }
    fn complete(&self, state: &mut State) -> bool {
        let progress = !state.completed.is_empty();
        while let Some(completion) = state.completed.pop_front() {
            let Some(flight) = state.flights.remove(&completion.flight) else {
                continue;
            };
            if let Some(key) = &flight.key {
                state.shared.remove(key);
            }
            let result = completion.result.and_then(|outcome| {
                outcome
                    .validate_for(&flight.request)
                    .map_err(|_| ExecutionError::InvalidRequest)?;
                if flight.request.effect_class == EffectClass::Pure {
                    let observed = match outcome.exit {
                        StepExit::Completed {
                            observation_root, ..
                        }
                        | StepExit::Suspended {
                            observation_root, ..
                        } => observation_root,
                    };
                    if observed != flight.request.observation_root {
                        return Err(ExecutionError::InvalidRequest);
                    }
                }
                Ok(outcome)
            });
            let completed = result
                .as_ref()
                .is_ok_and(|outcome| matches!(outcome.exit, StepExit::Completed { .. }));
            let mut retained = false;
            for task in &flight.consumers {
                let sharing = state
                    .runs
                    .get(&task.run)
                    .and_then(|run| run.nodes.get(&task.node))
                    .is_some_and(|node| matches!(node.status, Status::Sharing));
                if sharing
                    && result
                        .as_ref()
                        .is_ok_and(|outcome| matches!(outcome.exit, StepExit::Suspended { .. }))
                {
                    if let Some(run) = state.runs.get_mut(&task.run)
                        && let Some(node) = run.nodes.get_mut(&task.node)
                    {
                        node.status = Status::Ready;
                        state.ready.push(node.graph.request.tenant, *task);
                    }
                    continue;
                }
                if sharing {
                    state.queued -= 1;
                    if completed && let Some(run) = state.runs.get_mut(&task.run) {
                        if let Some(node) = run.nodes.get(&task.node) {
                            run.event(SemanticEvent::Scheduled {
                                node: task.node,
                                request: node.graph.request.clone(),
                                source: EvaluationSource::Shared,
                            });
                        }
                        run.metrics.shared_hits += 1;
                        run.metrics.reused += 1;
                        state.metrics.shared_hits += 1;
                        state.metrics.reused += 1;
                    }
                }
                match &result {
                    Ok(outcome) => match self.retain(state, *task, outcome.clone(), None) {
                        Ok(()) => retained = true,
                        Err(error) => self.fail(state, *task, error),
                    },
                    Err(error) => self.fail(state, *task, *error),
                }
            }
            if retained
                && completed
                && let (Some(key), Ok(outcome)) = (flight.key, result)
            {
                self.cache_result(state, key, &flight.request, &outcome);
            }
            // Both semaphore permits are held until this flight is removed and
            // its actual blocking attempt has returned, even after cancellation.
        }
        progress
    }
    fn dispatch(self: &Arc<Self>, state: &mut State, runtime: &Handle) -> bool {
        let mut progress = false;
        let count = state.ready.len();
        let mut deferred = Vec::new();
        for _ in 0..count {
            let Some((tenant, task)) = state.ready.pop() else {
                break;
            };
            let Some(node) = state
                .runs
                .get(&task.run)
                .and_then(|run| run.nodes.get(&task.node))
            else {
                continue;
            };
            if !matches!(node.status, Status::Ready) {
                continue;
            }
            if let Some(error) = state.repository_fault {
                state.queued -= 1;
                self.fail(state, task, error);
                progress = true;
                continue;
            }
            let request = node.graph.request.clone();
            let Some(backend) = self.backends.get(&request.resource_class).cloned() else {
                state.queued -= 1;
                self.fail(state, task, ExecutionError::UnsupportedResource);
                progress = true;
                continue;
            };
            let admitted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                backend.validate_request(&request)?;
                Ok::<_, ExecutionError>(
                    request.effect_class == EffectClass::Pure
                        && backend.permits_pure_reuse(&request),
                )
            }))
            .unwrap_or(Err(ExecutionError::BackendUnavailable));
            let pure = match admitted {
                Ok(pure) => pure && state.reuse,
                Err(error) => {
                    state.queued -= 1;
                    self.fail(state, task, error);
                    progress = true;
                    continue;
                }
            };
            let key = if pure {
                match EvaluationKey::from_request(&request) {
                    Ok(key) => Some(key),
                    Err(_) => {
                        state.queued -= 1;
                        self.fail(state, task, ExecutionError::InvalidRequest);
                        progress = true;
                        continue;
                    }
                }
            } else {
                None
            };
            if let Some(key) = &key {
                if let Some(outcome) = state.cache.get(key).map(|cached| cached.outcome.clone()) {
                    state.queued -= 1;
                    if let Some(run) = state.runs.get_mut(&task.run) {
                        run.event(SemanticEvent::Scheduled {
                            node: task.node,
                            request: request.clone(),
                            source: EvaluationSource::Cache,
                        });
                        run.metrics.cache_hits += 1;
                        run.metrics.reused += 1;
                    }
                    state.metrics.cache_hits += 1;
                    state.metrics.reused += 1;
                    if let Err(error) = self.retain(state, task, outcome, Some(key)) {
                        self.fail(state, task, error);
                    }
                    progress = true;
                    continue;
                }
                if let Some(flight_id) = state.shared.get(key).copied() {
                    if let Some(flight) = state.flights.get_mut(&flight_id) {
                        flight.consumers.insert(task);
                    }
                    if let Some(node) = state
                        .runs
                        .get_mut(&task.run)
                        .and_then(|run| run.nodes.get_mut(&task.node))
                    {
                        node.status = Status::Sharing;
                    }
                    progress = true;
                    continue;
                }
            }
            let Ok(global) = self.global.clone().try_acquire_owned() else {
                deferred.push((tenant, task));
                continue;
            };
            let Some(class) = state.classes.get(&request.resource_class).cloned() else {
                deferred.push((tenant, task));
                continue;
            };
            let Ok(class) = class.try_acquire_owned() else {
                deferred.push((tenant, task));
                continue;
            };
            let flight_id = state.next_flight;
            let Some(next) = state.next_flight.checked_add(1) else {
                state.queued -= 1;
                self.fail(state, task, ExecutionError::ResourceExhausted);
                progress = true;
                continue;
            };
            state.next_flight = next;
            state.queued -= 1;
            if let Some(run) = state.runs.get_mut(&task.run) {
                if let Some(node) = run.nodes.get_mut(&task.node) {
                    node.status = Status::Running;
                }
                run.event(SemanticEvent::Scheduled {
                    node: task.node,
                    request: request.clone(),
                    source: EvaluationSource::Worker,
                });
                run.metrics.executed += 1;
            }
            state.metrics.executed += 1;
            if let Some(key) = &key {
                state.shared.insert(key.clone(), flight_id);
            }
            state.flights.insert(
                flight_id,
                Flight {
                    request: request.clone(),
                    key,
                    consumers: BTreeSet::from([task]),
                    _global: global,
                    _class: class,
                },
            );
            state.metrics.peak_resident = state.metrics.peak_resident.max(state.flights.len());
            let inner = self.clone();
            runtime.spawn_blocking(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    backend.run_step(request)
                }))
                .unwrap_or(Err(ExecutionError::BackendUnavailable));
                if let Ok(mut state) = inner.state.lock() {
                    state.completed.push_back(Completion {
                        flight: flight_id,
                        result,
                    });
                }
                inner.wake.notify_all();
                inner.changed();
            });
            progress = true;
        }
        for (tenant, task) in deferred {
            state.ready.push(tenant, task);
        }
        progress
    }
    fn pump(self: Arc<Self>, runtime: Handle) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.changed();
                return;
            }
        };
        loop {
            let completed = self.complete(&mut state);
            let ready = self.prepare_ready(&mut state);
            let dispatched = self.dispatch(&mut state, &runtime);
            self.changed();
            if completed || ready || dispatched {
                continue;
            }
            if state.flights.is_empty() {
                state.pumping = false;
                self.changed();
                return;
            }
            state = match self.wake.wait(state) {
                Ok(state) => state,
                Err(_) => {
                    self.changed();
                    return;
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{node, test_id};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Backend {
        change_observation: bool,
    }
    impl WorkerBackend for Backend {
        fn resource_class(&self) -> ResourceClass {
            ResourceClass::Wasm
        }
        fn validate_request(&self, _: &StepRequest) -> Result<(), ExecutionError> {
            Ok(())
        }
        fn permits_pure_reuse(&self, _: &StepRequest) -> bool {
            true
        }
        fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
            Ok(StepOutcome {
                attempt: request.attempt,
                profile: request.profile,
                exit: StepExit::Completed {
                    result: test_id(80),
                    delta: test_id(81),
                    observation_root: if self.change_observation {
                        test_id(82)
                    } else {
                        request.observation_root
                    },
                },
            })
        }
    }
    #[derive(Default)]
    struct Repository {
        violate_cache_limit: bool,
        fail_release: AtomicBool,
        owners: Mutex<BTreeSet<String>>,
    }
    impl ResultRepository for Repository {
        fn retain(
            &self,
            owner: &str,
            _: &StepRequest,
            _: &StepOutcome,
            max_bytes: u64,
        ) -> Result<u64, ExecutionError> {
            self.owners
                .lock()
                .map_err(|_| ExecutionError::BackendUnavailable)?
                .insert(owner.to_owned());
            Ok(if self.violate_cache_limit && owner.contains(":cache:") {
                max_bytes + 1
            } else {
                1
            })
        }
        fn release(&self, owner: &str) -> Result<(), ExecutionError> {
            if self.fail_release.load(Ordering::SeqCst) {
                return Err(ExecutionError::BackendUnavailable);
            }
            self.owners
                .lock()
                .map_err(|_| ExecutionError::BackendUnavailable)?
                .remove(owner);
            Ok(())
        }
    }
    fn scheduler(repository: Arc<Repository>, change_observation: bool) -> Scheduler {
        Scheduler::new(
            ScheduleLimits {
                resident_workers: 1,
                queued_branches: 8,
                retained_bytes: 1024 * 1024,
            },
            BTreeMap::from([(
                ResourceClass::Wasm,
                Arc::new(Backend { change_observation }) as Arc<dyn WorkerBackend>,
            )]),
            repository,
        )
        .expect("scheduler")
    }
    #[tokio::test]
    async fn pure_observation_contract_survives_disabled_reuse() {
        let scheduler = scheduler(Arc::new(Repository::default()), true);
        scheduler.set_reuse_enabled(false).expect("disabled reuse");
        let run = scheduler
            .submit(WorldlineGraph {
                nodes: vec![node(1, &[])],
            })
            .expect("run");
        let report = scheduler.drive(run).await.expect("report");
        assert!(report.outcomes.is_empty());
        assert_eq!(
            report.failures.get(&test_id(1)),
            Some(&ExecutionError::InvalidRequest)
        );
    }
    #[tokio::test]
    async fn violated_repository_limit_keeps_failed_cleanup_retryable() {
        let repository = Arc::new(Repository {
            violate_cache_limit: true,
            fail_release: AtomicBool::new(true),
            ..Repository::default()
        });
        let scheduler = scheduler(repository.clone(), false);
        let run = scheduler
            .submit(WorldlineGraph {
                nodes: vec![node(1, &[])],
            })
            .expect("run");
        assert!(matches!(
            scheduler.drive(run).await,
            Err(ScheduleError::Repository(_))
        ));
        assert!(matches!(
            scheduler.submit(WorldlineGraph {
                nodes: vec![node(2, &[])]
            }),
            Err(ScheduleError::Repository(_))
        ));
        assert!(scheduler.clear_cache().is_err());
        repository.fail_release.store(false, Ordering::SeqCst);
        scheduler.clear_cache().expect("retry cleanup");
        scheduler.forget(run).expect("forget historical owner");
        assert!(repository.owners.lock().expect("owners").is_empty());
    }
    #[tokio::test]
    async fn replacement_work_limit_rejects_before_mutating_accepted_history() {
        let scheduler = Scheduler::new(
            ScheduleLimits {
                resident_workers: 1,
                queued_branches: 1000,
                retained_bytes: 32 * 1024 * 1024,
            },
            BTreeMap::from([(
                ResourceClass::Wasm,
                Arc::new(Backend {
                    change_observation: false,
                }) as Arc<dyn WorkerBackend>,
            )]),
            Arc::new(Repository::default()),
        )
        .expect("scheduler");
        let run = scheduler
            .submit(WorldlineGraph {
                nodes: (1..=1000).map(|number| node(number, &[])).collect(),
            })
            .expect("bounded graph");
        scheduler
            .cancel(run)
            .await
            .expect("quiescent cancelled graph");
        for _ in 0..1000 {
            scheduler
                .replace_observation(run, test_id(1), test_id(90))
                .expect("within graph work cap");
            scheduler.cancel(run).await.expect("cancel replacement");
        }
        let snapshot = || {
            let state = scheduler.inner.state.lock().expect("state");
            let stored = &state.runs[&run];
            (
                stored.revision,
                stored.trace.events.len(),
                stored.nodes[&test_id(1)].graph.request.attempt,
                state.queued,
                state.retained,
            )
        };
        let before = snapshot();
        assert!(matches!(
            scheduler.replace_observation(run, test_id(1), test_id(91)),
            Err(ScheduleError::Limit("replacement work"))
        ));
        assert_eq!(
            snapshot(),
            before,
            "rejected replacement mutated accepted history"
        );
    }
}
