use crate::reducer::{TxReader, append, apply};
use crate::*;
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, object_id};

pub(crate) fn proposal_value(p: &EffectProposal) -> Value {
    Value::Array(vec![
        v_id(p.contract),
        Value::Text(p.provider.clone()),
        Value::Text(p.semantic_slot.clone()),
        v_id(p.operation),
        Value::Text(p.right.clone()),
        Value::Unsigned(p.cost),
        Value::Bytes(p.body.clone()),
    ])
}
pub(crate) fn proposal_from_value(value: Value) -> Result<EffectProposal, LedgerError> {
    let mut f = fixed(value, 7)?;
    let p = EffectProposal {
        contract: id(take(&mut f)?)?,
        provider: text_value(take(&mut f)?)?,
        semantic_slot: text_value(take(&mut f)?)?,
        operation: id(take(&mut f)?)?,
        right: text_value(take(&mut f)?)?,
        cost: num(take(&mut f)?)?,
        body: blob(take(&mut f)?)?,
    };
    validate_proposal(&p)?;
    Ok(p)
}
fn validate_proposal(p: &EffectProposal) -> Result<(), LedgerError> {
    string(&p.provider)?;
    string(&p.semantic_slot)?;
    string(&p.right)?;
    integer(p.cost)?;
    if p.cost == 0 || p.body.len() > 64 * 1024 {
        return Err(LedgerError::InvalidRequest);
    }
    Ok(())
}
pub(crate) fn encode_policy(p: &Policy) -> Result<Vec<u8>, LedgerError> {
    integer(p.max_effect_cost)?;
    if p.allowed_contracts.len() > 256 || p.invariants.len() > 256 {
        return Err(LedgerError::Limit);
    }
    let invariants = p
        .invariants
        .iter()
        .map(|invariant| match invariant {
            Invariant::KeyEquals { key, expected } => {
                if key.len() > chronos_state::MAX_KEY_BYTES
                    || expected
                        .as_ref()
                        .is_some_and(|value| value.len() > chronos_state::MAX_VALUE_BYTES)
                {
                    return Err(LedgerError::InvalidRequest);
                }
                Ok(Value::Array(vec![
                    Value::Bytes(key.clone()),
                    expected
                        .as_ref()
                        .map_or(Value::Null, |value| Value::Bytes(value.clone())),
                ]))
            }
        })
        .collect::<Result<_, LedgerError>>()?;
    Ok(Object {
        refs: BTreeSet::new(),
        payload: Value::Array(vec![
            Value::Text("attow-policy".into()),
            Value::Unsigned(1),
            Value::Array(p.allowed_contracts.iter().copied().map(v_id).collect()),
            Value::Unsigned(p.max_effect_cost),
            Value::Array(invariants),
        ]),
    }
    .encode()?)
}
pub(crate) fn decode_policy(bytes: &[u8]) -> Result<Policy, LedgerError> {
    let object = Object::decode(bytes, &Limits::default())?;
    if !object.refs.is_empty() {
        return Err(LedgerError::Corrupt);
    }
    let mut f = fixed(object.payload, 5)?;
    if text_value(take(&mut f)?)? != "attow-policy" || num(take(&mut f)?)? != 1 {
        return Err(LedgerError::Corrupt);
    }
    let contracts = list(take(&mut f)?)?;
    if contracts.len() > 256 {
        return Err(LedgerError::Limit);
    }
    let mut allowed_contracts = BTreeSet::new();
    for contract in contracts {
        if !allowed_contracts.insert(id(contract)?) {
            return Err(LedgerError::Corrupt);
        }
    }
    let max_effect_cost = num(take(&mut f)?)?;
    let mut invariants = Vec::new();
    for invariant in list(take(&mut f)?)? {
        let mut f = fixed(invariant, 2)?;
        let key = blob(take(&mut f)?)?;
        let expected = match take(&mut f)? {
            Value::Null => None,
            v => Some(blob(v)?),
        };
        invariants.push(Invariant::KeyEquals { key, expected });
    }
    let policy = Policy {
        allowed_contracts,
        max_effect_cost,
        invariants,
    };
    if encode_policy(&policy)? != bytes {
        return Err(LedgerError::Corrupt);
    }
    Ok(policy)
}
pub(crate) fn encode_payload(p: &PromotionPayload) -> Result<Vec<u8>, LedgerError> {
    integer(p.epoch)?;
    if let Some(effect) = &p.effect {
        validate_proposal(effect)?;
    }
    Ok(Object {
        refs: BTreeSet::from([p.policy]),
        payload: Value::Array(vec![
            Value::Text("attow-promotion".into()),
            Value::Unsigned(1),
            Value::Bytes(p.domain.0.to_vec()),
            Value::Unsigned(p.epoch),
            v_id(p.policy),
            p.effect.as_ref().map_or(Value::Null, proposal_value),
        ]),
    }
    .encode()?)
}
pub(crate) fn decode_payload(bytes: &[u8]) -> Result<PromotionPayload, LedgerError> {
    let object = Object::decode(bytes, &Limits::default())?;
    let mut f = fixed(object.payload, 6)?;
    if text_value(take(&mut f)?)? != "attow-promotion" || num(take(&mut f)?)? != 1 {
        return Err(LedgerError::Corrupt);
    }
    let p = PromotionPayload {
        domain: DomainId(id(take(&mut f)?)?.0),
        epoch: num(take(&mut f)?)?,
        policy: id(take(&mut f)?)?,
        effect: match take(&mut f)? {
            Value::Null => None,
            value => Some(proposal_from_value(value)?),
        },
    };
    if object.refs != BTreeSet::from([p.policy]) || encode_payload(&p)? != bytes {
        return Err(LedgerError::Corrupt);
    }
    Ok(p)
}

