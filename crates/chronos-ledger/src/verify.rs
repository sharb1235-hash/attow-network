//! Bounded event replay into a fresh SQLite database with dispatch absent.
use crate::{
    Authority, LedgerError, MAX_LEDGER_ROWS, MAX_SNAPSHOT_BYTES, fixed, id, num, reducer, schema,
    take, text_value,
};
use chronos_codec::{Limits, OBJECT_KIND, OBJECT_SCHEMA, ObjectId, references};
use chronos_state::{DomainId, ObjectData, StateError, StateReader};
use chronos_store::StoreReadTransaction;
use rusqlite::{Connection, params, types::ValueRef};

const MAX_VERIFY_BYTES: usize = MAX_SNAPSHOT_BYTES * 4;

/// Evidence about the actual local replay, not a signed external checkpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationReport {
    pub events: u64,
    pub head: ObjectId,
    pub state_root: ObjectId,
    pub materialized_tables: usize,
}

/// Stream only objects requested by admitted events from the same SQL snapshot.
/// No global scan or cumulative resident cache can let unrelated staged objects
/// prevent authority recovery. Each read and branch graph remains bounded.
struct SnapshotReader<'a, 'connection>(&'a StoreReadTransaction<'connection>);
impl StateReader for SnapshotReader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let object = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        // Validate direct reference existence even for a rejected proposal whose
        // policy is never used by the reducer. FrozenBranch::reopen independently
        // traverses its complete graph with depth/count/byte limits.
        if (object.kind, object.schema) == (OBJECT_KIND, OBJECT_SCHEMA) {
            let children = references(
                object.kind,
                object.schema,
                &object.bytes,
                &Limits::default(),
            )?;
            if children.len() > chronos_state::MAX_GRAPH_OBJECTS {
                return Err(StateError::Limit("verification references"));
            }
            let mut bytes = object.bytes.len();
            for child in children {
                let child = self.0.get_object(child).map_err(|_| StateError::Storage)?;
                bytes = bytes
                    .checked_add(child.bytes.len())
                    .ok_or(StateError::Limit("verification reference bytes"))?;
                if bytes > chronos_state::MAX_GRAPH_BYTES {
                    return Err(StateError::Limit("verification reference bytes"));
                }
            }
        }
        Ok(ObjectData {
            kind: object.kind,
            schema: object.schema,
            bytes: object.bytes,
        })
    }
}

