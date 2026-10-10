use std::collections::{BTreeMap, BTreeSet};

use attow_scheduler::{
    EvaluationSource, ExecutionError, ExecutionTrace, GraphNode, RecordedInputs, SemanticEvent,
    WorldlineGraph, replay,
};
use chronos_codec::{
    Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use chronos_state::{
    BranchId, ObjectData,
    execution::{EffectClass, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest},
};

fn fixture() -> (ExecutionTrace, RecordedInputs) {
    let bytes = Object {
        refs: BTreeSet::new(),
        payload: Value::Null,
    }
    .encode()
    .expect("fixture");
    let root = object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes);
    let node = ObjectId([10; 32]);
    let request = StepRequest {
        attempt: ObjectId([11; 32]),
        code: root,
        profile: root,
        tenant: ObjectId([12; 32]),
        security_scope: ObjectId([13; 32]),
        input_root: root,
        continuation: root,
        observation_root: root,
        policy: root,
        fidelity: root,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: 1,
            memory_bytes: 1,
            output_bytes: 1,
            host_calls: 0,
        },
    };
    let outcome = StepOutcome {
        attempt: request.attempt,
        profile: root,
        exit: StepExit::Completed {
            result: root,
            delta: root,
            observation_root: root,
        },
    };
    (
        ExecutionTrace {
            graph: WorldlineGraph {
                nodes: vec![GraphNode {
                    id: node,
                    branch: BranchId([14; 32]),
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
        RecordedInputs {
            objects: BTreeMap::from([(
                root,
                ObjectData {
                    kind: OBJECT_KIND,
                    schema: OBJECT_SCHEMA,
                    bytes,
                },
            )]),
        },
    )
}

#[test]
fn portable_trace_roundtrip_preserves_offline_replay_and_exact_references() {
    let (trace, inputs) = fixture();
    let bytes = trace.encode().expect("encode trace");
    let decoded = ExecutionTrace::decode(&bytes, &Limits::default()).expect("decode trace");
    assert_eq!(trace, decoded);
    assert_eq!(bytes, decoded.encode().expect("canonical"));
    assert_eq!(
        replay(&trace, &inputs).expect("source replay"),
        replay(&decoded, &inputs).expect("portable replay")
    );
    assert_eq!(
        Object::decode(&bytes, &Limits::default())
            .expect("known envelope")
            .refs,
        inputs.objects.keys().copied().collect()
    );
}

#[test]
fn every_event_and_typed_failure_roundtrips_without_turning_diagnostics_into_refs() {
    let (mut trace, _) = fixture();
    let node = trace.graph.nodes[0].id;
    for error in [
        ExecutionError::UnsupportedResource,
        ExecutionError::UnsupportedProfile,
        ExecutionError::InvalidRequest,
        ExecutionError::MissingObject(ObjectId([90; 32])),
        ExecutionError::ResourceExhausted,
        ExecutionError::Trapped,
        ExecutionError::Interrupted,
        ExecutionError::BackendUnavailable,
    ] {
        trace.events.push(SemanticEvent::Failed { node, error });
    }
    for source in [EvaluationSource::Shared, EvaluationSource::Cache] {
        trace.events.push(SemanticEvent::Scheduled {
            node,
            request: trace.graph.nodes[0].request.clone(),
            source,
        });
    }
    let receipt = ObjectId([91; 32]);
    trace.events.push(SemanticEvent::Cancelled { node });
    trace.events.push(SemanticEvent::ObservationReplaced {
        node,
        observation_root: receipt,
        revision: 1,
        invalidated: vec![node],
    });
    trace.events.push(SemanticEvent::Completed {
        node,
        outcome: StepOutcome {
            attempt: node,
            profile: receipt,
            exit: StepExit::Suspended {
                continuation: receipt,
                request: receipt,
                delta: receipt,
                observation_root: receipt,
            },
        },
    });
    let bytes = trace.encode().expect("all variants");
    assert_eq!(
        ExecutionTrace::decode(&bytes, &Limits::default()).expect("decode"),
        trace
    );
    let refs = Object::decode(&bytes, &Limits::default())
        .expect("envelope")
        .refs;
    assert!(refs.contains(&receipt));
    assert!(!refs.contains(&ObjectId([90; 32])));
}

#[test]
fn unknown_version_missing_refs_extra_fields_and_malformed_bytes_fail_closed() {
    let (trace, _) = fixture();
    let bytes = trace.encode().expect("baseline");
    for mutation in 0..5 {
        let mut object = Object::decode(&bytes, &Limits::default()).expect("envelope");
        let Value::Array(fields) = &mut object.payload else {
            panic!("array")
        };
        match mutation {
            0 => fields[1] = Value::Unsigned(99),
            1 => object.refs.clear(),
            2 => {
                object.refs.insert(ObjectId([99; 32]));
            }
            3 => fields.push(Value::Null),
            _ => fields[3] = Value::Array(vec![Value::Array(vec![Value::Unsigned(99)])]),
        }
        assert!(
            ExecutionTrace::decode(
                &object.encode().expect("canonical tamper"),
                &Limits::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
    for prefix in 0..bytes.len() {
        assert!(ExecutionTrace::decode(&bytes[..prefix], &Limits::default()).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(ExecutionTrace::decode(&trailing, &Limits::default()).is_err());
    assert!(
        ExecutionTrace::decode(
            &[0x9b, 255, 255, 255, 255, 255, 255, 255, 255],
            &Limits::default()
        )
        .is_err()
    );
}

#[test]
fn portable_byte_event_and_reference_bounds_are_enforced_even_with_looser_caller_limits() {
    let (mut trace, _) = fixture();
    let event = trace.events[0].clone();
    trace.events = vec![event; 16_385];
    assert!(trace.encode().is_err());
    let loose = Limits {
        object_bytes: 2 * 1024 * 1024,
        ..Limits::default()
    };
    assert!(ExecutionTrace::decode(&vec![0; 1024 * 1024 + 1], &loose).is_err());
    let (trace, _) = fixture();
    let bytes = trace.encode().expect("baseline");
    let tight = Limits {
        object_bytes: bytes.len() - 1,
        ..Limits::default()
    };
    assert!(ExecutionTrace::decode(&bytes, &tight).is_err());
}