impl Authority {
    pub fn configure_policy(&mut self, policy: Policy) -> Result<ObjectId, LedgerError> {
        self.require_writable()?;
        let bytes = encode_policy(&policy)?;
        let binding = self.write_binding();
        let result = self.store.transaction_with(|tx| {
            let id = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &bytes)?;
            let mut state = schema::load(tx.sql())?;
            federation::check_binding(&state, binding)?;
            let command = Value::Array(vec![Value::Unsigned(1), v_id(id)]);
            apply(&mut state, &command, &TxReader(tx))?;
            append(tx, &mut state, command)?;
            tx.pin("ledger:policies", id)?;
            Ok::<_, LedgerError>(id)
        })?;
        self.finish_write()?;
        Ok(result)
    }
    pub fn grant_right(&mut self, name: &str, units: u64) -> Result<(), LedgerError> {
        self.require_writable()?;
        string(name)?;
        integer(units)?;
        if units == 0 {
            return Err(LedgerError::InvalidRequest);
        }
        let command = Value::Array(vec![
            Value::Unsigned(2),
            Value::Text(name.to_owned()),
            Value::Unsigned(units),
        ]);
        self.admin(command)
    }
    pub fn advance_generation(
        &mut self,
        decision: ObjectId,
        generation: u64,
    ) -> Result<(), LedgerError> {
        self.require_writable()?;
        integer(generation)?;
        self.admin(Value::Array(vec![
            Value::Unsigned(3),
            v_id(decision),
            Value::Unsigned(generation),
        ]))
    }
    fn admin(&mut self, command: Value) -> Result<(), LedgerError> {
        let binding = self.write_binding();
        self.store.transaction_with(|tx| {
            let mut state = schema::load(tx.sql())?;
            federation::check_binding(&state, binding)?;
            apply(&mut state, &command, &TxReader(tx))?;
            append(tx, &mut state, command)?;
            Ok::<_, LedgerError>(())
        })?;
        self.finish_write()
    }
    pub fn store_payload(&mut self, payload: &PromotionPayload) -> Result<ObjectId, LedgerError> {
        self.require_writable()?;
        let bytes = encode_payload(payload)?;
        let binding = self.write_binding();
        self.store.transaction_with(|tx| {
            federation::check_binding(&schema::load(tx.sql())?, binding)?;
            Ok(tx.put(OBJECT_KIND, OBJECT_SCHEMA, &bytes)?)
        })
    }
    pub fn promote(&mut self, request: PromotionRequest) -> Result<PromotionOutcome, LedgerError> {
        self.require_writable()?;
        integer(request.generation)?;
        let branch_id = request.branch.object_id()?;
        let command = Value::Array(vec![
            Value::Unsigned(4),
            v_id(request.id),
            v_id(request.payload),
            v_id(branch_id),
            v_id(request.decision),
            Value::Unsigned(request.generation),
        ]);
        let fingerprint = object_id(0x5052, 1, &command.encode()?);
        let hook = self.hook;
        let binding = self.write_binding();
        let result = self.store.transaction_with(|tx| {
            let mut state = schema::load(tx.sql())?;
            federation::check_binding(&state, binding)?;
            if let Some(previous) = state.promotions.get(&request.id) {
                if previous.payload != request.payload || previous.fingerprint != fingerprint {
                    return Err(LedgerError::IdempotencyConflict);
                }
                return Ok(previous.response.clone());
            }
            tx.put(OBJECT_KIND, OBJECT_SCHEMA, &request.branch.base().encode()?)?;
            tx.put(OBJECT_KIND, OBJECT_SCHEMA, &request.branch.encode()?)?;
            apply(&mut state, &command, &TxReader(tx))?;
            let outcome = state
                .promotions
                .get(&request.id)
                .ok_or(LedgerError::Corrupt)?
                .response
                .clone();
            append(tx, &mut state, command)?;
            let owner = format!("ledger:proposal:{}", state.seq);
            tx.pin(&owner, branch_id)?;
            tx.pin(&owner, request.payload)?;
            if let Some(hook) = hook {
                hook(TransitionBoundary::BeforePromoteCommit);
            }
            Ok(outcome)
        })?;
        self.finish_write()?;
        if let Some(hook) = hook {
            hook(TransitionBoundary::AfterPromoteCommit);
        }
        Ok(result)
    }
}
