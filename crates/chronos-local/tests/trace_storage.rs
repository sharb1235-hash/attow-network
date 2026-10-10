#![cfg(feature = "scheduler")]
use attow_local::scheduler::{
    EvaluationSource, ExecutionTrace, GraphNode, ResultRepository, SemanticEvent,
    SqliteResultRepository, WorldlineGraph, replay,
};
use chronos_codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id};
use chronos_state::{
    BranchId,
    execution::{EffectClass, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest},
};
use chronos_store::Store;
use std::{collections::BTreeSet, path::Path};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(path: &Path) -> Result<(ExecutionTrace, ObjectId), Box<dyn std::error::Error>> {
    let mut store = Store::open(path)?;
    let root = store.put(
        OBJECT_KIND,
        OBJECT_SCHEMA,
        &Object {
            refs: BTreeSet::new(),
            payload: Value::Null,
        }
        .encode()?,
    )?;
    let request = StepRequest {
        attempt: ObjectId([1; 32]),
        code: root,
        profile: root,
        tenant: ObjectId([2; 32]),
        security_scope: ObjectId([3; 32]),
        input_root: root,
        continuation: root,
        observation_root: root,
        policy: root,
        fidelity: root,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: 100,
            memory_bytes: 100,
            output_bytes: 100,
            host_calls: 0,
        },
    };
    let node = ObjectId([4; 32]);
    let outcome = StepOutcome {
        attempt: request.attempt,
        profile: root,
        exit: StepExit::Completed {
            result: root,
            delta: root,
            observation_root: root,
        },
    };
    Ok((
        ExecutionTrace {
            graph: WorldlineGraph {
                nodes: vec![GraphNode {
                    id: node,
                    branch: BranchId([5; 32]),
                    request: request.clone(),
                    dependencies: vec![],
                }],
            },
            events: vec![
                SemanticEvent::Scheduled {
                    node,
                    request,
                    source: EvaluationSource::Worker,
                },
                SemanticEvent::Completed { node, outcome },
            ],
        },
        root,
    ))
}

#[test]
fn persisted_trace_survives_reopen_and_replays_without_workers() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("trace.sqlite");
    let (trace, leaf) = fixture(&path)?;
    let repo = SqliteResultRepository::open(&path)?;
    let id = repo.store_trace("trace-run", &trace)?;
    drop(repo);
    Store::open(&path)?.collect_garbage()?;
    let repo = SqliteResultRepository::open(&path)?;
    let restored = repo.load_trace(id)?;
    assert_eq!(restored, trace);
    let inputs = repo.recorded_inputs(&restored)?;
    assert_eq!(replay(&restored, &inputs)?, replay(&trace, &inputs)?);
    assert!(Store::open(&path)?.contains(leaf)?);
    repo.release("trace-run")?;
    let mut store = Store::open(&path)?;
    store.collect_garbage()?;
    assert!(!store.contains(id)?);
    assert!(!store.contains(leaf)?);
    Ok(())
}

#[test]
fn incomplete_trace_graph_cannot_leave_an_object_or_pin() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("trace.sqlite");
    let (mut trace, _) = fixture(&path)?;
    let repo = SqliteResultRepository::open(&path)?;
    let good = repo.store_trace("valid", &trace)?;
    trace.graph.nodes[0].request.code = ObjectId([99; 32]);
    let id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &trace.encode()?);
    assert!(repo.store_trace("invalid", &trace).is_err());
    assert!(!Store::open(&path)?.contains(id)?);
    assert!(repo.load_trace(good).is_ok());
    assert!(repo.store_trace("", &trace).is_err());
    Ok(())
}
