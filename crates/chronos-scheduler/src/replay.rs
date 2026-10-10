//! Offline replay consumes immutable recorded objects and semantic events only.
use crate::{EvaluationKey, EvaluationSource, ExecutionTrace, SemanticEvent, WorldlineGraph};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use chronos_state::{
    MAX_GRAPH_BYTES, MAX_GRAPH_DEPTH, MAX_GRAPH_OBJECTS, ObjectData,
    execution::{EffectClass, ExecutionError, StepExit, StepOutcome, StepRequest},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordedInputs {
    pub objects: BTreeMap<ObjectId, ObjectData>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    pub outcomes: BTreeMap<ObjectId, StepOutcome>,
    pub failures: BTreeMap<ObjectId, ExecutionError>,
    pub cancelled: BTreeSet<ObjectId>,
    pub pending: BTreeSet<ObjectId>,
    pub events: usize,
    pub output_hash: ObjectId,
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("invalid recorded execution trace")]
    InvalidTrace,
    #[error("recorded execution exceeds replay limits")]
    Limit,
    #[error("required recorded object is missing")]
    MissingObject(ObjectId),
    #[error("recorded object or its reference graph is invalid")]
    InvalidObject,
}
pub fn replay(
    trace: &ExecutionTrace,
    inputs: &RecordedInputs,
) -> Result<ReplayReport, ReplayError> {
    trace
        .graph
        .validate()
        .map_err(|_| ReplayError::InvalidTrace)?;
    if trace.events.len() > crate::budget::MAX_TRACE_EVENTS {
        return Err(ReplayError::Limit);
    }
    validate_objects(inputs)?;
    for node in &trace.graph.nodes {
        require(inputs, node.request.references())?;
    }
    let mut graph = trace.graph.clone();
    let positions: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    let mut active: BTreeMap<ObjectId, (StepRequest, EvaluationSource)> = BTreeMap::new();
    let mut outcomes = BTreeMap::new();
    let mut failures = BTreeMap::new();
    let mut cancelled = BTreeSet::new();
    let mut pure_results: BTreeMap<EvaluationKey, StepExit> = BTreeMap::new();
    let mut revision = 0_u64;
    let replacement_cost = graph.nodes.len()
        + graph
            .nodes
            .iter()
            .map(|node| node.dependencies.len())
            .sum::<usize>();
    let mut replacement_work = crate::MAX_REPLACEMENT_WORK;
    for event in &trace.events {
        let node_id = match event {
            SemanticEvent::Scheduled { node, .. }
            | SemanticEvent::Completed { node, .. }
            | SemanticEvent::Failed { node, .. }
            | SemanticEvent::Cancelled { node }
            | SemanticEvent::ObservationReplaced { node, .. } => *node,
        };
        let position = *positions.get(&node_id).ok_or(ReplayError::InvalidTrace)?;
        let terminal = outcomes.contains_key(&node_id)
            || failures.contains_key(&node_id)
            || cancelled.contains(&node_id);
        match event {
            SemanticEvent::Scheduled {
                request, source, ..
            } => {
                let node = &graph.nodes[position];
                if terminal
                    || active.contains_key(&node_id)
                    || request != &node.request
                    || node.dependencies.iter().any(|dependency| {
                        !matches!(
                            outcomes.get(dependency),
                            Some(StepOutcome {
                                exit: StepExit::Completed { .. },
                                ..
                            })
                        )
                    })
                {
                    return Err(ReplayError::InvalidTrace);
                }
                request.encode().map_err(|_| ReplayError::InvalidTrace)?;
                require(inputs, request.references())?;
                if *source != EvaluationSource::Worker {
                    EvaluationKey::from_request(request).map_err(|_| ReplayError::InvalidTrace)?;
                }
                active.insert(node_id, (request.clone(), *source));
            }
            SemanticEvent::Completed { outcome, .. } => {
                if terminal {
                    return Err(ReplayError::InvalidTrace);
                }
                let (request, source) = active.remove(&node_id).ok_or(ReplayError::InvalidTrace)?;
                outcome
                    .validate_for(&request)
                    .map_err(|_| ReplayError::InvalidTrace)?;
                require(inputs, outcome.references())?;
                if source != EvaluationSource::Worker
                    && !matches!(outcome.exit, StepExit::Completed { .. })
                {
                    return Err(ReplayError::InvalidTrace);
                }
                if request.effect_class == EffectClass::Pure {
                    let observation = match outcome.exit {
                        StepExit::Completed {
                            observation_root, ..
                        }
                        | StepExit::Suspended {
                            observation_root, ..
                        } => observation_root,
                    };
                    if observation != request.observation_root {
                        return Err(ReplayError::InvalidTrace);
                    }
                    let key = EvaluationKey::from_request(&request)
                        .map_err(|_| ReplayError::InvalidTrace)?;
                    if pure_results
                        .get(&key)
                        .is_some_and(|previous| previous != &outcome.exit)
                    {
                        return Err(ReplayError::InvalidTrace);
                    }
                    pure_results.insert(key, outcome.exit.clone());
                }
                outcomes.insert(node_id, outcome.clone());
            }
            SemanticEvent::Failed { error, .. } => {
                if terminal {
                    return Err(ReplayError::InvalidTrace);
                }
                active.remove(&node_id);
                failures.insert(node_id, *error);
            }
            SemanticEvent::Cancelled { .. } => {
                if terminal {
                    return Err(ReplayError::InvalidTrace);
                }
                active.remove(&node_id);
                cancelled.insert(node_id);
            }
            SemanticEvent::ObservationReplaced {
                observation_root,
                revision: next_revision,
                invalidated,
                ..
            } => {
                replacement_work = replacement_work
                    .checked_sub(replacement_cost)
                    .ok_or(ReplayError::Limit)?;
                if !active.is_empty() || revision.checked_add(1) != Some(*next_revision) {
                    return Err(ReplayError::InvalidTrace);
                }
                parked(&graph, &outcomes, &failures, &cancelled)?;
                require(inputs, [*observation_root])?;
                let expected = crate::invalidate::descendants(&graph, node_id)
                    .map_err(|_| ReplayError::InvalidTrace)?;
                if *invalidated != expected {
                    return Err(ReplayError::InvalidTrace);
                }
                for id in expected {
                    let request = &mut graph.nodes[positions[&id]].request;
                    request.attempt = crate::invalidate::replacement_attempt(
                        request.attempt,
                        *observation_root,
                        *next_revision,
                    )
                    .map_err(|_| ReplayError::InvalidTrace)?;
                    request.observation_root = *observation_root;
                    outcomes.remove(&id);
                    failures.remove(&id);
                    cancelled.remove(&id);
                }
                revision = *next_revision;
            }
        }
    }
    if !active.is_empty() {
        return Err(ReplayError::InvalidTrace);
    }
    let pending = parked(&graph, &outcomes, &failures, &cancelled)?;
    let output_hash = summary_hash(&graph, &outcomes, &failures, &cancelled)?;
    Ok(ReplayReport {
        outcomes,
        failures,
        cancelled,
        pending,
        events: trace.events.len(),
        output_hash,
    })
}

