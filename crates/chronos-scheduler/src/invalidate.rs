//! Conservative invalidation membership, with no mutation or backend execution.
use crate::{ScheduleError, graph::WorldlineGraph};
use chronos_codec::{Canonical, ObjectId, Value, object_id};

pub fn descendants(graph: &WorldlineGraph, root: ObjectId) -> Result<Vec<ObjectId>, ScheduleError> {
    let index = graph.index()?;
    let root = *index.by_id.get(&root).ok_or(ScheduleError::UnknownNode)?;
    let mut affected = vec![false; graph.nodes.len()];
    affected[root] = true;
    let mut output = Vec::new();
    for node in index.order {
        if affected[node] {
            output.push(graph.nodes[node].id);
            for &child in &index.children[node] {
                affected[child] = true;
            }
        }
    }
    Ok(output)
}

/// Derive the next opaque attempt identity; revision is the run's replacement
/// counter starting at one. This identity is not a persisted object reference.
pub fn replacement_attempt(
    previous: ObjectId,
    observation: ObjectId,
    revision: u64,
) -> Result<ObjectId, ScheduleError> {
    if revision == 0 {
        return Err(ScheduleError::InvalidRequest);
    }
    let bytes = Value::Array(vec![
        Value::Bytes(previous.0.to_vec()),
        Value::Bytes(observation.0.to_vec()),
        Value::Unsigned(revision),
    ])
    .encode()
    .map_err(|_| ScheduleError::InvalidRequest)?;
    Ok(object_id(0x4152, 1, &bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{node, test_id};

    #[test]
    fn invalidation_includes_root_and_transitive_consumers_without_touching_siblings() {
        let graph = WorldlineGraph {
            nodes: vec![
                node(4, &[2, 3]),
                node(3, &[1]),
                node(5, &[]),
                node(2, &[1]),
                node(1, &[]),
            ],
        };
        assert_eq!(
            descendants(&graph, test_id(2)).expect("subtree"),
            vec![test_id(2), test_id(4)]
        );
        assert_eq!(
            descendants(&graph, test_id(1)).expect("diamond"),
            vec![test_id(1), test_id(2), test_id(3), test_id(4)]
        );
        let before = graph.clone();
        assert!(descendants(&graph, test_id(99)).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn replacement_attempt_binds_previous_observation_and_monotonic_revision() {
        let previous = test_id(1);
        let observation = test_id(2);
        let next = replacement_attempt(previous, observation, 1).expect("next");
        assert_eq!(
            next,
            replacement_attempt(previous, observation, 1).expect("deterministic")
        );
        assert_ne!(next, previous);
        assert_ne!(
            next,
            replacement_attempt(test_id(3), observation, 1).expect("previous")
        );
        assert_ne!(
            next,
            replacement_attempt(previous, test_id(3), 1).expect("observation")
        );
        assert_ne!(
            next,
            replacement_attempt(previous, observation, 2).expect("revision")
        );
        assert!(replacement_attempt(previous, observation, 0).is_err());
    }
}
