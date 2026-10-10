use crate::promotion::{decode_policy, encode_policy, proposal_from_value, proposal_value};
use crate::*;
use chronos_codec::{Canonical, Limits};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;

pub(crate) const TABLES: &[&str] = &[
    "ledger_meta",
    "ledger_cells",
    "ledger_rights",
    "ledger_decisions",
    "ledger_promotions",
    "ledger_intents",
    "ledger_outbox",
    "ledger_evidence",
    "ledger_slots",
    "ledger_policies",
];
pub(crate) const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),domain BLOB NOT NULL,epoch INTEGER NOT NULL,root BLOB NOT NULL,seq INTEGER NOT NULL,head BLOB NOT NULL,policy BLOB,state BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS ledger_cells(key BLOB PRIMARY KEY,version INTEGER NOT NULL CHECK(version>0),value BLOB);
CREATE TABLE IF NOT EXISTS ledger_rights(name TEXT PRIMARY KEY,granted INTEGER NOT NULL CHECK(granted>=0),reserved INTEGER NOT NULL CHECK(reserved>=0),spent INTEGER NOT NULL CHECK(spent>=0),CHECK(reserved+spent<=granted));
CREATE TABLE IF NOT EXISTS ledger_decisions(id BLOB PRIMARY KEY,generation INTEGER NOT NULL CHECK(generation>=0),winner BLOB);
CREATE TABLE IF NOT EXISTS ledger_promotions(id BLOB PRIMARY KEY,payload BLOB NOT NULL,fingerprint BLOB NOT NULL,response BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS ledger_intents(id BLOB PRIMARY KEY,contract BLOB NOT NULL,provider TEXT NOT NULL,slot TEXT NOT NULL,operation BLOB NOT NULL,right_name TEXT NOT NULL,cost INTEGER NOT NULL CHECK(cost>0),body BLOB NOT NULL,state INTEGER NOT NULL CHECK(state BETWEEN 0 AND 5),attempt INTEGER NOT NULL,epoch INTEGER NOT NULL,UNIQUE(contract,provider,operation));
CREATE TABLE IF NOT EXISTS ledger_outbox(intent BLOB PRIMARY KEY REFERENCES ledger_intents(id),state INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ledger_evidence(intent BLOB NOT NULL REFERENCES ledger_intents(id),ordinal INTEGER NOT NULL,status INTEGER NOT NULL,receipt BLOB NOT NULL,attempt INTEGER NOT NULL,PRIMARY KEY(intent,ordinal));
CREATE TABLE IF NOT EXISTS ledger_slots(slot TEXT PRIMARY KEY,intent BLOB NOT NULL REFERENCES ledger_intents(id));
CREATE TABLE IF NOT EXISTS ledger_policies(id BLOB PRIMARY KEY,bytes BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS ledger_events(seq INTEGER PRIMARY KEY CHECK(seq>0),id BLOB NOT NULL UNIQUE,previous BLOB NOT NULL,bytes BLOB NOT NULL);
"#;

#[derive(Clone, Debug)]
pub(crate) struct Right {
    pub granted: u64,
    pub reserved: u64,
    pub spent: u64,
}
#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub generation: u64,
    pub winner: Option<ObjectId>,
}
#[derive(Clone, Debug)]
pub(crate) struct Dedup {
    pub payload: ObjectId,
    pub fingerprint: ObjectId,
    pub response: PromotionOutcome,
}
#[derive(Clone, Debug)]
pub(crate) struct Intent {
    pub proposal: EffectProposal,
    pub state: EffectState,
    pub attempt: u64,
    pub epoch: u64,
}
#[derive(Clone, Debug)]
pub(crate) struct Evidence {
    pub intent: ObjectId,
    pub attempt: u64,
    pub status: u64,
    pub receipt: ObjectId,
}
#[derive(Clone, Debug)]
pub(crate) struct Model {
    pub location: String,
    pub writer: Option<ObjectId>,
    pub next_writer: Option<ObjectId>,
    pub epoch: u64,
    pub root: StateRoot,
    pub seq: u64,
    pub head: ObjectId,
    pub policy: Option<ObjectId>,
    pub policies: BTreeMap<ObjectId, Policy>,
    pub rights: BTreeMap<String, Right>,
    pub decisions: BTreeMap<ObjectId, Decision>,
    pub promotions: BTreeMap<ObjectId, Dedup>,
    pub intents: BTreeMap<ObjectId, Intent>,
    pub evidence: Vec<Evidence>,
}
impl Model {
    pub fn new(domain: DomainId, location: String) -> Self {
        Self {
            location,
            writer: None,
            next_writer: None,
            epoch: 1,
            root: StateRoot::new(domain),
            seq: 0,
            head: ObjectId([0; 32]),
            policy: None,
            policies: BTreeMap::new(),
            rights: BTreeMap::new(),
            decisions: BTreeMap::new(),
            promotions: BTreeMap::new(),
            intents: BTreeMap::new(),
            evidence: Vec::new(),
        }
    }
    pub fn check(&self) -> Result<(), LedgerError> {
        integer(self.epoch)?;
        if self.epoch == 0
            || (self.writer.is_none() && self.next_writer.is_some())
            || (self.next_writer.is_some() && self.next_writer == self.writer)
        {
            return Err(LedgerError::Corrupt);
        }
        if [
            self.policies.len(),
            self.rights.len(),
            self.decisions.len(),
            self.promotions.len(),
            self.intents.len(),
            self.evidence.len(),
        ]
        .into_iter()
        .any(|count| count > MAX_LEDGER_ROWS)
            || self.seq > MAX_LEDGER_ROWS as u64
        {
            return Err(LedgerError::Limit);
        }
        // Existing effects must retain room for a claim, possible recovery, and
        // a terminal receipt. Administrative events and new proposals may only
        // consume capacity beyond these mandatory lifecycle transitions.
        let required_events = self
            .seq
            .checked_add(self.reserved_lifecycle_events()?)
            .ok_or(LedgerError::Limit)?;
        if required_events > MAX_LEDGER_ROWS as u64 {
            return Err(LedgerError::Limit);
        }
        for right in self.rights.values() {
            integer(right.granted)?;
            if right
                .reserved
                .checked_add(right.spent)
                .is_none_or(|spent| spent > right.granted)
            {
                return Err(LedgerError::Corrupt);
            }
        }
        Ok(())
    }
    fn reserved_lifecycle_events(&self) -> Result<u64, LedgerError> {
        // Closing must leave activation plus the next active epoch's reserve.
        // A close that cannot fit is rejected while the old writer can still
        // complete its pending effects. This is a finite log, not log rotation.
        let federation: u64 = if self.writer.is_none() {
            0
        } else if self.next_writer.is_some() {
            3
        } else {
            2
        };
        self.intents.values().try_fold(federation, |total, intent| {
            let remaining = match intent.state {
                EffectState::Authorized => 3,
                EffectState::Dispatching => 2,
                EffectState::Unknown => 1,
                EffectState::Confirmed
                | EffectState::DefinitivelyRejected
                | EffectState::CancelledBeforeSend => 0,
            };
            total.checked_add(remaining).ok_or(LedgerError::Limit)
        })
    }
}

fn snapshot_limits() -> Limits {
    Limits {
        object_bytes: MAX_SNAPSHOT_BYTES,
        depth: 64,
        items: 1_000_000,
    }
}
pub(crate) fn outcome_value(value: &PromotionOutcome) -> Value {
    match value {
        PromotionOutcome::LocalCommitted { seq } => {
            Value::Array(vec![Value::Unsigned(0), Value::Unsigned(*seq)])
        }
        PromotionOutcome::AcceptedPending { seq, intent } => Value::Array(vec![
            Value::Unsigned(1),
            Value::Unsigned(*seq),
            v_id(*intent),
        ]),
        PromotionOutcome::Conflict(report) => Value::Array(vec![
            Value::Unsigned(2),
            Value::Array(report.reasons.iter().cloned().map(Value::Text).collect()),
        ]),
    }
}
pub(crate) fn outcome_from_value(value: Value) -> Result<PromotionOutcome, LedgerError> {
    let mut fields = list(value)?.into_iter();
    let code = num(take(&mut fields)?)?;
    let result = match code {
        0 => PromotionOutcome::LocalCommitted {
            seq: num(take(&mut fields)?)?,
        },
        1 => PromotionOutcome::AcceptedPending {
            seq: num(take(&mut fields)?)?,
            intent: id(take(&mut fields)?)?,
        },
        2 => PromotionOutcome::Conflict(ConflictReport {
            reasons: list(take(&mut fields)?)?
                .into_iter()
                .map(text_value)
                .collect::<Result<_, _>>()?,
        }),
        _ => return Err(LedgerError::Corrupt),
    };
    if fields.next().is_some() {
        return Err(LedgerError::Corrupt);
    }
    Ok(result)
}
pub(crate) fn encode(model: &Model) -> Result<Vec<u8>, LedgerError> {
    model.check()?;
    // Every mandatory transition releases a unit before encoding its result.
    // A unit covers a receipt's five values plus sequence/array headers and
    // right-counter growth; the raw receipt is stored separately in the CAS.
    let reserved =
        usize::try_from(model.reserved_lifecycle_events()?).map_err(|_| LedgerError::Limit)?;
    let mut limits = snapshot_limits();
    limits.object_bytes = limits
        .object_bytes
        .checked_sub(reserved.checked_mul(256).ok_or(LedgerError::Limit)?)
        .ok_or(LedgerError::Limit)?;
    if model.next_writer.is_some() {
        // The successor's canonical database path may grow up to the admitted
        // 4096-byte bound; closing cannot strand activation at the byte cap.
        limits.object_bytes = limits
            .object_bytes
            .checked_sub(4096)
            .ok_or(LedgerError::Limit)?;
    }
    limits.items = limits
        .items
        .checked_sub(reserved.checked_mul(16).ok_or(LedgerError::Limit)?)
        .ok_or(LedgerError::Limit)?;
    let policies = model
        .policies
        .iter()
        .map(|(key, value)| {
            Ok(Value::Array(vec![
                v_id(*key),
                Value::Bytes(encode_policy(value)?),
            ]))
        })
        .collect::<Result<_, LedgerError>>()?;
    let rights = model
        .rights
        .iter()
        .map(|(name, r)| {
            Value::Array(vec![
                Value::Text(name.clone()),
                Value::Unsigned(r.granted),
                Value::Unsigned(r.reserved),
                Value::Unsigned(r.spent),
            ])
        })
        .collect();
    let decisions = model
        .decisions
        .iter()
        .map(|(id, d)| {
            Value::Array(vec![
                v_id(*id),
                Value::Unsigned(d.generation),
                v_option(d.winner),
            ])
        })
        .collect();
    let promotions = model
        .promotions
        .iter()
        .map(|(id, d)| {
            Value::Array(vec![
                v_id(*id),
                v_id(d.payload),
                v_id(d.fingerprint),
                outcome_value(&d.response),
            ])
        })
        .collect();
    let intents = model
        .intents
        .iter()
        .map(|(id, i)| {
            Value::Array(vec![
                v_id(*id),
                proposal_value(&i.proposal),
                Value::Unsigned(i.state.code()),
                Value::Unsigned(i.attempt),
                Value::Unsigned(i.epoch),
            ])
        })
        .collect();
    let evidence = model
        .evidence
        .iter()
        .map(|e| {
            Value::Array(vec![
                v_id(e.intent),
                Value::Unsigned(e.attempt),
                Value::Unsigned(e.status),
                v_id(e.receipt),
            ])
        })
        .collect();
    let mut values = vec![
        Value::Unsigned(if model.writer.is_some() { 2 } else { 1 }),
        Value::Bytes(model.root.domain().0.to_vec()),
        Value::Text(model.location.clone()),
        Value::Unsigned(model.epoch),
        Value::Bytes(model.root.encode()?),
        Value::Unsigned(model.seq),
        v_id(model.head),
        v_option(model.policy),
        Value::Array(policies),
        Value::Array(rights),
        Value::Array(decisions),
        Value::Array(promotions),
        Value::Array(intents),
        Value::Array(evidence),
    ];
    if let Some(writer) = model.writer {
        values.push(v_id(writer));
        values.push(v_option(model.next_writer));
    }
    Ok(Value::Array(values).encode_with_limits(&limits)?)
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Model, LedgerError> {
    let fields = list(Value::decode(bytes, &snapshot_limits())?)?;
    let count = fields.len();
    let mut f = fields.into_iter();
    let version = num(take(&mut f)?)?;
    if !matches!((version, count), (1, 14) | (2, 16)) {
        return Err(LedgerError::Corrupt);
    }
    let domain = DomainId(id(take(&mut f)?)?.0);
    let location = text_value(take(&mut f)?)?;
    let mut m = Model::new(domain, location);
    m.epoch = num(take(&mut f)?)?;
    m.root = StateRoot::decode(&blob(take(&mut f)?)?, &Limits::default())?;
    if m.root.domain() != domain {
        return Err(LedgerError::Corrupt);
    }
    m.seq = num(take(&mut f)?)?;
    m.head = id(take(&mut f)?)?;
    m.policy = option_id(take(&mut f)?)?;
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 2)?;
        let key = id(take(&mut r)?)?;
        let policy = decode_policy(&blob(take(&mut r)?)?)?;
        if m.policies.insert(key, policy).is_some() {
            return Err(LedgerError::Corrupt);
        }
    }
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 4)?;
        let name = text_value(take(&mut r)?)?;
        let right = Right {
            granted: num(take(&mut r)?)?,
            reserved: num(take(&mut r)?)?,
            spent: num(take(&mut r)?)?,
        };
        if m.rights.insert(name, right).is_some() {
            return Err(LedgerError::Corrupt);
        }
    }
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 3)?;
        let key = id(take(&mut r)?)?;
        let d = Decision {
            generation: num(take(&mut r)?)?,
            winner: option_id(take(&mut r)?)?,
        };
        if m.decisions.insert(key, d).is_some() {
            return Err(LedgerError::Corrupt);
        }
    }
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 4)?;
        let key = id(take(&mut r)?)?;
        let d = Dedup {
            payload: id(take(&mut r)?)?,
            fingerprint: id(take(&mut r)?)?,
            response: outcome_from_value(take(&mut r)?)?,
        };
        if m.promotions.insert(key, d).is_some() {
            return Err(LedgerError::Corrupt);
        }
    }
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 5)?;
        let key = id(take(&mut r)?)?;
        let i = Intent {
            proposal: proposal_from_value(take(&mut r)?)?,
            state: EffectState::from_code(num(take(&mut r)?)?)?,
            attempt: num(take(&mut r)?)?,
            epoch: num(take(&mut r)?)?,
        };
        if m.intents.insert(key, i).is_some() {
            return Err(LedgerError::Corrupt);
        }
    }
    for row in list(take(&mut f)?)? {
        let mut r = fixed(row, 4)?;
        m.evidence.push(Evidence {
            intent: id(take(&mut r)?)?,
            attempt: num(take(&mut r)?)?,
            status: num(take(&mut r)?)?,
            receipt: id(take(&mut r)?)?,
        });
    }
    if version == 2 {
        m.writer = Some(id(take(&mut f)?)?);
        m.next_writer = option_id(take(&mut f)?)?;
    }
    m.check()?;
    if encode(&m)? != bytes {
        return Err(LedgerError::Corrupt);
    }
    Ok(m)
}
pub(crate) fn load(connection: &Connection) -> Result<Model, LedgerError> {
    let bytes: Vec<u8> = connection.query_row(
        "SELECT state FROM ledger_meta WHERE singleton=1 AND length(state)<=?1",
        [MAX_SNAPSHOT_BYTES as i64],
        |row| row.get(0),
    )?;
    let model = decode(&bytes)?;
    crate::verify::check_materialized(connection, &model)?;
    Ok(model)
}
pub(crate) fn exists(connection: &Connection) -> Result<bool, LedgerError> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM ledger_meta WHERE singleton=1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}
pub(crate) fn write(connection: &Connection, m: &Model) -> Result<(), LedgerError> {
    m.check()?;
    // Delete dependants first; immutable events are never rewritten here.
    for table in [
        "ledger_evidence",
        "ledger_outbox",
        "ledger_slots",
        "ledger_intents",
        "ledger_promotions",
        "ledger_decisions",
        "ledger_rights",
        "ledger_cells",
        "ledger_policies",
        "ledger_meta",
    ] {
        connection.execute(&format!("DELETE FROM {table}"), [])?;
    }
    let state = encode(m)?;
    let root = m.root.object_id()?;
    connection.execute(
        "INSERT INTO ledger_meta VALUES(1,?1,?2,?3,?4,?5,?6,?7)",
        params![
            &m.root.domain().0[..],
            integer(m.epoch)?,
            &root.0[..],
            integer(m.seq)?,
            &m.head.0[..],
            m.policy.map(|id| id.0.to_vec()),
            state
        ],
    )?;
    let object = chronos_codec::Object::decode(&m.root.encode()?, &Limits::default())?;
    let Value::Array(fields) = object.payload else {
        return Err(LedgerError::Corrupt);
    };
    let cells = fields.into_iter().nth(5).ok_or(LedgerError::Corrupt)?;
    for row in list(cells)? {
        let mut row = fixed(row, 3)?;
        let key = blob(take(&mut row)?)?;
        let version = num(take(&mut row)?)?;
        let value = match take(&mut row)? {
            Value::Null => None,
            v => Some(blob(v)?),
        };
        connection.execute(
            "INSERT INTO ledger_cells VALUES(?1,?2,?3)",
            params![key, integer(version)?, value],
        )?;
    }
    for (name, r) in &m.rights {
        connection.execute(
            "INSERT INTO ledger_rights VALUES(?1,?2,?3,?4)",
            params![
                name,
                integer(r.granted)?,
                integer(r.reserved)?,
                integer(r.spent)?
            ],
        )?;
    }
    for (id, d) in &m.decisions {
        connection.execute(
            "INSERT INTO ledger_decisions VALUES(?1,?2,?3)",
            params![
                &id.0[..],
                integer(d.generation)?,
                d.winner.map(|id| id.0.to_vec())
            ],
        )?;
    }
    for (id, d) in &m.promotions {
        connection.execute(
            "INSERT INTO ledger_promotions VALUES(?1,?2,?3,?4)",
            params![
                &id.0[..],
                &d.payload.0[..],
                &d.fingerprint.0[..],
                outcome_value(&d.response).encode()?
            ],
        )?;
    }
    for (id, i) in &m.intents {
        let p = &i.proposal;
        connection.execute(
            "INSERT INTO ledger_intents VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                &id.0[..],
                &p.contract.0[..],
                p.provider,
                p.semantic_slot,
                &p.operation.0[..],
                p.right,
                integer(p.cost)?,
                p.body,
                integer(i.state.code())?,
                integer(i.attempt)?,
                integer(i.epoch)?
            ],
        )?;
        connection.execute(
            "INSERT INTO ledger_outbox VALUES(?1,?2)",
            params![&id.0[..], integer(i.state.code())?],
        )?;
        if matches!(
            i.state,
            EffectState::Authorized
                | EffectState::Dispatching
                | EffectState::Unknown
                | EffectState::Confirmed
        ) {
            connection.execute(
                "INSERT INTO ledger_slots VALUES(?1,?2)",
                params![p.semantic_slot, &id.0[..]],
            )?;
        }
    }
    for (index, e) in m.evidence.iter().enumerate() {
        connection.execute(
            "INSERT INTO ledger_evidence VALUES(?1,?2,?3,?4,?5)",
            params![
                &e.intent.0[..],
                index as i64,
                integer(e.status)?,
                &e.receipt.0[..],
                integer(e.attempt)?
            ],
        )?;
    }
    for (id, policy) in &m.policies {
        connection.execute(
            "INSERT INTO ledger_policies VALUES(?1,?2)",
            params![&id.0[..], encode_policy(policy)?],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    use chronos_codec::{OBJECT_KIND, OBJECT_SCHEMA, Object, object_id};
    use chronos_state::{ObjectData, StateReader};

    struct Receipt(ObjectId, Vec<u8>);
    impl StateReader for Receipt {
        fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
            if id != self.0 {
                return Err(StateError::Storage);
            }
            Ok(ObjectData {
                kind: OBJECT_KIND,
                schema: OBJECT_SCHEMA,
                bytes: self.1.clone(),
            })
        }
    }
    fn receipt(id: ObjectId, intent: &Intent, status: u64) -> Result<Receipt, LedgerError> {
        let bytes = Object {
            refs: BTreeSet::new(),
            payload: Value::Array(vec![
                Value::Text("attow-evidence".into()),
                Value::Unsigned(1),
                v_id(id),
                v_id(intent.proposal.contract),
                v_id(intent.proposal.operation),
                v_id(object_id(0x4244, 1, &intent.proposal.body)),
                Value::Unsigned(intent.attempt),
                Value::Unsigned(intent.epoch),
                Value::Unsigned(status),
                Value::Bytes(b"owned-boundary-fixture".to_vec()),
            ]),
        }
        .encode()?;
        Ok(Receipt(
            object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes),
            bytes,
        ))
    }

    fn model_with(states: &[EffectState], seq: u64) -> Model {
        let mut model = Model::new(DomainId([1; 32]), "owned-test".into());
        model.seq = seq;
        for (index, state) in states.iter().enumerate() {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
            model.intents.insert(
                ObjectId(bytes),
                Intent {
                    proposal: EffectProposal {
                        contract: ObjectId([2; 32]),
                        provider: "owned-test".into(),
                        semantic_slot: format!("slot-{index}"),
                        operation: ObjectId(bytes),
                        right: "budget".into(),
                        cost: 1,
                        body: vec![],
                    },
                    state: *state,
                    attempt: 0,
                    epoch: 1,
                },
            );
        }
        model
    }

    fn count_items(value: &Value) -> usize {
        1 + match value {
            Value::Array(values) => values.iter().map(count_items).sum(),
            _ => 0,
        }
    }

    #[test]
    fn close_admission_preserves_activation_and_unknown_completion_capacity()
    -> Result<(), LedgerError> {
        let empty_reader = Receipt(ObjectId([255; 32]), vec![]);
        let close = Value::Array(vec![Value::Unsigned(10), v_id(ObjectId([8; 32]))]);
        let activate = Value::Array(vec![
            Value::Unsigned(11),
            v_id(ObjectId([8; 32])),
            Value::Unsigned(2),
            Value::Text("owned-new".into()),
        ]);
        let mut too_full = model_with(&[EffectState::Unknown], MAX_LEDGER_ROWS as u64 - 4);
        too_full.writer = Some(ObjectId([7; 32]));
        assert!(
            matches!(
                crate::reducer::apply(&mut too_full, &close, &empty_reader),
                Err(LedgerError::Limit)
            ),
            "closing would strand activation and the existing Unknown outcome"
        );
        let mut fits = model_with(&[EffectState::Unknown], MAX_LEDGER_ROWS as u64 - 5);
        fits.writer = Some(ObjectId([7; 32]));
        crate::reducer::apply(&mut fits, &close, &empty_reader)?;
        encode(&fits)?;
        crate::reducer::apply(&mut fits, &activate, &empty_reader)?;
        encode(&fits)?;
        assert_eq!(fits.epoch, 2);
        Ok(())
    }

    #[test]
    fn snapshot_admission_preserves_terminal_metadata_byte_capacity() -> Result<(), LedgerError> {
        let mut model = model_with(&[EffectState::Authorized], 4093);
        model.location.clear();
        let baseline = encode(&model)?.len();
        // Synthetic padding isolates the exact encoding boundary; this is not
        // a real filesystem location accepted through the public API.
        model.location = "x".repeat(MAX_SNAPSHOT_BYTES - 3 * 256 - baseline - 4);
        assert_eq!(encode(&model)?.len(), MAX_SNAPSHOT_BYTES - 3 * 256);
        model.location.push('x');
        assert!(matches!(
            encode(&model),
            Err(LedgerError::Codec(CodecError::ByteLimit))
        ));
        Ok(())
    }

    #[test]
    fn snapshot_admission_preserves_terminal_metadata_item_capacity() -> Result<(), LedgerError> {
        let mut model = model_with(&[EffectState::Authorized], 4093);
        let id = ObjectId([9; 32]);
        model.promotions.insert(
            id,
            Dedup {
                payload: id,
                fingerprint: id,
                response: PromotionOutcome::Conflict(ConflictReport { reasons: vec![] }),
            },
        );
        let baseline = count_items(&Value::decode(&encode(&model)?, &snapshot_limits())?);
        let dedup = model.promotions.get_mut(&id).ok_or(LedgerError::Missing)?;
        let PromotionOutcome::Conflict(report) = &mut dedup.response else {
            return Err(LedgerError::Corrupt);
        };
        // Deliberately synthetic cardinality tests the codec accounting itself.
        report.reasons = vec![String::new(); snapshot_limits().items - 3 * 16 - baseline + 1];
        assert!(matches!(
            encode(&model),
            Err(LedgerError::Codec(CodecError::ItemLimit))
        ));
        Ok(())
    }

    #[test]
    fn real_lifecycle_transitions_fit_reserved_snapshot_capacity() -> Result<(), LedgerError> {
        for recover in [false, true] {
            let mut model = model_with(&[EffectState::Authorized], 255);
            let id = ObjectId([0; 32]);
            model.rights.insert(
                "budget".into(),
                Right {
                    granted: 256,
                    reserved: 1,
                    spent: 255,
                },
            );
            model.location.clear();
            let baseline = encode(&model)?.len();
            // Exact byte-boundary synthetic padding; actual transitions below
            // use the production reducer, receipts and right accounting.
            model.location = "x".repeat(MAX_SNAPSHOT_BYTES - 3 * 256 - baseline - 4);
            let mut before = encode(&model)?;
            let mut before_items = count_items(&Value::decode(&before, &snapshot_limits())?);
            let empty_reader = Receipt(ObjectId([255; 32]), vec![]);
            crate::reducer::apply(
                &mut model,
                &Value::Array(vec![Value::Unsigned(5), v_id(id)]),
                &empty_reader,
            )?;
            // 255 -> 256 also enlarges the encoded sequence counter.
            assert_eq!(model.seq, 256);
            let after = encode(&model)?;
            assert!(after.len() <= before.len() + 256);
            before = after;
            if recover {
                crate::reducer::apply(
                    &mut model,
                    &Value::Array(vec![Value::Unsigned(8), Value::Array(vec![v_id(id)])]),
                    &empty_reader,
                )?;
            } else {
                let evidence = receipt(id, model.intents.get(&id).ok_or(LedgerError::Missing)?, 2)?;
                crate::reducer::apply(
                    &mut model,
                    &Value::Array(vec![
                        Value::Unsigned(6),
                        v_id(id),
                        Value::Unsigned(1),
                        Value::Unsigned(2),
                        v_id(evidence.0),
                    ]),
                    &evidence,
                )?;
            }
            let after = encode(&model)?;
            let after_items = count_items(&Value::decode(&after, &snapshot_limits())?);
            assert!(after.len() <= before.len() + 256);
            assert!(after_items <= before_items + 16);
            before = after;
            before_items = after_items;
            let evidence = receipt(id, model.intents.get(&id).ok_or(LedgerError::Missing)?, 3)?;
            crate::reducer::apply(
                &mut model,
                &Value::Array(vec![
                    Value::Unsigned(6),
                    v_id(id),
                    Value::Unsigned(1),
                    Value::Unsigned(3),
                    v_id(evidence.0),
                ]),
                &evidence,
            )?;
            let after = encode(&model)?;
            assert!(after.len() <= before.len() + 256);
            assert!(count_items(&Value::decode(&after, &snapshot_limits())?) <= before_items + 16);
            assert_eq!(model.reserved_lifecycle_events()?, 0);
            let right = model.rights.get("budget").ok_or(LedgerError::Missing)?;
            assert_eq!((right.reserved, right.spent), (0, 256));
        }
        Ok(())
    }

    #[test]
    fn active_intents_reserve_claim_recovery_and_terminal_event_capacity() {
        for (state, required) in [
            (EffectState::Authorized, 3),
            (EffectState::Dispatching, 2),
            (EffectState::Unknown, 1),
        ] {
            assert!(
                model_with(&[state], MAX_LEDGER_ROWS as u64 - required)
                    .check()
                    .is_ok()
            );
            assert!(matches!(
                model_with(&[state], MAX_LEDGER_ROWS as u64 - required + 1).check(),
                Err(LedgerError::Limit)
            ));
        }
    }

    #[test]
    fn capacity_is_additive_and_terminal_intents_need_no_reservation() {
        let states = [
            EffectState::Authorized,
            EffectState::Dispatching,
            EffectState::Unknown,
            EffectState::Confirmed,
            EffectState::DefinitivelyRejected,
            EffectState::CancelledBeforeSend,
        ];
        assert!(
            model_with(&states, MAX_LEDGER_ROWS as u64 - 6)
                .check()
                .is_ok()
        );
        assert!(matches!(
            model_with(&states, MAX_LEDGER_ROWS as u64 - 5).check(),
            Err(LedgerError::Limit)
        ));
        assert!(
            model_with(&states[3..], MAX_LEDGER_ROWS as u64)
                .check()
                .is_ok()
        );
        assert!(matches!(
            model_with(&states, u64::MAX).check(),
            Err(LedgerError::Limit)
        ));
    }
}