fn require(
    inputs: &RecordedInputs,
    ids: impl IntoIterator<Item = ObjectId>,
) -> Result<(), ReplayError> {
    for id in ids {
        if !inputs.objects.contains_key(&id) {
            return Err(ReplayError::MissingObject(id));
        }
    }
    Ok(())
}
fn validate_objects(inputs: &RecordedInputs) -> Result<(), ReplayError> {
    if inputs.objects.len() > MAX_GRAPH_OBJECTS {
        return Err(ReplayError::Limit);
    }
    let mut bytes = 0_usize;
    let mut edges = BTreeMap::new();
    for (id, data) in &inputs.objects {
        bytes = bytes
            .checked_add(data.bytes.len())
            .ok_or(ReplayError::Limit)?;
        if bytes > MAX_GRAPH_BYTES || data.bytes.len() > Limits::default().object_bytes {
            return Err(ReplayError::Limit);
        }
        if (data.kind, data.schema) != (OBJECT_KIND, OBJECT_SCHEMA)
            || object_id(data.kind, data.schema, &data.bytes) != *id
        {
            return Err(ReplayError::InvalidObject);
        }
        let object = Object::decode(&data.bytes, &Limits::default())
            .map_err(|_| ReplayError::InvalidObject)?;
        require(inputs, object.refs.iter().copied())?;
        edges.insert(*id, object.refs);
    }
    let mut heights = BTreeMap::new();
    let mut active = BTreeSet::new();
    for id in edges.keys() {
        height(*id, 0, &edges, &mut heights, &mut active)?;
    }
    Ok(())
}
fn height(
    id: ObjectId,
    depth: usize,
    edges: &BTreeMap<ObjectId, BTreeSet<ObjectId>>,
    heights: &mut BTreeMap<ObjectId, usize>,
    active: &mut BTreeSet<ObjectId>,
) -> Result<usize, ReplayError> {
    if depth > MAX_GRAPH_DEPTH {
        return Err(ReplayError::Limit);
    }
    if let Some(height) = heights.get(&id) {
        return Ok(*height);
    }
    if !active.insert(id) {
        return Err(ReplayError::InvalidObject);
    }
    let mut result = 0;
    for child in &edges[&id] {
        result = result.max(1 + height(*child, depth + 1, edges, heights, active)?);
    }
    if result > MAX_GRAPH_DEPTH {
        return Err(ReplayError::Limit);
    }
    active.remove(&id);
    heights.insert(id, result);
    Ok(result)
}
fn parked(
    graph: &WorldlineGraph,
    outcomes: &BTreeMap<ObjectId, StepOutcome>,
    failures: &BTreeMap<ObjectId, ExecutionError>,
    cancelled: &BTreeSet<ObjectId>,
) -> Result<BTreeSet<ObjectId>, ReplayError> {
    let pending: BTreeSet<_> = graph
        .nodes
        .iter()
        .map(|node| node.id)
        .filter(|id| {
            !outcomes.contains_key(id) && !failures.contains_key(id) && !cancelled.contains(id)
        })
        .collect();
    if pending.is_empty() {
        return Ok(pending);
    }
    if graph.nodes.iter().any(|node| {
        pending.contains(&node.id)
            && node.dependencies.iter().any(|dependency| {
                failures.contains_key(dependency) || cancelled.contains(dependency)
            })
    }) {
        return Err(ReplayError::InvalidTrace);
    }
    let mut consumers: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
    for node in &graph.nodes {
        for dependency in &node.dependencies {
            consumers.entry(*dependency).or_default().push(node.id);
        }
    }
    let mut roots: Vec<_> = outcomes
        .iter()
        .filter_map(|(id, outcome)| {
            matches!(outcome.exit, StepExit::Suspended { .. }).then_some(*id)
        })
        .collect();
    let mut behind_receipt = BTreeSet::new();
    while let Some(id) = roots.pop() {
        if behind_receipt.insert(id)
            && let Some(children) = consumers.get(&id)
        {
            roots.extend(children.iter().copied());
        }
    }
    if !pending.is_subset(&behind_receipt) {
        return Err(ReplayError::InvalidTrace);
    }
    Ok(pending)
}
fn summary_hash(
    graph: &WorldlineGraph,
    outcomes: &BTreeMap<ObjectId, StepOutcome>,
    failures: &BTreeMap<ObjectId, ExecutionError>,
    cancelled: &BTreeSet<ObjectId>,
) -> Result<ObjectId, ReplayError> {
    let mut nodes: BTreeMap<_, _> = BTreeMap::new();
    for node in &graph.nodes {
        let request = object_id(
            OBJECT_KIND,
            OBJECT_SCHEMA,
            &node
                .request
                .encode()
                .map_err(|_| ReplayError::InvalidTrace)?,
        );
        let state = if let Some(outcome) = outcomes.get(&node.id) {
            Value::Array(vec![
                Value::Unsigned(0),
                Value::Bytes(
                    object_id(
                        OBJECT_KIND,
                        OBJECT_SCHEMA,
                        &outcome.encode().map_err(|_| ReplayError::InvalidTrace)?,
                    )
                    .0
                    .to_vec(),
                ),
            ])
        } else if let Some(error) = failures.get(&node.id) {
            Value::Array(vec![Value::Unsigned(1), failure_value(*error)])
        } else {
            Value::Unsigned(if cancelled.contains(&node.id) { 2 } else { 3 })
        };
        let mut dependencies = node.dependencies.clone();
        dependencies.sort();
        nodes.insert(
            node.id,
            Value::Array(vec![
                Value::Bytes(node.id.0.to_vec()),
                Value::Bytes(node.branch.0.to_vec()),
                Value::Bytes(request.0.to_vec()),
                Value::Array(
                    dependencies
                        .iter()
                        .map(|id| Value::Bytes(id.0.to_vec()))
                        .collect(),
                ),
                state,
            ]),
        );
    }
    let bytes = Value::Array(vec![
        Value::Text("attow-replay-summary".into()),
        Value::Unsigned(1),
        Value::Array(nodes.into_values().collect()),
    ])
    .encode_with_limits(&Limits {
        object_bytes: 16 * 1024 * 1024,
        depth: 64,
        items: 1_000_000,
    })
    .map_err(|_| ReplayError::Limit)?;
    Ok(object_id(0x4156, 1, &bytes))
}
fn failure_value(error: ExecutionError) -> Value {
    let (tag, missing) = match error {
        ExecutionError::UnsupportedResource => (0, None),
        ExecutionError::UnsupportedProfile => (1, None),
        ExecutionError::InvalidRequest => (2, None),
        ExecutionError::MissingObject(id) => (3, Some(id)),
        ExecutionError::ResourceExhausted => (4, None),
        ExecutionError::Trapped => (5, None),
        ExecutionError::Interrupted => (6, None),
        ExecutionError::BackendUnavailable => (7, None),
    };
    Value::Array(vec![
        Value::Unsigned(tag),
        missing.map_or(Value::Null, |id| Value::Bytes(id.0.to_vec())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvaluationSource, GraphNode, SemanticEvent, WorldlineGraph};
    use chronos_codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, Value, object_id};
    use chronos_state::{
        BranchId,
        execution::{EffectClass, ResourceBudget, ResourceClass, StepExit, StepRequest},
    };

    fn fixture() -> (ExecutionTrace, RecordedInputs) {
        let bytes = Object {
            refs: BTreeSet::new(),
            payload: Value::Null,
        }
        .encode()
        .expect("object");
        let object = ObjectData {
            kind: OBJECT_KIND,
            schema: OBJECT_SCHEMA,
            bytes,
        };
        let id = object_id(object.kind, object.schema, &object.bytes);
        let nodes: Vec<_> = (1..=2)
            .map(|number| GraphNode {
                id: ObjectId([number; 32]),
                branch: BranchId([number; 32]),
                dependencies: vec![],
                request: StepRequest {
                    attempt: ObjectId([number; 32]),
                    code: id,
                    profile: id,
                    tenant: ObjectId([9; 32]),
                    security_scope: ObjectId([10; 32]),
                    input_root: id,
                    continuation: id,
                    observation_root: id,
                    policy: id,
                    fidelity: id,
                    sample_id: None,
                    resource_class: ResourceClass::Wasm,
                    effect_class: EffectClass::Pure,
                    budget: ResourceBudget {
                        fuel: 100,
                        memory_bytes: 100,
                        output_bytes: 100,
                        host_calls: 10,
                    },
                },
            })
            .collect();
        let mut events = vec![];
        for node in &nodes {
            events.push(SemanticEvent::Scheduled {
                node: node.id,
                request: node.request.clone(),
                source: if events.is_empty() {
                    EvaluationSource::Worker
                } else {
                    EvaluationSource::Cache
                },
            });
            events.push(SemanticEvent::Completed {
                node: node.id,
                outcome: StepOutcome {
                    attempt: node.request.attempt,
                    profile: id,
                    exit: StepExit::Completed {
                        result: id,
                        delta: id,
                        observation_root: id,
                    },
                },
            });
        }
        (
            ExecutionTrace {
                graph: WorldlineGraph { nodes },
                events,
            },
            RecordedInputs {
                objects: BTreeMap::from([(id, object)]),
            },
        )
    }

    #[test]
    fn recorded_results_replay_without_a_worker_or_dispatch_port() {
        let (trace, inputs) = fixture();
        let report = replay(&trace, &inputs).expect("valid recorded trace");
        assert_eq!(report.outcomes.len(), 2);
        assert_ne!(
            report.outcomes[&ObjectId([1; 32])].attempt,
            report.outcomes[&ObjectId([2; 32])].attempt
        );
        assert!(report.pending.is_empty());
        assert_eq!(report, replay(&trace, &inputs).expect("same replay"));
    }
    #[test]
    fn missing_corrupt_and_inconsistent_records_fail_closed() {
        let (trace, inputs) = fixture();
        replay(&trace, &inputs).expect("baseline valid");
        let mut changed = inputs.clone();
        changed.objects.clear();
        assert!(matches!(
            replay(&trace, &changed),
            Err(ReplayError::MissingObject(_))
        ));
        let mut changed = inputs.clone();
        changed
            .objects
            .values_mut()
            .next()
            .expect("object")
            .bytes
            .push(0);
        assert!(matches!(
            replay(&trace, &changed),
            Err(ReplayError::InvalidObject)
        ));
        for field in 0..4 {
            let mut changed = trace.clone();
            match field {
                0 => {
                    let SemanticEvent::Completed { outcome, .. } = &mut changed.events[1] else {
                        panic!("completion");
                    };
                    outcome.attempt = ObjectId([98; 32]);
                }
                1 => {
                    let SemanticEvent::Scheduled { request, .. } = &mut changed.events[0] else {
                        panic!("schedule");
                    };
                    request.tenant = ObjectId([97; 32]);
                }
                2 => {
                    changed.events.swap(0, 1);
                }
                _ => {
                    changed.events.pop();
                }
            }
            assert!(replay(&changed, &inputs).is_err(), "tampered field {field}");
        }
    }
    #[test]
    fn quiescent_receipt_replacement_rebinds_attempts_and_discards_old_descendants() {
        let (mut trace, mut inputs) = fixture();
        trace.graph.nodes[1].dependencies = vec![trace.graph.nodes[0].id];
        let bytes = Object {
            refs: BTreeSet::new(),
            payload: Value::Unsigned(42),
        }
        .encode()
        .expect("receipt");
        let receipt = object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes);
        inputs.objects.insert(
            receipt,
            ObjectData {
                kind: OBJECT_KIND,
                schema: OBJECT_SCHEMA,
                bytes,
            },
        );
        trace.events.push(SemanticEvent::ObservationReplaced {
            node: ObjectId([1; 32]),
            observation_root: receipt,
            revision: 1,
            invalidated: vec![ObjectId([1; 32]), ObjectId([2; 32])],
        });
        for node in &trace.graph.nodes {
            let mut request = node.request.clone();
            request.attempt = crate::invalidate::replacement_attempt(request.attempt, receipt, 1)
                .expect("new attempt");
            request.observation_root = receipt;
            trace.events.push(SemanticEvent::Scheduled {
                node: node.id,
                request: request.clone(),
                source: EvaluationSource::Worker,
            });
            trace.events.push(SemanticEvent::Completed {
                node: node.id,
                outcome: StepOutcome {
                    attempt: request.attempt,
                    profile: request.profile,
                    exit: StepExit::Completed {
                        result: receipt,
                        delta: request.input_root,
                        observation_root: receipt,
                    },
                },
            });
        }
        let report = replay(&trace, &inputs).expect("receipt trace");
        assert_eq!(report.outcomes.len(), 2);
        assert_ne!(
            report.outcomes[&ObjectId([1; 32])].attempt,
            trace.graph.nodes[0].request.attempt
        );
        let mut wrong = trace;
        let SemanticEvent::ObservationReplaced { invalidated, .. } = &mut wrong.events[4] else {
            panic!("replacement");
        };
        invalidated.pop();
        assert_eq!(replay(&wrong, &inputs), Err(ReplayError::InvalidTrace));
    }
    #[test]
    fn suspended_ancestor_does_not_hide_a_failed_or_cancelled_dependency() {
        let (mut trace, inputs) = fixture();
        let mut child = trace.graph.nodes[0].clone();
        child.id = ObjectId([3; 32]);
        child.branch = BranchId([3; 32]);
        child.request.attempt = child.id;
        child.dependencies = trace.graph.nodes.iter().map(|node| node.id).collect();
        trace.graph.nodes.push(child);
        trace.events.truncate(2);
        let SemanticEvent::Completed { outcome, .. } = &mut trace.events[1] else {
            panic!("completion");
        };
        outcome.exit = StepExit::Suspended {
            continuation: trace.graph.nodes[0].request.continuation,
            request: trace.graph.nodes[0].request.input_root,
            delta: trace.graph.nodes[0].request.input_root,
            observation_root: trace.graph.nodes[0].request.observation_root,
        };
        for terminal in [
            SemanticEvent::Cancelled {
                node: ObjectId([2; 32]),
            },
            SemanticEvent::Failed {
                node: ObjectId([2; 32]),
                error: ExecutionError::Trapped,
            },
        ] {
            let mut changed = trace.clone();
            changed.events.push(terminal);
            assert_eq!(replay(&changed, &inputs), Err(ReplayError::InvalidTrace));
            changed.events.push(SemanticEvent::Cancelled {
                node: ObjectId([3; 32]),
            });
            replay(&changed, &inputs).expect("mandatory cancellation recorded");
        }
    }

    #[test]
    fn repeated_receipt_replacement_has_a_finite_graph_work_budget() {
        let (baseline, inputs) = fixture();
        let template = &baseline.graph.nodes[0];
        let nodes: Vec<_> = (1..=200_u8)
            .map(|number| crate::GraphNode {
                id: ObjectId([number; 32]),
                branch: chronos_state::BranchId([number; 32]),
                request: StepRequest {
                    attempt: ObjectId([number; 32]),
                    ..template.request.clone()
                },
                dependencies: vec![],
            })
            .collect();
        let mut events: Vec<_> = nodes
            .iter()
            .map(|node| SemanticEvent::Cancelled { node: node.id })
            .collect();
        for revision in 1..=6000 {
            events.push(SemanticEvent::ObservationReplaced {
                node: template.id,
                observation_root: template.request.observation_root,
                revision,
                invalidated: vec![template.id],
            });
            events.push(SemanticEvent::Cancelled { node: template.id });
        }
        assert_eq!(
            replay(
                &ExecutionTrace {
                    graph: WorldlineGraph { nodes },
                    events
                },
                &inputs
            ),
            Err(ReplayError::Limit)
        );
    }
}
