use crate::schema::{Decision, Dedup, Evidence, Intent, Model, Right};
use crate::*;
use chronos_codec::{Canonical, Limits, OBJECT_KIND, OBJECT_SCHEMA, object_id};
use chronos_state::{ObjectData, StateReader};
use chronos_store::StoreTransaction;
use rusqlite::params;

pub(crate) struct TxReader<'a, 'b>(pub &'a StoreTransaction<'b>);
impl StateReader for TxReader<'_, '_> {
    fn get_object(&self, id: ObjectId) -> Result<ObjectData, StateError> {
        let data = self.0.get_object(id).map_err(|_| StateError::Storage)?;
        Ok(ObjectData {
            kind: data.kind,
            schema: data.schema,
            bytes: data.bytes,
        })
    }
}
pub(crate) fn read(
    reader: &(impl StateReader + ?Sized),
    id: ObjectId,
) -> Result<Vec<u8>, LedgerError> {
    let object = reader.get_object(id)?;
    if object.bytes.len() > Limits::default().object_bytes
        || (object.kind, object.schema) != (OBJECT_KIND, OBJECT_SCHEMA)
        || object_id(object.kind, object.schema, &object.bytes) != id
    {
        return Err(LedgerError::Corrupt);
    }
    Ok(object.bytes)
}
pub(crate) fn event_id(bytes: &[u8]) -> ObjectId {
    object_id(0x4c45, 1, bytes)
}
pub(crate) fn decode_event(bytes: &[u8]) -> Result<(u64, ObjectId, Value), LedgerError> {
    let mut f = fixed(Value::decode(bytes, &Limits::default())?, 4)?;
    if num(take(&mut f)?)? != 1 {
        return Err(LedgerError::Corrupt);
    }
    Ok((num(take(&mut f)?)?, id(take(&mut f)?)?, take(&mut f)?))
}
pub(crate) fn append(
    tx: &StoreTransaction<'_>,
    model: &mut Model,
    command: Value,
) -> Result<(), LedgerError> {
    let previous = model.head;
    let bytes = Value::Array(vec![
        Value::Unsigned(1),
        Value::Unsigned(model.seq),
        v_id(previous),
        command,
    ])
    .encode()?;
    let head = event_id(&bytes);
    model.head = head;
    let root = tx.put(OBJECT_KIND, OBJECT_SCHEMA, &model.root.encode()?)?;
    tx.pin(&format!("ledger:root:{}", model.seq), root)?;
    tx.sql().execute(
        "INSERT INTO ledger_events(seq,id,previous,bytes) VALUES(?1,?2,?3,?4)",
        params![integer(model.seq)?, &head.0[..], &previous.0[..], bytes],
    )?;
    schema::write(tx.sql(), model)
}

