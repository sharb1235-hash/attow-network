use std::collections::{BTreeSet, VecDeque};

use chronos_codec::{Limits, ObjectId, references};
use rusqlite::Transaction;

use crate::{DEFAULT_GC_OBJECT_LIMIT, StoreError, sqlite};

/// Resource budgets for one collection pass. Depth counts edges from a root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcLimits {
    pub max_objects: usize,
    pub max_depth: usize,
    pub max_bytes: usize,
}

impl Default for GcLimits {
    fn default() -> Self {
        Self {
            max_objects: DEFAULT_GC_OBJECT_LIMIT,
            max_depth: 1024,
            max_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Counts from one committed collection pass; no byte-erasure guarantee is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcStats {
    pub retained: usize,
    pub deleted: usize,
}

fn enqueue(
    id: ObjectId,
    depth: usize,
    seen: &mut BTreeSet<ObjectId>,
    pending: &mut VecDeque<(ObjectId, usize)>,
    limits: &GcLimits,
) -> Result<(), StoreError> {
    if seen.contains(&id) {
        return Ok(());
    }
    if seen.len() >= limits.max_objects {
        return Err(StoreError::CollectionLimit(limits.max_objects));
    }
    if depth > limits.max_depth {
        return Err(StoreError::CollectionBudget("depth"));
    }
    seen.insert(id);
    pending.push_back((id, depth));
    Ok(())
}

pub(crate) fn collect(
    transaction: &Transaction<'_>,
    limits: &Limits,
    budget: GcLimits,
) -> Result<GcStats, StoreError> {
    // The caller owns an IMMEDIATE transaction before the root read through sweep.
    let mut seen = BTreeSet::new();
    let mut pending = VecDeque::new();
    {
        let mut roots = transaction.prepare("SELECT DISTINCT id FROM root_pins ORDER BY id")?;
        let mut rows = roots.query([])?;
        while let Some(row) = rows.next()? {
            let bytes = row
                .get_ref(0)?
                .as_blob()
                .map_err(|_| StoreError::WrongDatabase)?;
            let id = ObjectId(bytes.try_into().map_err(|_| StoreError::WrongDatabase)?);
            enqueue(id, 0, &mut seen, &mut pending, &budget)?;
        }
    }
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS attow_gc_live (id BLOB PRIMARY KEY) WITHOUT ROWID;
         DELETE FROM temp.attow_gc_live;",
    )?;
    let mut total_bytes = 0_usize;
    while let Some((id, depth)) = pending.pop_front() {
        let object = sqlite::get(transaction, id, limits)?;
        total_bytes = total_bytes
            .checked_add(object.bytes.len())
            .ok_or(StoreError::CollectionBudget("bytes"))?;
        if total_bytes > budget.max_bytes {
            return Err(StoreError::CollectionBudget("bytes"));
        }
        // Unknown schemas and broken children abort before any sweep.
        for child in references(object.kind, object.schema, &object.bytes, limits)? {
            let next_depth = depth
                .checked_add(1)
                .ok_or(StoreError::CollectionBudget("depth"))?;
            enqueue(child, next_depth, &mut seen, &mut pending, &budget)?;
        }
        transaction.execute(
            "INSERT INTO temp.attow_gc_live (id) VALUES (?1)",
            [&id.0[..]],
        )?;
    }
    let deleted = transaction.execute(
        "DELETE FROM objects WHERE NOT EXISTS (SELECT 1 FROM temp.attow_gc_live WHERE attow_gc_live.id=objects.id)", [],
    )?;
    transaction.execute("DELETE FROM temp.attow_gc_live", [])?;
    Ok(GcStats {
        retained: seen.len(),
        deleted,
    })
}
