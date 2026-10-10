use crate::{Scope, TransportError};
use chronos_codec::{Limits, ObjectId, object_id, references};
use chronos_store::{Store, StoreError};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};

pub const MAX_GRAPH_OBJECTS: usize = 4096;
pub const MAX_GRAPH_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferObject {
    pub id: ObjectId,
    pub kind: u16,
    pub schema: u16,
    pub bytes: Vec<u8>,
}
/// Trusted bounded persistence port. Implementations must enforce scope access
/// and return success only after validated objects and root pins commit together.
pub trait ObjectRepository: Send + Sync {
    fn export(
        &self,
        scope: Scope,
        roots: &[ObjectId],
        max_bytes: usize,
    ) -> Result<Vec<TransferObject>, TransportError>;
    fn import(
        &self,
        scope: Scope,
        roots: &[ObjectId],
        objects: Vec<TransferObject>,
        max_bytes: usize,
    ) -> Result<u64, TransportError>;
}
pub struct SqliteObjectRepository {
    store: Mutex<Store>,
}
fn owner(scope: Scope) -> String {
    format!("peer:{}:{}", scope.tenant, scope.domain)
}
fn codec_limits() -> Limits {
    Limits {
        object_bytes: MAX_GRAPH_BYTES,
        ..Limits::default()
    }
}
fn validate_roots(roots: &[ObjectId], max_bytes: usize) -> Result<(), TransportError> {
    if roots.is_empty()
        || roots.len() > MAX_GRAPH_OBJECTS
        || roots.iter().collect::<BTreeSet<_>>().len() != roots.len()
        || max_bytes == 0
        || max_bytes > MAX_GRAPH_BYTES
    {
        return Err(TransportError::Limit);
    }
    Ok(())
}
/// Walk a bounded graph once, returning children before parents. Hash/schema
/// validation occurs before any imported object becomes persistent.
fn graph(
    roots: &[ObjectId],
    max_bytes: usize,
    mut load: impl FnMut(ObjectId, usize) -> Result<TransferObject, TransportError>,
) -> Result<(Vec<TransferObject>, u64), TransportError> {
    validate_roots(roots, max_bytes)?;
    struct Walk {
        active: BTreeSet<ObjectId>,
        done: BTreeSet<ObjectId>,
        out: Vec<TransferObject>,
        bytes: usize,
    }
    fn visit(
        id: ObjectId,
        depth: usize,
        max: usize,
        walk: &mut Walk,
        load: &mut impl FnMut(ObjectId, usize) -> Result<TransferObject, TransportError>,
    ) -> Result<(), TransportError> {
        if depth > 64 {
            return Err(TransportError::Limit);
        }
        if walk.done.contains(&id) {
            return Ok(());
        }
        if !walk.active.insert(id) {
            return Err(TransportError::InvalidGraph);
        }
        if walk.active.len() + walk.done.len() > MAX_GRAPH_OBJECTS {
            return Err(TransportError::Limit);
        }
        let object = load(id, max - walk.bytes)?;
        walk.bytes = walk
            .bytes
            .checked_add(object.bytes.len())
            .ok_or(TransportError::Limit)?;
        if walk.bytes > max {
            return Err(TransportError::Limit);
        }
        if object.id != id || object_id(object.kind, object.schema, &object.bytes) != id {
            return Err(TransportError::InvalidGraph);
        }
        let children = references(object.kind, object.schema, &object.bytes, &codec_limits())
            .map_err(|_| TransportError::InvalidGraph)?;
        if children.len() > MAX_GRAPH_OBJECTS {
            return Err(TransportError::Limit);
        }
        for child in children {
            visit(child, depth + 1, max, walk, load)?;
        }
        walk.active.remove(&id);
        walk.done.insert(id);
        walk.out.push(object);
        Ok(())
    }
    let mut walk = Walk {
        active: BTreeSet::new(),
        done: BTreeSet::new(),
        out: Vec::new(),
        bytes: 0,
    };
    for root in roots {
        visit(*root, 1, max_bytes, &mut walk, &mut load)?;
    }
    Ok((walk.out, walk.bytes as u64))
}
impl SqliteObjectRepository {
    pub fn open(path: &Path) -> Result<Self, TransportError> {
        Ok(Self {
            store: Mutex::new(Store::open_with_limits(path, codec_limits())?),
        })
    }
    /// Local application permission: export exactly these roots and their closure.
    /// Root grants and imported roots are durable, scope-specific pins.
    pub fn grant(&self, scope: Scope, roots: &[ObjectId]) -> Result<(), TransportError> {
        self.store
            .lock()
            .map_err(|_| TransportError::Storage)?
            .transaction_with(|tx| {
                graph(roots, MAX_GRAPH_BYTES, |id, remaining| {
                    let size: i64 = tx
                        .sql()
                        .query_row(
                            "SELECT length(bytes) FROM objects WHERE id=?1",
                            [&id.0[..]],
                            |r| r.get(0),
                        )
                        .map_err(StoreError::from)?;
                    if size < 0 || size as u64 > remaining as u64 {
                        return Err(TransportError::Limit);
                    }
                    let o = tx.get_object(id)?;
                    Ok(TransferObject {
                        id,
                        kind: o.kind,
                        schema: o.schema,
                        bytes: o.bytes,
                    })
                })?;
                for root in roots {
                    tx.pin(&owner(scope), *root)?;
                }
                Ok(())
            })
    }
    pub fn revoke(&self, scope: Scope, roots: &[ObjectId]) -> Result<(), TransportError> {
        validate_roots(roots, MAX_GRAPH_BYTES)?;
        self.store
            .lock()
            .map_err(|_| TransportError::Storage)?
            .transaction_with(|tx| {
                for root in roots {
                    tx.unpin(&owner(scope), *root)?;
                }
                Ok(())
            })
    }
}
impl ObjectRepository for SqliteObjectRepository {
    fn export(
        &self,
        scope: Scope,
        roots: &[ObjectId],
        max_bytes: usize,
    ) -> Result<Vec<TransferObject>, TransportError> {
        validate_roots(roots, max_bytes)?;
        self.store
            .lock()
            .map_err(|_| TransportError::Storage)?
            .read_transaction_with(|tx| {
                for root in roots {
                    let granted: bool = tx
                        .sql()
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM root_pins WHERE owner=?1 AND id=?2)",
                            (&owner(scope), &root.0[..]),
                            |r| r.get(0),
                        )
                        .map_err(StoreError::from)?;
                    if !granted {
                        return Err(TransportError::Unauthorized);
                    }
                }
                let (objects, _) = graph(roots, max_bytes, |id, remaining| {
                    // The same read snapshot protects this size check and the blob read.
                    let size: i64 = tx
                        .sql()
                        .query_row(
                            "SELECT length(bytes) FROM objects WHERE id=?1",
                            [&id.0[..]],
                            |r| r.get(0),
                        )
                        .map_err(StoreError::from)?;
                    if size < 0 || size as u64 > remaining as u64 {
                        return Err(TransportError::Limit);
                    }
                    let o = tx.get_object(id)?;
                    Ok(TransferObject {
                        id,
                        kind: o.kind,
                        schema: o.schema,
                        bytes: o.bytes,
                    })
                })?;
                Ok(objects)
            })
    }
    fn import(
        &self,
        scope: Scope,
        roots: &[ObjectId],
        objects: Vec<TransferObject>,
        max_bytes: usize,
    ) -> Result<u64, TransportError> {
        validate_roots(roots, max_bytes)?;
        if objects.len() > MAX_GRAPH_OBJECTS {
            return Err(TransportError::Limit);
        }
        let mut incoming = BTreeMap::new();
        let mut bytes = 0usize;
        for object in objects {
            bytes = bytes
                .checked_add(object.bytes.len())
                .ok_or(TransportError::Limit)?;
            if bytes > max_bytes {
                return Err(TransportError::Limit);
            }
            if incoming.insert(object.id, object).is_some() {
                return Err(TransportError::InvalidGraph);
            }
        }
        let (ordered, bytes) = graph(roots, max_bytes, |id, _remaining| {
            incoming.remove(&id).ok_or(TransportError::InvalidGraph)
        })?;
        if !incoming.is_empty() {
            return Err(TransportError::InvalidGraph);
        }
        self.store
            .lock()
            .map_err(|_| TransportError::Storage)?
            .transaction_with(|tx| {
                for o in &ordered {
                    tx.import(o.id, o.kind, o.schema, &o.bytes)?;
                }
                for root in roots {
                    tx.pin(&owner(scope), *root)?;
                }
                Ok(bytes)
            })
    }
}