/// Pure deterministic reducer: only supplied immutable objects are read. No
/// provider, network, time, random source or dispatcher is reachable from here.
pub(crate) fn apply(
    model: &mut Model,
    command: &Value,
    reader: &(impl StateReader + ?Sized),
) -> Result<(), LedgerError> {
    let mut f = list(command.clone())?.into_iter();
    let code = num(take(&mut f)?)?;
    let next_seq = model.seq.checked_add(1).ok_or(LedgerError::Limit)?;
    match code {
        0 => {
            let domain = DomainId(id(take(&mut f)?)?.0);
            let location = text_value(take(&mut f)?)?;
            if model.seq != 0 || model.root.domain() != domain || model.location != location {
                return Err(LedgerError::Corrupt);
            }
        }
        1 => {
            let id = id(take(&mut f)?)?;
            let policy = promotion::decode_policy(&read(reader, id)?)?;
            model.policies.insert(id, policy);
            model.policy = Some(id);
        }
        2 => {
            let name = text_value(take(&mut f)?)?;
            string(&name)?;
            let units = num(take(&mut f)?)?;
            if units == 0 {
                return Err(LedgerError::InvalidRequest);
            }
            let right = model.rights.entry(name).or_insert(Right {
                granted: 0,
                reserved: 0,
                spent: 0,
            });
            right.granted = right.granted.checked_add(units).ok_or(LedgerError::Limit)?;
            integer(right.granted)?;
        }
        3 => {
            let id = id(take(&mut f)?)?;
            let generation = num(take(&mut f)?)?;
            integer(generation)?;
            let expected = model
                .decisions
                .get(&id)
                .map_or(Some(0), |decision| decision.generation.checked_add(1))
                .ok_or(LedgerError::Limit)?;
            if generation != expected {
                return Err(LedgerError::InvalidRequest);
            }
            model.decisions.insert(
                id,
                Decision {
                    generation,
                    winner: None,
                },
            );
        }
        4 => {
            let request = id(take(&mut f)?)?;
            let payload = id(take(&mut f)?)?;
            let branch_id = id(take(&mut f)?)?;
            let decision = id(take(&mut f)?)?;
            let generation = num(take(&mut f)?)?;
            if model.promotions.contains_key(&request) {
                return Err(LedgerError::IdempotencyConflict);
            }
            let fingerprint = object_id(0x5052, 1, &command.encode()?);
            let proposal = promotion::decode_payload(&read(reader, payload)?)?;
            // Decode under the branch's own domain first so mismatches produce a
            // stored conflict rather than becoming invisible reducer failures.
            let branch = FrozenBranch::reopen(reader, branch_id, proposal.domain)?;
            let mut reasons = Vec::new();
            if proposal.domain != model.root.domain() || branch.domain() != model.root.domain() {
                reasons.push("domain".into());
            }
            if proposal.epoch != model.epoch {
                reasons.push("epoch".into());
            }
            if model.policy != Some(proposal.policy)
                || branch.worldline().policy != Some(proposal.policy)
            {
                reasons.push("policy".into());
            }
            if model
                .decisions
                .get(&decision)
                .is_none_or(|entry| entry.generation != generation || entry.winner.is_some())
            {
                reasons.push("decision".into());
            }
            if branch.domain() == model.root.domain() && !branch.conflicts(&model.root)?.is_empty()
            {
                reasons.push("read-dependency".into());
            }
            let policy = model.policies.get(&proposal.policy);
            if policy.is_none() {
                reasons.push("policy-missing".into());
            }
            let mut intent_id = None;
            if let Some(effect) = &proposal.effect {
                if policy.is_none_or(|policy| {
                    !policy.allowed_contracts.contains(&effect.contract)
                        || effect.cost > policy.max_effect_cost
                }) {
                    reasons.push("effect-policy".into());
                }
                if model
                    .rights
                    .get(&effect.right)
                    .is_none_or(|right| right.granted - right.reserved - right.spent < effect.cost)
                {
                    reasons.push("insufficient-right".into());
                }
                if model.intents.values().any(|intent| {
                    intent.proposal.semantic_slot == effect.semantic_slot
                        && matches!(
                            intent.state,
                            EffectState::Authorized
                                | EffectState::Dispatching
                                | EffectState::Unknown
                                | EffectState::Confirmed
                        )
                }) {
                    reasons.push("semantic-slot".into());
                }
                if model.intents.values().any(|intent| {
                    intent.proposal.contract == effect.contract
                        && intent.proposal.provider == effect.provider
                        && intent.proposal.operation == effect.operation
                }) {
                    reasons.push("provider-operation-reused".into());
                }
                let id = object_id(
                    0x494e,
                    1,
                    &Value::Array(vec![
                        Value::Bytes(model.root.domain().0.to_vec()),
                        promotion::proposal_value(effect),
                    ])
                    .encode()?,
                );
                if model.intents.contains_key(&id) {
                    reasons.push("operation-reused".into());
                }
                intent_id = Some(id);
            }
            let candidate = if reasons.is_empty() {
                Some(model.root.apply(branch.overlay())?)
            } else {
                None
            };
            if let (Some(candidate), Some(policy)) = (&candidate, policy) {
                for invariant in &policy.invariants {
                    match invariant {
                        Invariant::KeyEquals { key, expected }
                            if candidate.get(key)? != *expected =>
                        {
                            reasons.push("business-invariant".into())
                        }
                        _ => {}
                    }
                }
            }
            let response = if !reasons.is_empty() {
                PromotionOutcome::Conflict(ConflictReport { reasons })
            } else {
                model.root = candidate.ok_or(LedgerError::Corrupt)?;
                model
                    .decisions
                    .get_mut(&decision)
                    .ok_or(LedgerError::Corrupt)?
                    .winner = Some(request);
                if let (Some(effect), Some(intent)) = (proposal.effect, intent_id) {
                    let right = model
                        .rights
                        .get_mut(&effect.right)
                        .ok_or(LedgerError::Corrupt)?;
                    right.reserved = right
                        .reserved
                        .checked_add(effect.cost)
                        .ok_or(LedgerError::Limit)?;
                    model.intents.insert(
                        intent,
                        Intent {
                            proposal: effect,
                            state: EffectState::Authorized,
                            attempt: 0,
                            epoch: model.epoch,
                        },
                    );
                    PromotionOutcome::AcceptedPending {
                        seq: next_seq,
                        intent,
                    }
                } else {
                    PromotionOutcome::LocalCommitted { seq: next_seq }
                }
            };
            model.promotions.insert(
                request,
                Dedup {
                    payload,
                    fingerprint,
                    response,
                },
            );
        }
        5 => {
            let id = id(take(&mut f)?)?;
            let intent = model.intents.get_mut(&id).ok_or(LedgerError::Missing)?;
            if intent.state != EffectState::Authorized {
                return Err(LedgerError::IllegalTransition);
            }
            intent.attempt = intent.attempt.checked_add(1).ok_or(LedgerError::Limit)?;
            intent.state = EffectState::Dispatching;
        }
        6 => {
            let intent_id = id(take(&mut f)?)?;
            let attempt = num(take(&mut f)?)?;
            let status = num(take(&mut f)?)?;
            let receipt = id(take(&mut f)?)?;
            let intent = model
                .intents
                .get_mut(&intent_id)
                .ok_or(LedgerError::Missing)?;
            if !matches!(
                intent.state,
                EffectState::Dispatching | EffectState::Unknown
            ) || intent.attempt != attempt
                || attempt == 0
            {
                return Err(LedgerError::IllegalTransition);
            }
            crate::effects::check_receipt(&read(reader, receipt)?, intent_id, intent, status)?;
            let next = EffectState::from_code(status)?;
            if !matches!(
                next,
                EffectState::Confirmed | EffectState::DefinitivelyRejected | EffectState::Unknown
            ) {
                return Err(LedgerError::InvalidEvidence);
            }
            let right = model
                .rights
                .get_mut(&intent.proposal.right)
                .ok_or(LedgerError::Corrupt)?;
            match next {
                EffectState::Confirmed => {
                    right.reserved = right
                        .reserved
                        .checked_sub(intent.proposal.cost)
                        .ok_or(LedgerError::Corrupt)?;
                    right.spent = right
                        .spent
                        .checked_add(intent.proposal.cost)
                        .ok_or(LedgerError::Limit)?;
                }
                EffectState::DefinitivelyRejected => {
                    right.reserved = right
                        .reserved
                        .checked_sub(intent.proposal.cost)
                        .ok_or(LedgerError::Corrupt)?
                }
                _ => {}
            }
            intent.state = next;
            model.evidence.push(Evidence {
                intent: intent_id,
                attempt,
                status,
                receipt,
            });
        }
        7 => {
            let id = id(take(&mut f)?)?;
            let intent = model.intents.get_mut(&id).ok_or(LedgerError::Missing)?;
            if intent.state != EffectState::Authorized {
                return Err(LedgerError::IllegalTransition);
            }
            let right = model
                .rights
                .get_mut(&intent.proposal.right)
                .ok_or(LedgerError::Corrupt)?;
            right.reserved = right
                .reserved
                .checked_sub(intent.proposal.cost)
                .ok_or(LedgerError::Corrupt)?;
            intent.state = EffectState::CancelledBeforeSend;
        }
        8 => {
            let ids = list(take(&mut f)?)?
                .into_iter()
                .map(id)
                .collect::<Result<Vec<_>, _>>()?;
            let expected: Vec<_> = model
                .intents
                .iter()
                .filter(|(_, intent)| intent.state == EffectState::Dispatching)
                .map(|(id, _)| *id)
                .collect();
            if ids.is_empty() || ids != expected {
                return Err(LedgerError::Corrupt);
            }
            for id in ids {
                model
                    .intents
                    .get_mut(&id)
                    .ok_or(LedgerError::Corrupt)?
                    .state = EffectState::Unknown;
            }
        }
        _ => return Err(LedgerError::Corrupt),
    }
    if f.next().is_some() {
        return Err(LedgerError::Corrupt);
    }
    model.seq = next_seq;
    model.check()
}
