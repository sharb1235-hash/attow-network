use rusqlite::{Connection, TransactionBehavior, params};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub type Digest = [u8; 32];
type Cell = (i64, Option<Vec<u8>>);
type Cells = BTreeMap<String, Cell>;

#[derive(Debug)]
pub enum Error {
    Sql(rusqlite::Error),
    Conflict(String),
    Invalid(String),
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Snapshot {
    cells: Arc<Cells>,
    epoch: i64,
    authority_id: Digest,
}

/// Context values are structurally shared. This is not a live-process clone.
pub struct Branch {
    base: Snapshot,
    reads: BTreeMap<String, i64>,
    writes: BTreeMap<String, Option<Vec<u8>>>,
    scanned: bool,
    intent: Option<Intent>,
}

#[derive(Clone, Debug)]
pub struct Intent {
    pub id: String,
    /// Identifies the one logical business action, shared by competing branches.
    pub slot: String,
    pub request: Vec<u8>,
}

impl Snapshot {
    pub fn fork(&self) -> Branch {
        Branch {
            base: self.clone(),
            reads: BTreeMap::new(),
            writes: BTreeMap::new(),
            scanned: false,
            intent: None,
        }
    }
}
impl Branch {
    fn track(&mut self, key: &str) {
        let version = self.base.cells.get(key).map_or(0, |c| c.0);
        self.reads.entry(key.into()).or_insert(version);
    }
    pub fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        self.track(key);
        self.writes
            .get(key)
            .cloned()
            .unwrap_or_else(|| self.base.cells.get(key).and_then(|c| c.1.clone()))
    }
    pub fn put(&mut self, key: &str, value: Vec<u8>) {
        self.track(key);
        self.writes.insert(key.into(), Some(value));
    }
    pub fn delete(&mut self, key: &str) {
        self.track(key);
        self.writes.insert(key.into(), None);
    }
    /// Conservative namespace-wide phantom protection in this reference model.
    pub fn scan_prefix(&mut self, prefix: &str) -> BTreeMap<String, Vec<u8>> {
        self.scanned = true;
        let mut out: BTreeMap<_, _> = self
            .base
            .cells
            .iter()
            .filter_map(|(k, (_, v))| {
                if k.starts_with(prefix) {
                    v.clone().map(|v| (k.clone(), v))
                } else {
                    None
                }
            })
            .collect();
        for (k, v) in &self.writes {
            if k.starts_with(prefix) {
                if let Some(v) = v {
                    out.insert(k.clone(), v.clone());
                } else {
                    out.remove(k);
                }
            }
        }
        out
    }
    pub fn stage(&mut self, intent: Intent) -> Result<()> {
        if self.intent.is_some() || intent.id.is_empty() || intent.slot.is_empty() {
            return Err(Error::Invalid(
                "one nonempty effect intent per branch in reference model".into(),
            ));
        }
        self.intent = Some(intent);
        Ok(())
    }
}

pub struct Authority {
    conn: Connection,
}
#[derive(Debug, Clone)]
pub struct Promotion {
    pub epoch: i64,
    pub digest: Digest,
    pub effect_pending: bool,
}

