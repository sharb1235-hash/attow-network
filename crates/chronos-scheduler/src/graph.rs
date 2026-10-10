//! Bounded logical dependency graphs. Validation grants no backend capabilities.
use std::collections::{BTreeMap, BTreeSet};

use chronos_codec::{Canonical, ObjectId, execution::StepRequest};
use chronos_state::BranchId;

use crate::ScheduleError;

pub const MAX_GRAPH_NODES: usize = 4096;
pub const MAX_NODE_DEPENDENCIES: usize = 64;
pub const MAX_NODE_FANOUT: usize = 64;
pub const MAX_GRAPH_EDGES: usize = 16_384;
pub const MAX_GRAPH_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNode {
    pub id: ObjectId,
    pub branch: BranchId,
    pub request: StepRequest,
    pub dependencies: Vec<ObjectId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldlineGraph {
    pub nodes: Vec<GraphNode>,
}

impl WorldlineGraph {
    pub fn validate(&self) -> Result<(), ScheduleError> {
        self.index().map(|_| ())
    }

    /// Conservative descriptor accounting, not a process RSS measurement.
    /// Preflight applies the hard allocation limits before allocating indexes.
    pub fn retained_bytes(&self) -> Result<u64, ScheduleError> {
        let edges = self.preflight()?;
        (self.nodes.capacity() as u64)
            .checked_mul(1024)
            .and_then(|nodes| {
                (edges as u64)
                    .checked_mul(32)
                    .and_then(|edges| nodes.checked_add(edges))
            })
            .ok_or(ScheduleError::Limit("graph descriptor bytes"))
    }

    fn preflight(&self) -> Result<usize, ScheduleError> {
        if self.nodes.is_empty() {
            return Err(ScheduleError::InvalidGraph);
        }
        if self.nodes.capacity() > MAX_GRAPH_NODES {
            return Err(ScheduleError::Limit("graph nodes"));
        }
        let mut edges = 0usize;
        for node in &self.nodes {
            if node.dependencies.capacity() > MAX_NODE_DEPENDENCIES {
                return Err(ScheduleError::Limit("node dependencies"));
            }
            edges = edges
                .checked_add(node.dependencies.capacity())
                .ok_or(ScheduleError::Limit("graph edges"))?;
            if edges > MAX_GRAPH_EDGES {
                return Err(ScheduleError::Limit("graph edges"));
            }
            // Every request has fixed-size identities; encoding also enforces
            // the shared codec's budget constraints before backend admission.
            node.request
                .encode()
                .map_err(|_| ScheduleError::InvalidRequest)?;
        }
        Ok(edges)
    }

    pub(crate) fn index(&self) -> Result<GraphIndex, ScheduleError> {
        self.preflight()?;
        let mut by_id = BTreeMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            if by_id.insert(node.id, index).is_some() {
                return Err(ScheduleError::InvalidGraph);
            }
        }
        let mut children = vec![Vec::new(); self.nodes.len()];
        let mut incoming = vec![0usize; self.nodes.len()];
        let mut ready = BTreeSet::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let mut unique = BTreeSet::new();
            for dependency in &node.dependencies {
                if dependency == &node.id || !unique.insert(*dependency) {
                    return Err(ScheduleError::InvalidGraph);
                }
                let parent = *by_id.get(dependency).ok_or(ScheduleError::InvalidGraph)?;
                if children[parent].len() >= MAX_NODE_FANOUT {
                    return Err(ScheduleError::Limit("node fanout"));
                }
                children[parent].push(index);
            }
            incoming[index] = node.dependencies.len();
            if incoming[index] == 0 {
                ready.insert((node.id, index));
            }
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut depth = vec![1usize; self.nodes.len()];
        while let Some((_, index)) = ready.pop_first() {
            order.push(index);
            for &child in &children[index] {
                depth[child] = depth[child].max(depth[index] + 1);
                if depth[child] > MAX_GRAPH_DEPTH {
                    return Err(ScheduleError::Limit("graph depth"));
                }
                incoming[child] -= 1;
                if incoming[child] == 0 {
                    ready.insert((self.nodes[child].id, child));
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(ScheduleError::InvalidGraph);
        }
        Ok(GraphIndex {
            by_id,
            children,
            order,
        })
    }
}

pub(crate) struct GraphIndex {
    pub by_id: BTreeMap<ObjectId, usize>,
    pub children: Vec<Vec<usize>>,
    pub order: Vec<usize>,
}

#[cfg(test)]
pub(crate) fn node(number: u16, dependencies: &[u16]) -> GraphNode {
    use chronos_codec::execution::{EffectClass, ResourceBudget, ResourceClass};
    let id = test_id(number);
    GraphNode {
        id,
        branch: BranchId(id.0),
        dependencies: dependencies.iter().copied().map(test_id).collect(),
        request: StepRequest {
            attempt: id,
            code: test_id(100),
            profile: test_id(101),
            tenant: test_id(102),
            security_scope: test_id(103),
            input_root: test_id(104),
            continuation: test_id(105),
            observation_root: test_id(106),
            policy: test_id(107),
            fidelity: test_id(108),
            sample_id: None,
            resource_class: ResourceClass::Wasm,
            effect_class: EffectClass::Pure,
            budget: ResourceBudget {
                fuel: 100,
                memory_bytes: 65_536,
                output_bytes: 1024,
                host_calls: 100,
            },
        },
    }
}

#[cfg(test)]
pub(crate) fn test_id(number: u16) -> ObjectId {
    let mut bytes = [0; 32];
    bytes[..2].copy_from_slice(&number.to_be_bytes());
    ObjectId(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_diamond_retains_distinct_branches_and_charges_descriptor_bytes() {
        let graph = WorldlineGraph {
            nodes: vec![node(4, &[2, 3]), node(2, &[1]), node(1, &[]), node(3, &[1])],
        };
        assert!(graph.validate().is_ok());
        assert_ne!(graph.nodes[0].branch, graph.nodes[1].branch);
        assert!(graph.retained_bytes().expect("retained bytes") >= 4 * 1024 + 4 * 32);
    }

    #[test]
    fn duplicate_missing_self_cyclic_and_invalid_budget_graphs_fail() {
        for nodes in [
            vec![node(1, &[]), node(1, &[])],
            vec![node(1, &[9])],
            vec![node(1, &[1])],
            vec![node(1, &[2]), node(2, &[1])],
            vec![node(1, &[]), node(2, &[1, 1])],
            vec![],
        ] {
            assert!(WorldlineGraph { nodes }.validate().is_err());
        }
        let mut bad = node(1, &[]);
        bad.request.budget.fuel = 0;
        assert!(WorldlineGraph { nodes: vec![bad] }.validate().is_err());
    }

    #[test]
    fn graph_depth_fanout_and_node_count_are_bounded() {
        let chain = |length: u16| WorldlineGraph {
            nodes: (1..=length)
                .map(|id| node(id, &if id == 1 { vec![] } else { vec![id - 1] }))
                .collect(),
        };
        assert!(chain(MAX_GRAPH_DEPTH as u16).validate().is_ok());
        assert!(chain(MAX_GRAPH_DEPTH as u16 + 1).validate().is_err());
        let mut fanout = vec![node(1, &[])];
        fanout.extend((2..=MAX_NODE_FANOUT as u16 + 2).map(|id| node(id, &[1])));
        assert!(WorldlineGraph { nodes: fanout }.validate().is_err());
        let nodes = (0..=MAX_GRAPH_NODES as u16)
            .map(|id| node(id, &[]))
            .collect();
        assert!(WorldlineGraph { nodes }.validate().is_err());
    }

    #[test]
    fn retained_spare_capacity_is_charged_and_cannot_bypass_graph_caps() {
        let mut entry = node(1, &[]);
        entry.dependencies = Vec::with_capacity(8);
        let mut nodes = Vec::with_capacity(8);
        nodes.push(entry);
        let graph = WorldlineGraph { nodes };
        assert!(graph.retained_bytes().expect("bounded capacity") >= 8 * 1024 + 8 * 32);

        let mut nodes = Vec::with_capacity(MAX_GRAPH_NODES + 1);
        nodes.push(node(1, &[]));
        assert!(WorldlineGraph { nodes }.validate().is_err());
        let mut entry = node(1, &[]);
        entry.dependencies = Vec::with_capacity(MAX_NODE_DEPENDENCIES + 1);
        assert!(WorldlineGraph { nodes: vec![entry] }.validate().is_err());
    }
}
