use crate::{Cell, Dependency, FrozenBranch, Mutation, StateError, StateRoot, Version};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub key: Vec<u8>,
    pub before: Option<Cell>,
    pub after: Option<Cell>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Conflict {
    Key {
        key: Vec<u8>,
        expected: Version,
        actual: Version,
    },
    Namespace {
        expected: u64,
        actual: u64,
    },
    Write {
        key: Vec<u8>,
    },
    ReadWrite {
        key: Vec<u8>,
    },
    ScanWrite,
    BaseMismatch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedProposal {
    pub overlay: BTreeMap<Vec<u8>, Mutation>,
    pub dependencies: Vec<Dependency>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinOutcome {
    Compatible(JoinedProposal),
    Conflicts(Vec<Conflict>),
}

pub fn diff(left: &StateRoot, right: &StateRoot) -> Result<Vec<Difference>, StateError> {
    if left.domain() != right.domain() {
        return Err(StateError::DomainMismatch);
    }
    let keys: BTreeSet<_> = left
        .cells
        .iter()
        .map(|(key, _)| key.clone())
        .chain(right.cells.iter().map(|(key, _)| key.clone()))
        .collect();
    let mut differences = Vec::new();
    for key in keys {
        let before = left.cell(&key)?;
        let after = right.cell(&key)?;
        if before != after {
            differences.push(Difference { key, before, after });
        }
    }
    Ok(differences)
}
impl FrozenBranch {
    pub fn conflicts(&self, current: &StateRoot) -> Result<Vec<Conflict>, StateError> {
        if self.domain() != current.domain() {
            return Err(StateError::DomainMismatch);
        }
        let mut conflicts = Vec::new();
        for dependency in self.dependencies() {
            match dependency {
                Dependency::Key { key, version } => {
                    let actual = current.version(key)?;
                    if actual != *version {
                        conflicts.push(Conflict::Key {
                            key: key.clone(),
                            expected: *version,
                            actual,
                        });
                    }
                }
                Dependency::Namespace { epoch } => {
                    let actual = current.namespace_epoch();
                    if actual != *epoch {
                        conflicts.push(Conflict::Namespace {
                            expected: *epoch,
                            actual,
                        });
                    }
                }
            }
        }
        Ok(conflicts)
    }
}
/// Conservative compatibility of two proposals, never authorization or a general
/// serializability proof. Shared writes, read/write overlaps and scans conflict.
pub fn join(left: &FrozenBranch, right: &FrozenBranch) -> Result<JoinOutcome, StateError> {
    if left.domain() != right.domain() {
        return Err(StateError::DomainMismatch);
    }
    if left.base().object_id()? != right.base().object_id()? {
        return Ok(JoinOutcome::Conflicts(vec![Conflict::BaseMismatch]));
    }
    let mut conflicts = BTreeSet::new();
    for key in left.overlay().keys() {
        if right.overlay().contains_key(key) {
            conflicts.insert(Conflict::Write { key: key.clone() });
        }
    }
    for (reader, writer) in [(left, right), (right, left)] {
        for dependency in reader.dependencies() {
            match dependency {
                Dependency::Key { key, .. } if writer.overlay().contains_key(key) => {
                    conflicts.insert(Conflict::ReadWrite { key: key.clone() });
                }
                Dependency::Namespace { .. } if !writer.overlay().is_empty() => {
                    conflicts.insert(Conflict::ScanWrite);
                }
                _ => {}
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(JoinOutcome::Conflicts(conflicts.into_iter().collect()));
    }
    let mut overlay = left.overlay().clone();
    overlay.extend(right.overlay().clone());
    crate::branch::check_overlay(&overlay)?;
    let dependencies: BTreeSet<_> = left
        .dependencies()
        .iter()
        .chain(right.dependencies())
        .cloned()
        .collect();
    if overlay.len() > crate::MAX_KEYS || dependencies.len() > crate::MAX_DEPENDENCIES {
        return Err(StateError::Limit("joined proposal"));
    }
    Ok(JoinOutcome::Compatible(JoinedProposal {
        overlay,
        dependencies: dependencies.into_iter().collect(),
    }))
}