fn append_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}
fn encode_cell(out: &mut Vec<u8>, key: &str, version: i64, value: &Option<Vec<u8>>) {
    append_bytes(out, key.as_bytes());
    out.extend_from_slice(&version.to_le_bytes());
    match value {
        Some(v) => {
            out.push(1);
            append_bytes(out, v);
        }
        None => out.push(0),
    }
}
fn state_digest(cells: &Cells) -> Digest {
    let mut bytes = b"chronos-reference-state-v1\0".to_vec();
    bytes.extend_from_slice(&(cells.len() as u64).to_le_bytes());
    for (key, (version, value)) in cells {
        encode_cell(&mut bytes, key, *version, value);
    }
    *blake3::hash(&bytes).as_bytes()
}
fn load_cells(conn: &Connection) -> Result<Cells> {
    let mut stmt = conn.prepare("SELECT key,version,value FROM kv ORDER BY key")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (r.get::<_, i64>(1)?, r.get::<_, Option<Vec<u8>>>(2)?),
        ))
    })?;
    let mut cells = Cells::new();
    for row in rows {
        let (key, cell) = row?;
        cells.insert(key, cell);
    }
    Ok(cells)
}
impl Authority {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        // Serialize initialization and migration across independent connections.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch("
            CREATE TABLE IF NOT EXISTS meta(id INTEGER PRIMARY KEY CHECK(id=1),epoch INTEGER NOT NULL,head BLOB NOT NULL);
            INSERT OR IGNORE INTO meta(id,epoch,head) VALUES(1,0,zeroblob(32));
            CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY,version INTEGER NOT NULL,value BLOB);
            CREATE TABLE IF NOT EXISTS commits(epoch INTEGER PRIMARY KEY,parent BLOB NOT NULL,digest BLOB NOT NULL UNIQUE,payload BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS effects(id TEXT PRIMARY KEY,slot TEXT NOT NULL UNIQUE,state TEXT NOT NULL,request BLOB NOT NULL,receipt BLOB);")?;
        let has_authority_id = {
            let mut stmt = tx.prepare("PRAGMA table_info(meta)")?;
            let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
            let mut found = false;
            for name in names {
                found |= name? == "authority_id";
            }
            found
        };
        if !has_authority_id {
            tx.execute_batch("ALTER TABLE meta ADD COLUMN authority_id BLOB;")?;
        }
        tx.execute(
            "UPDATE meta SET authority_id=randomblob(32) WHERE authority_id IS NULL",
            [],
        )?;
        tx.commit()?;
        Ok(Self { conn })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        // A read transaction keeps the epoch and row set from one SQLite snapshot.
        let tx = self.conn.unchecked_transaction()?;
        let (epoch, authority_id): (i64, Vec<u8>) =
            tx.query_row("SELECT epoch,authority_id FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let authority_id = authority_id
            .try_into()
            .map_err(|_| Error::Invalid("invalid authority identity".into()))?;
        let cells = load_cells(&tx)?;
        tx.commit()?;
        Ok(Snapshot {
            cells: Arc::new(cells),
            epoch,
            authority_id,
        })
    }
    pub fn promote(&mut self, branch: Branch) -> Result<Promotion> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (epoch, parent, authority_id): (i64, Vec<u8>, Vec<u8>) = tx.query_row(
            "SELECT epoch,head,authority_id FROM meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if authority_id != branch.base.authority_id {
            return Err(Error::Invalid("branch belongs to another authority".into()));
        }
        if branch.scanned && epoch != branch.base.epoch {
            return Err(Error::Conflict("scan epoch changed".into()));
        }
        let mut cells = load_cells(&tx)?;
        for (key, expected) in &branch.reads {
            if cells.get(key).map_or(0, |c| c.0) != *expected {
                return Err(Error::Conflict(key.clone()));
            }
        }
        if let Some(i) = &branch.intent {
            let occupied: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM effects WHERE slot=?1 OR id=?2)",
                params![i.slot, i.id],
                |r| r.get(0),
            )?;
            if occupied {
                return Err(Error::Conflict(i.slot.clone()));
            }
        }
        let next_epoch = epoch
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("epoch exhausted".into()))?;
        let mut payload = b"chronos-reference-commit-v1\0".to_vec();
        payload.extend_from_slice(&parent);
        payload.extend_from_slice(&next_epoch.to_le_bytes());
        payload.extend_from_slice(&(branch.writes.len() as u64).to_le_bytes());
        for (key, value) in &branch.writes {
            cells.insert(key.clone(), (next_epoch, value.clone()));
            encode_cell(&mut payload, key, next_epoch, value);
            tx.execute("INSERT INTO kv VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET version=excluded.version,value=excluded.value", params![key,next_epoch,value])?;
        }
        if let Some(i) = &branch.intent {
            payload.push(1);
            append_bytes(&mut payload, i.id.as_bytes());
            append_bytes(&mut payload, i.slot.as_bytes());
            append_bytes(&mut payload, &i.request);
            tx.execute(
                "INSERT INTO effects VALUES(?1,?2,'PENDING',?3,NULL)",
                params![i.id, i.slot, i.request],
            )?;
        } else {
            payload.push(0);
        }
        payload.extend_from_slice(&state_digest(&cells));
        let digest = *blake3::hash(&payload).as_bytes();
        tx.execute(
            "INSERT INTO commits VALUES(?1,?2,?3,?4)",
            params![next_epoch, parent, digest.as_slice(), payload],
        )?;
        tx.execute(
            "UPDATE meta SET epoch=?1,head=?2 WHERE id=1",
            params![next_epoch, digest.as_slice()],
        )?;
        tx.commit()?;
        Ok(Promotion {
            epoch: next_epoch,
            digest,
            effect_pending: branch.intent.is_some(),
        })
    }
    pub fn effect_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM effects", [], |r| r.get(0))?)
    }
    pub fn effect_state(&self, id: &str) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT state FROM effects WHERE id=?1", [id], |r| r.get(0))?)
    }
    /// Durable transition before any external send. No actual dispatch is provided.
    pub fn mark_dispatching(&mut self, id: &str) -> Result<()> {
        if self.conn.execute(
            "UPDATE effects SET state='DISPATCHING' WHERE id=?1 AND state='PENDING'",
            [id],
        )? != 1
        {
            return Err(Error::Invalid("not pending".into()));
        }
        Ok(())
    }
    /// After restart, a dispatched effect may already have happened externally.
    pub fn recover_ambiguous(&mut self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE effects SET state='UNKNOWN' WHERE state='DISPATCHING'",
            [],
        )?)
    }
    pub fn confirm(&mut self, id: &str, receipt: &[u8]) -> Result<()> {
        if self.conn.execute("UPDATE effects SET state='CONFIRMED',receipt=?2 WHERE id=?1 AND state IN ('DISPATCHING','UNKNOWN')",params![id,receipt])? != 1 { return Err(Error::Invalid("not reconcilable".into())); }
        Ok(())
    }
    pub fn verify_chain(&self) -> Result<Digest> {
        // The log and its head must come from the same read snapshot even when
        // another independently opened Authority commits during verification.
        let tx = self.conn.unchecked_transaction()?;
        let (head, epoch) = {
            let mut stmt =
                tx.prepare("SELECT epoch,parent,digest,payload FROM commits ORDER BY epoch")?;
            let mut rows = stmt.query([])?;
            let mut head = [0u8; 32];
            let mut epoch = 0i64;
            while let Some(row) = rows.next()? {
                let e: i64 = row.get(0)?;
                let p: Vec<u8> = row.get(1)?;
                let d: Vec<u8> = row.get(2)?;
                let payload: Vec<u8> = row.get(3)?;
                if e != epoch + 1 || p != head || d != blake3::hash(&payload).as_bytes() {
                    return Err(Error::Invalid("ledger chain mismatch".into()));
                }
                head.copy_from_slice(&d);
                epoch = e;
            }
            (head, epoch)
        };
        let (recorded_epoch, recorded_head): (i64, Vec<u8>) =
            tx.query_row("SELECT epoch,head FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if recorded_epoch != epoch || recorded_head != head {
            return Err(Error::Invalid("head mismatch".into()));
        }
        tx.commit()?;
        Ok(head)
    }
}
