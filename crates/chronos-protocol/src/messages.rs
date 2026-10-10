use crate::{Checkpoint, PROTOCOL_VERSION, ProtocolError, ProtocolLimits};
use chronos_codec::{ObjectId, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub version: u16,
    pub tenant: ObjectId,
    pub domain: ObjectId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub id: ObjectId,
    pub kind: u16,
    pub schema: u16,
    pub total_bytes: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
}
/// Data-only roots. Receipt does not authorize execution or mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalEnvelope {
    pub request_id: ObjectId,
    pub tenant: ObjectId,
    pub domain: ObjectId,
    pub epoch: u64,
    pub branch: ObjectId,
    pub payload: ObjectId,
    pub decision: ObjectId,
    pub generation: u64,
}
pub type Proposal = ProposalEnvelope;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromotionStatus {
    LocalCommitted { seq: u64 },
    AcceptedPending { seq: u64, intent: ObjectId },
    Conflict { reasons: Vec<String> },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionReply {
    pub request_id: ObjectId,
    pub domain: ObjectId,
    pub epoch: u64,
    pub status: PromotionStatus,
}
pub type ResultMessage = PromotionReply;
/// `durable=true` must be sent only after the receiving application's transaction commits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ack {
    pub objects: Vec<ObjectId>,
    pub durable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorMessage {
    pub code: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    OfferObjects(Vec<ObjectId>),
    WantObjects(Vec<ObjectId>),
    ObjectChunk(Chunk),
    BranchProposal(Proposal),
    PromotionResult(ResultMessage),
    SignedCheckpoint(Checkpoint),
    Ack(Ack),
    ProtocolError(ErrorMessage),
}

impl Message {
    pub fn validate(&self, limits: &ProtocolLimits) -> Result<(), ProtocolError> {
        limits.validate()?;
        match self {
            Self::Hello(hello) if hello.version != PROTOCOL_VERSION => {
                return Err(ProtocolError::Version);
            }
            Self::OfferObjects(values) | Self::WantObjects(values) => validate_ids(values, limits)?,
            Self::Ack(ack) => validate_ids(&ack.objects, limits)?,
            Self::ObjectChunk(chunk) => {
                if chunk.bytes.len() > limits.chunk_bytes || chunk.total_bytes > limits.object_bytes
                {
                    return Err(ProtocolError::Limit);
                }
                if chunk.bytes.is_empty()
                    || chunk.total_bytes == 0
                    || chunk.kind == 0
                    || chunk.schema == 0
                    || chunk
                        .offset
                        .checked_add(chunk.bytes.len() as u64)
                        .is_none_or(|end| end > chunk.total_bytes)
                {
                    return Err(ProtocolError::InvalidMessage);
                }
            }
            Self::BranchProposal(proposal) if proposal.epoch == 0 => {
                return Err(ProtocolError::InvalidMessage);
            }
            Self::PromotionResult(reply) => {
                if reply.epoch == 0 {
                    return Err(ProtocolError::InvalidMessage);
                }
                match &reply.status {
                    PromotionStatus::LocalCommitted { seq }
                    | PromotionStatus::AcceptedPending { seq, .. }
                        if *seq == 0 =>
                    {
                        return Err(ProtocolError::InvalidMessage);
                    }
                    PromotionStatus::Conflict { reasons } => {
                        if reasons.len() > 32 || reasons.iter().any(|reason| reason.len() > 256) {
                            return Err(ProtocolError::Limit);
                        }
                        if reasons.is_empty()
                            || reasons
                                .iter()
                                .any(|reason| reason.is_empty() || reason.contains('\0'))
                        {
                            return Err(ProtocolError::InvalidMessage);
                        }
                    }
                    _ => {}
                }
            }
            Self::SignedCheckpoint(checkpoint) if checkpoint.epoch == 0 => {
                return Err(ProtocolError::InvalidMessage);
            }
            Self::ProtocolError(error) if error.code == 0 => {
                return Err(ProtocolError::InvalidMessage);
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn tag(&self) -> u8 {
        match self {
            Self::Hello(_) => 1,
            Self::OfferObjects(_) => 2,
            Self::WantObjects(_) => 3,
            Self::ObjectChunk(_) => 4,
            Self::BranchProposal(_) => 5,
            Self::PromotionResult(_) => 6,
            Self::SignedCheckpoint(_) => 7,
            Self::Ack(_) => 8,
            Self::ProtocolError(_) => 9,
        }
    }
    pub(crate) fn to_value(&self) -> Value {
        let mut fields = vec![Value::Unsigned(u64::from(PROTOCOL_VERSION))];
        match self {
            Self::Hello(value) => fields.extend([id(value.tenant), id(value.domain)]),
            Self::OfferObjects(values) | Self::WantObjects(values) => fields.push(ids(values)),
            Self::ObjectChunk(value) => fields.extend([
                id(value.id),
                number(value.kind),
                number(value.schema),
                number(value.total_bytes),
                number(value.offset),
                Value::Bytes(value.bytes.clone()),
            ]),
            Self::BranchProposal(value) => fields.extend([
                id(value.request_id),
                id(value.tenant),
                id(value.domain),
                number(value.epoch),
                id(value.branch),
                id(value.payload),
                id(value.decision),
                number(value.generation),
            ]),
            Self::PromotionResult(value) => {
                let status = match &value.status {
                    PromotionStatus::LocalCommitted { seq } => {
                        Value::Array(vec![number(0u64), number(*seq)])
                    }
                    PromotionStatus::AcceptedPending { seq, intent } => {
                        Value::Array(vec![number(1u64), number(*seq), id(*intent)])
                    }
                    PromotionStatus::Conflict { reasons } => Value::Array(vec![
                        number(2u64),
                        Value::Array(reasons.iter().cloned().map(Value::Text).collect()),
                    ]),
                };
                fields.extend([
                    id(value.request_id),
                    id(value.domain),
                    number(value.epoch),
                    status,
                ]);
            }
            Self::SignedCheckpoint(value) => fields.extend([
                id(value.tenant),
                id(value.domain),
                Value::Bytes(value.writer.to_vec()),
                number(value.epoch),
                number(value.sequence),
                id(value.root),
                id(value.event_head),
                Value::Bytes(value.signature.to_vec()),
            ]),
            Self::Ack(value) => fields.extend([ids(&value.objects), Value::Bool(value.durable)]),
            Self::ProtocolError(value) => fields.push(number(value.code)),
        }
        Value::Array(fields)
    }
    pub(crate) fn from_value(tag: u8, value: Value) -> Result<Self, ProtocolError> {
        let length = match tag {
            1 => 3,
            2 | 3 | 9 => 2,
            4 => 7,
            5 | 7 => 9,
            6 => 5,
            8 => 3,
            _ => return Err(ProtocolError::UnknownMessage),
        };
        let mut fields = Fields::new(value, length)?;
        if fields.number()? != u64::from(PROTOCOL_VERSION) {
            return Err(ProtocolError::Version);
        }
        Ok(match tag {
            1 => Self::Hello(Hello {
                version: PROTOCOL_VERSION,
                tenant: fields.id()?,
                domain: fields.id()?,
            }),
            2 => Self::OfferObjects(fields.ids()?),
            3 => Self::WantObjects(fields.ids()?),
            4 => Self::ObjectChunk(Chunk {
                id: fields.id()?,
                kind: fields.small()?,
                schema: fields.small()?,
                total_bytes: fields.number()?,
                offset: fields.number()?,
                bytes: fields.bytes()?,
            }),
            5 => Self::BranchProposal(ProposalEnvelope {
                request_id: fields.id()?,
                tenant: fields.id()?,
                domain: fields.id()?,
                epoch: fields.number()?,
                branch: fields.id()?,
                payload: fields.id()?,
                decision: fields.id()?,
                generation: fields.number()?,
            }),
            6 => {
                let request_id = fields.id()?;
                let domain = fields.id()?;
                let epoch = fields.number()?;
                let values = array(fields.next()?)?;
                let mut status = Fields(values.into_iter());
                let variant = status.number()?;
                let result = match variant {
                    0 => PromotionStatus::LocalCommitted {
                        seq: status.number()?,
                    },
                    1 => PromotionStatus::AcceptedPending {
                        seq: status.number()?,
                        intent: status.id()?,
                    },
                    2 => PromotionStatus::Conflict {
                        reasons: array(status.next()?)?
                            .into_iter()
                            .map(|value| match value {
                                Value::Text(value) => Ok(value),
                                _ => Err(ProtocolError::InvalidMessage),
                            })
                            .collect::<Result<_, _>>()?,
                    },
                    _ => return Err(ProtocolError::InvalidMessage),
                };
                if status.0.next().is_some() {
                    return Err(ProtocolError::InvalidMessage);
                }
                Self::PromotionResult(PromotionReply {
                    request_id,
                    domain,
                    epoch,
                    status: result,
                })
            }
            7 => Self::SignedCheckpoint(Checkpoint {
                tenant: fields.id()?,
                domain: fields.id()?,
                writer: fields.fixed()?,
                epoch: fields.number()?,
                sequence: fields.number()?,
                root: fields.id()?,
                event_head: fields.id()?,
                signature: fields.fixed()?,
            }),
            8 => {
                let objects = fields.ids()?;
                let Value::Bool(durable) = fields.next()? else {
                    return Err(ProtocolError::InvalidMessage);
                };
                Self::Ack(Ack { objects, durable })
            }
            9 => Self::ProtocolError(ErrorMessage {
                code: fields.small()?,
            }),
            _ => return Err(ProtocolError::UnknownMessage),
        })
    }
}
fn validate_ids(values: &[ObjectId], limits: &ProtocolLimits) -> Result<(), ProtocolError> {
    if values.len() > limits.max_ids {
        return Err(ProtocolError::Limit);
    }
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(ProtocolError::InvalidMessage);
    }
    Ok(())
}
fn id(value: ObjectId) -> Value {
    Value::Bytes(value.0.to_vec())
}
fn ids(values: &[ObjectId]) -> Value {
    Value::Array(values.iter().copied().map(id).collect())
}
fn number(value: impl Into<u64>) -> Value {
    Value::Unsigned(value.into())
}
fn array(value: Value) -> Result<Vec<Value>, ProtocolError> {
    if let Value::Array(values) = value {
        Ok(values)
    } else {
        Err(ProtocolError::InvalidMessage)
    }
}
struct Fields(std::vec::IntoIter<Value>);
impl Fields {
    fn new(value: Value, length: usize) -> Result<Self, ProtocolError> {
        let values = array(value)?;
        if values.len() != length {
            return Err(ProtocolError::InvalidMessage);
        }
        Ok(Self(values.into_iter()))
    }
    fn next(&mut self) -> Result<Value, ProtocolError> {
        self.0.next().ok_or(ProtocolError::InvalidMessage)
    }
    fn number(&mut self) -> Result<u64, ProtocolError> {
        if let Value::Unsigned(value) = self.next()? {
            Ok(value)
        } else {
            Err(ProtocolError::InvalidMessage)
        }
    }
    fn small(&mut self) -> Result<u16, ProtocolError> {
        self.number()?
            .try_into()
            .map_err(|_| ProtocolError::InvalidMessage)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        if let Value::Bytes(value) = self.next()? {
            Ok(value)
        } else {
            Err(ProtocolError::InvalidMessage)
        }
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        self.bytes()?
            .try_into()
            .map_err(|_| ProtocolError::InvalidMessage)
    }
    fn id(&mut self) -> Result<ObjectId, ProtocolError> {
        Ok(ObjectId(self.fixed()?))
    }
    fn ids(&mut self) -> Result<Vec<ObjectId>, ProtocolError> {
        array(self.next()?)?
            .into_iter()
            .map(|value| {
                if let Value::Bytes(bytes) = value {
                    Ok(ObjectId(
                        bytes
                            .try_into()
                            .map_err(|_| ProtocolError::InvalidMessage)?,
                    ))
                } else {
                    Err(ProtocolError::InvalidMessage)
                }
            })
            .collect()
    }
}
