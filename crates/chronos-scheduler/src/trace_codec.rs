//! Portable trace payload `[0x41545452,1,nodes,events]` inside a known Object.
//! Worker records are their existing canonical byte envelopes. Node/branch/attempt
//! identities and missing-object diagnostics are not CAS references. Structural
//! decoding grants no capability; offline replay must verify event semantics.
use std::collections::BTreeSet;

use crate::{
    EvaluationSource, ExecutionError, ExecutionTrace, GraphNode, SemanticEvent, WorldlineGraph,
};
use chronos_codec::{Canonical, CodecError, Limits, Object, ObjectId, Value};
use chronos_state::{
    BranchId,
    execution::{StepOutcome, StepRequest},
};

const TAG: u64 = 0x4154_5452;
const MAX_REFS: usize = 4096;
const MAX_BYTES: usize = 1024 * 1024;

fn limits(caller: &Limits) -> Result<Limits, CodecError> {
    caller.validate()?;
    Ok(Limits {
        object_bytes: caller.object_bytes.min(MAX_BYTES),
        depth: caller.depth.min(16),
        items: caller.items.min(100_000),
    })
}
fn id(value: ObjectId) -> Value {
    Value::Bytes(value.0.to_vec())
}
fn uint(value: u64) -> Value {
    Value::Unsigned(value)
}
fn fields<const N: usize>(value: Value) -> Result<[Value; N], CodecError> {
    let Value::Array(values) = value else {
        return Err(CodecError::InvalidObject);
    };
    values.try_into().map_err(|_| CodecError::InvalidObject)
}
fn number(value: Value) -> Result<u64, CodecError> {
    match value {
        Value::Unsigned(n) => Ok(n),
        _ => Err(CodecError::InvalidObject),
    }
}
fn bytes(value: Value) -> Result<Vec<u8>, CodecError> {
    match value {
        Value::Bytes(bytes) => Ok(bytes),
        _ => Err(CodecError::InvalidObject),
    }
}
fn read_id(value: Value) -> Result<ObjectId, CodecError> {
    Ok(ObjectId(
        bytes(value)?
            .try_into()
            .map_err(|_| CodecError::InvalidObject)?,
    ))
}
fn array(value: Value, cap: usize) -> Result<Vec<Value>, CodecError> {
    match value {
        Value::Array(values) if values.len() <= cap => Ok(values),
        Value::Array(_) => Err(CodecError::ItemLimit),
        _ => Err(CodecError::InvalidObject),
    }
}
fn error_value(error: ExecutionError) -> Value {
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
    Value::Array(vec![uint(tag), missing.map_or(Value::Null, id)])
}
fn read_error(value: Value) -> Result<ExecutionError, CodecError> {
    let [tag, detail] = fields(value)?;
    let tag = number(tag)?;
    if tag == 3 {
        return Ok(ExecutionError::MissingObject(read_id(detail)?));
    }
    if detail != Value::Null {
        return Err(CodecError::InvalidObject);
    }
    match tag {
        0 => Ok(ExecutionError::UnsupportedResource),
        1 => Ok(ExecutionError::UnsupportedProfile),
        2 => Ok(ExecutionError::InvalidRequest),
        4 => Ok(ExecutionError::ResourceExhausted),
        5 => Ok(ExecutionError::Trapped),
        6 => Ok(ExecutionError::Interrupted),
        7 => Ok(ExecutionError::BackendUnavailable),
        _ => Err(CodecError::InvalidObject),
    }
}
// Charge before retaining a record. The fixed allowance covers all outer array
// headers; reference bytes are counted separately as they enter the exact set.
struct Budget {
    size: usize,
    refs: BTreeSet<ObjectId>,
}
impl Budget {
    fn add(
        &mut self,
        value: &Value,
        refs: impl IntoIterator<Item = ObjectId>,
    ) -> Result<(), CodecError> {
        self.size = self
            .size
            .checked_add(value.encode()?.len())
            .ok_or(CodecError::ByteLimit)?;
        for reference in refs {
            if !self.refs.contains(&reference) {
                if self.refs.len() == MAX_REFS {
                    return Err(CodecError::ItemLimit);
                }
                self.size = self.size.checked_add(34).ok_or(CodecError::ByteLimit)?;
                self.refs.insert(reference);
            }
        }
        if self.size > MAX_BYTES {
            return Err(CodecError::ByteLimit);
        }
        Ok(())
    }
}
impl ExecutionTrace {
    fn object(&self) -> Result<Object, CodecError> {
        self.graph
            .validate()
            .map_err(|_| CodecError::InvalidObject)?;
        if self.events.len() > crate::budget::MAX_TRACE_EVENTS {
            return Err(CodecError::ItemLimit);
        }
        let node_ids: BTreeSet<_> = self.graph.nodes.iter().map(|node| node.id).collect();
        let mut budget = Budget {
            size: 64,
            refs: BTreeSet::new(),
        };
        let mut nodes = Vec::new();
        for node in &self.graph.nodes {
            let value = Value::Array(vec![
                id(node.id),
                Value::Bytes(node.branch.0.to_vec()),
                Value::Bytes(node.request.encode()?),
                Value::Array(node.dependencies.iter().copied().map(id).collect()),
            ]);
            budget.add(&value, node.request.references())?;
            nodes.push(value);
        }
        let cost = self.graph.nodes.len()
            + self
                .graph
                .nodes
                .iter()
                .map(|node| node.dependencies.len())
                .sum::<usize>();
        let mut remaining_work = crate::MAX_REPLACEMENT_WORK;
        let mut events = Vec::new();
        for event in &self.events {
            let (node, value, refs) = match event {
                SemanticEvent::Scheduled {
                    node,
                    request,
                    source,
                } => (
                    *node,
                    Value::Array(vec![
                        uint(0),
                        id(*node),
                        Value::Bytes(request.encode()?),
                        uint(match source {
                            EvaluationSource::Worker => 0,
                            EvaluationSource::Shared => 1,
                            EvaluationSource::Cache => 2,
                        }),
                    ]),
                    request.references(),
                ),
                SemanticEvent::Completed { node, outcome } => (
                    *node,
                    Value::Array(vec![uint(1), id(*node), Value::Bytes(outcome.encode()?)]),
                    outcome.references(),
                ),
                SemanticEvent::Failed { node, error } => (
                    *node,
                    Value::Array(vec![uint(2), id(*node), error_value(*error)]),
                    BTreeSet::new(),
                ),
                SemanticEvent::Cancelled { node } => (
                    *node,
                    Value::Array(vec![uint(3), id(*node)]),
                    BTreeSet::new(),
                ),
                SemanticEvent::ObservationReplaced {
                    node,
                    observation_root,
                    revision,
                    invalidated,
                } => {
                    remaining_work = remaining_work
                        .checked_sub(cost)
                        .ok_or(CodecError::ItemLimit)?;
                    if *revision == 0
                        || invalidated.is_empty()
                        || invalidated.len() > node_ids.len()
                        || invalidated.iter().any(|id| !node_ids.contains(id))
                        || invalidated.iter().collect::<BTreeSet<_>>().len() != invalidated.len()
                    {
                        return Err(CodecError::InvalidObject);
                    }
                    (
                        *node,
                        Value::Array(vec![
                            uint(4),
                            id(*node),
                            id(*observation_root),
                            uint(*revision),
                            Value::Array(invalidated.iter().copied().map(id).collect()),
                        ]),
                        BTreeSet::from([*observation_root]),
                    )
                }
            };
            if !node_ids.contains(&node) {
                return Err(CodecError::InvalidObject);
            }
            budget.add(&value, refs)?;
            events.push(value);
        }
        Ok(Object {
            refs: budget.refs,
            payload: Value::Array(vec![
                uint(TAG),
                uint(1),
                Value::Array(nodes),
                Value::Array(events),
            ]),
        })
    }
}
impl Canonical for ExecutionTrace {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.object()?
            .encode_with_limits(&limits(&Limits::default())?)
    }
    fn decode(encoded: &[u8], caller: &Limits) -> Result<Self, CodecError> {
        let limits = limits(caller)?;
        let object = Object::decode(encoded, &limits)?;
        if object.refs.len() > MAX_REFS {
            return Err(CodecError::ItemLimit);
        }
        let [tag, version, nodes, events] = fields(object.payload)?;
        if number(tag)? != TAG || number(version)? != 1 {
            return Err(CodecError::UnknownSchema);
        }
        let mut graph = Vec::new();
        for node in array(nodes, crate::graph::MAX_GRAPH_NODES)? {
            let [node, branch, request, dependencies] = fields(node)?;
            let dependencies = array(dependencies, crate::graph::MAX_NODE_DEPENDENCIES)?
                .into_iter()
                .map(read_id)
                .collect::<Result<Vec<_>, _>>()?;
            graph.push(GraphNode {
                id: read_id(node)?,
                branch: BranchId(read_id(branch)?.0),
                request: StepRequest::decode(&bytes(request)?, &limits)?,
                dependencies,
            });
        }
        let mut decoded_events = Vec::new();
        for event in array(events, crate::budget::MAX_TRACE_EVENTS)? {
            let mut event = array(event, 5)?.into_iter();
            let tag = number(event.next().ok_or(CodecError::InvalidObject)?)?;
            let node = read_id(event.next().ok_or(CodecError::InvalidObject)?)?;
            let next = |event: &mut std::vec::IntoIter<Value>| {
                event.next().ok_or(CodecError::InvalidObject)
            };
            let decoded = match tag {
                0 => SemanticEvent::Scheduled {
                    node,
                    request: StepRequest::decode(&bytes(next(&mut event)?)?, &limits)?,
                    source: match number(next(&mut event)?)? {
                        0 => EvaluationSource::Worker,
                        1 => EvaluationSource::Shared,
                        2 => EvaluationSource::Cache,
                        _ => return Err(CodecError::InvalidObject),
                    },
                },
                1 => SemanticEvent::Completed {
                    node,
                    outcome: StepOutcome::decode(&bytes(next(&mut event)?)?, &limits)?,
                },
                2 => SemanticEvent::Failed {
                    node,
                    error: read_error(next(&mut event)?)?,
                },
                3 => SemanticEvent::Cancelled { node },
                4 => SemanticEvent::ObservationReplaced {
                    node,
                    observation_root: read_id(next(&mut event)?)?,
                    revision: number(next(&mut event)?)?,
                    invalidated: array(next(&mut event)?, crate::graph::MAX_GRAPH_NODES)?
                        .into_iter()
                        .map(read_id)
                        .collect::<Result<Vec<_>, _>>()?,
                },
                _ => return Err(CodecError::InvalidObject),
            };
            if event.next().is_some() {
                return Err(CodecError::InvalidObject);
            }
            decoded_events.push(decoded);
        }
        let trace = Self {
            graph: WorldlineGraph { nodes: graph },
            events: decoded_events,
        };
        // Check exact refs and the portable preallocation/work bounds as well as
        // shape. Semantic event ordering is deliberately the replayer's job.
        let expected = trace.object()?;
        if expected.refs != object.refs {
            return Err(CodecError::InvalidObject);
        }
        Ok(trace)
    }
}