fn charge(total: &mut usize, amount: usize) -> Result<(), LedgerError> {
    *total = total.checked_add(amount).ok_or(LedgerError::Limit)?;
    if *total > MAX_VERIFY_BYTES {
        return Err(LedgerError::Limit);
    }
    Ok(())
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SqlCell {
    Null,
    Integer(i64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
type SqlRows = Vec<Vec<SqlCell>>;

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Compare exact SQLite types and bytes, with a size guard inside each SELECT.
fn table_rows(connection: &Connection, table: &str) -> Result<SqlRows, LedgerError> {
    let template = connection.prepare(&format!("SELECT * FROM {} LIMIT 0", quoted(table)))?;
    let columns = template.column_names();
    if columns.is_empty() || columns.len() > 16 {
        return Err(LedgerError::Corrupt);
    }
    let column_names: Vec<_> = columns.iter().map(|name| quoted(name)).collect();
    drop(template);
    let oversized: Vec<_> = column_names
        .iter()
        .map(|name| {
            format!("(typeof({name}) IN ('text','blob') AND length(CAST({name} AS BLOB))>?1)")
        })
        .collect();
    let projections: Vec<_> = column_names
        .iter()
        .zip(&oversized)
        .map(|(name, guard)| format!("CASE WHEN {guard} THEN NULL ELSE {name} END"))
        .collect();
    let query = format!(
        "SELECT {}, CASE WHEN {} THEN 1 ELSE 0 END FROM {}",
        projections.join(","),
        oversized.join(" OR "),
        quoted(table)
    );
    let mut statement = connection.prepare(&query)?;
    let mut cursor = statement.query([MAX_SNAPSHOT_BYTES as i64])?;
    let mut result = Vec::new();
    let mut bytes = 0;
    while let Some(row) = cursor.next()? {
        if result.len() >= MAX_LEDGER_ROWS {
            return Err(LedgerError::Limit);
        }
        if row.get::<_, i64>(column_names.len())? != 0 {
            return Err(LedgerError::Limit);
        }
        let mut cells = Vec::with_capacity(column_names.len());
        for index in 0..column_names.len() {
            let cell = match row.get_ref(index)? {
                ValueRef::Null => SqlCell::Null,
                ValueRef::Integer(value) => {
                    charge(&mut bytes, 8)?;
                    SqlCell::Integer(value)
                }
                ValueRef::Text(value) => {
                    charge(&mut bytes, value.len())?;
                    SqlCell::Text(value.to_vec())
                }
                ValueRef::Blob(value) => {
                    charge(&mut bytes, value.len())?;
                    SqlCell::Blob(value.to_vec())
                }
                ValueRef::Real(_) => return Err(LedgerError::Corrupt),
            };
            cells.push(cell);
        }
        result.push(cells);
    }
    result.sort();
    Ok(result)
}

/// Reject corrupted indexes before a mutation could rewrite them from the
/// snapshot. This does not replace full event replay and trusted-head checking.
pub(crate) fn check_materialized(
    connection: &Connection,
    model: &schema::Model,
) -> Result<(), LedgerError> {
    let expected = Connection::open_in_memory()?;
    expected.pragma_update(None, "foreign_keys", true)?;
    expected.execute_batch(schema::DDL)?;
    schema::write(&expected, model)?;
    for table in schema::TABLES {
        if table_rows(connection, table)? != table_rows(&expected, table)? {
            return Err(LedgerError::Corrupt);
        }
    }
    Ok(())
}

impl Authority {
    /// Rebuild from canonical events using no provider, credential or dispatcher.
    /// Compare every materialized authority table against a fresh SQLite database.
    /// All source reads share one snapshot. A caller can retain the returned head
    /// independently to detect rollback; this method cannot authenticate a fully
    /// rewritten database against a checkpoint it has never been given.
    pub fn rebuild_and_verify(&self) -> Result<VerificationReport, LedgerError> {
        self.store.read_transaction_with(|snapshot| {
            let materialized = schema::load(snapshot.sql())?;
            let objects = SnapshotReader(snapshot);
            let rebuilt = Connection::open_in_memory()?;
            rebuilt.pragma_update(None, "foreign_keys", true)?;
            rebuilt.execute_batch(schema::DDL)?;
            let mut replayed = None;
            let mut previous = ObjectId([0; 32]);
            let mut count = 0u64;
            let mut total_bytes = 0;
            let mut statement = snapshot.sql().prepare(
                "SELECT seq,CASE WHEN length(id)=32 THEN id ELSE NULL END,
                 CASE WHEN length(previous)=32 THEN previous ELSE NULL END,
                 CASE WHEN length(bytes)<=?1 THEN bytes ELSE NULL END
                 FROM ledger_events ORDER BY seq",
            )?;
            let mut events = statement.query([Limits::default().object_bytes as i64])?;
            while let Some(row) = events.next()? {
                if count >= MAX_LEDGER_ROWS as u64 {
                    return Err(LedgerError::Limit);
                }
                let sequence: i64 = row.get(0)?;
                let event_id = row
                    .get::<_, Option<Vec<u8>>>(1)?
                    .ok_or(LedgerError::Corrupt)?;
                let declared_previous = row
                    .get::<_, Option<Vec<u8>>>(2)?
                    .ok_or(LedgerError::Corrupt)?;
                let bytes = row
                    .get::<_, Option<Vec<u8>>>(3)?
                    .ok_or(LedgerError::Limit)?;
                charge(&mut total_bytes, bytes.len())?;
                let event_id = ObjectId(event_id.try_into().map_err(|_| LedgerError::Corrupt)?);
                let declared_previous = ObjectId(
                    declared_previous
                        .try_into()
                        .map_err(|_| LedgerError::Corrupt)?,
                );
                let (encoded_sequence, encoded_previous, command) = reducer::decode_event(&bytes)?;
                count += 1;
                if sequence != count as i64
                    || encoded_sequence != count
                    || declared_previous != previous
                    || encoded_previous != previous
                    || reducer::event_id(&bytes) != event_id
                {
                    return Err(LedgerError::Corrupt);
                }
                if replayed.is_none() {
                    let mut fields = fixed(command.clone(), 3)?;
                    if num(take(&mut fields)?)? != 0 {
                        return Err(LedgerError::Corrupt);
                    }
                    let domain = DomainId(id(take(&mut fields)?)?.0);
                    let location = text_value(take(&mut fields)?)?;
                    replayed = Some(schema::Model::new(domain, location));
                }
                let model = replayed.as_mut().ok_or(LedgerError::Corrupt)?;
                reducer::apply(model, &command, &objects)?;
                if model.seq != count {
                    return Err(LedgerError::Corrupt);
                }
                model.head = event_id;
                schema::write(&rebuilt, model)?;
                rebuilt.execute(
                    "INSERT INTO ledger_events(seq,id,previous,bytes) VALUES(?1,?2,?3,?4)",
                    params![sequence, &event_id.0[..], &previous.0[..], &bytes],
                )?;
                previous = event_id;
            }
            let replayed = replayed.ok_or(LedgerError::Corrupt)?;
            if replayed.seq != materialized.seq
                || replayed.head != materialized.head
                || schema::encode(&replayed)? != schema::encode(&materialized)?
            {
                return Err(LedgerError::Corrupt);
            }
            let state_root = replayed.root.object_id()?;
            let root_data = objects.get_object(state_root)?;
            if (root_data.kind, root_data.schema) != (OBJECT_KIND, OBJECT_SCHEMA)
                || root_data.bytes != replayed.root.encode()?
            {
                return Err(LedgerError::Corrupt);
            }
            for table in schema::TABLES {
                if table_rows(snapshot.sql(), table)? != table_rows(&rebuilt, table)? {
                    return Err(LedgerError::Corrupt);
                }
            }
            // Rows were checked individually above; comparison also checks exact
            // persisted event types and rules out unexpected extra columns.
            if table_rows(snapshot.sql(), "ledger_events")?
                != table_rows(&rebuilt, "ledger_events")?
            {
                return Err(LedgerError::Corrupt);
            }
            Ok(VerificationReport {
                events: count,
                head: previous,
                state_root,
                materialized_tables: schema::TABLES.len(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sql_comparison_keeps_types_nulls_and_blob_bytes_distinct() {
        let connection = Connection::open_in_memory().expect("memory database");
        connection.execute_batch("CREATE TABLE values_test(v); INSERT INTO values_test VALUES(NULL),(1),('1'),(x'31')").expect("rows");
        let rows = table_rows(&connection, "values_test").expect("typed rows");
        assert_eq!(rows.len(), 4);
        assert!(rows.windows(2).all(|pair| pair[0] != pair[1]));
    }
    #[test]
    fn sql_comparison_rejects_oversized_cells_before_copy() {
        let connection = Connection::open_in_memory().expect("memory database");
        connection
            .execute_batch("CREATE TABLE values_test(v)")
            .expect("table");
        connection
            .execute(
                "INSERT INTO values_test VALUES(zeroblob(?1))",
                [MAX_SNAPSHOT_BYTES as i64 + 1],
            )
            .expect("large blob");
        assert!(matches!(
            table_rows(&connection, "values_test"),
            Err(LedgerError::Limit)
        ));
    }
}
