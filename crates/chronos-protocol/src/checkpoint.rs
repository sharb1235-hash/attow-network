use crate::ProtocolError;
use chronos_codec::{Canonical, ObjectId, Value};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub tenant: ObjectId,
    pub domain: ObjectId,
    pub writer: [u8; 32],
    pub epoch: u64,
    pub sequence: u64,
    pub root: ObjectId,
    pub event_head: ObjectId,
    pub signature: [u8; 64],
}
/// Trust and rollback policy is supplied by the application, never by the checkpoint itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointTrust {
    pub tenant: ObjectId,
    pub domain: ObjectId,
    pub writer: [u8; 32],
    pub epoch: u64,
    pub minimum_sequence: u64,
    pub expected_at_floor: Option<(ObjectId, ObjectId)>,
}
impl Checkpoint {
    /// Set the writer from the signing key, then sign all checkpoint fields.
    pub fn sign(&mut self, key: &SigningKey) -> Result<(), ProtocolError> {
        if self.epoch == 0 {
            return Err(ProtocolError::InvalidMessage);
        }
        self.writer = key.verifying_key().to_bytes();
        self.signature = key.sign(&self.signed_bytes()?).to_bytes();
        Ok(())
    }
    pub fn verify(&self, trust: &CheckpointTrust) -> Result<(), ProtocolError> {
        if self.tenant != trust.tenant || self.domain != trust.domain || self.writer != trust.writer
        {
            return Err(ProtocolError::Trust);
        }
        if self.epoch == 0 || self.epoch != trust.epoch || self.sequence < trust.minimum_sequence {
            return Err(ProtocolError::StaleCheckpoint);
        }
        let key = VerifyingKey::from_bytes(&self.writer).map_err(|_| ProtocolError::Signature)?;
        key.verify_strict(
            &self.signed_bytes()?,
            &Signature::from_bytes(&self.signature),
        )
        .map_err(|_| ProtocolError::Signature)?;
        if self.sequence == trust.minimum_sequence
            && trust
                .expected_at_floor
                .is_some_and(|expected| expected != (self.root, self.event_head))
        {
            return Err(ProtocolError::Trust);
        }
        Ok(())
    }
    fn signed_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut bytes = b"attow-network/checkpoint/v1\0".to_vec();
        bytes.extend(
            Value::Array(vec![
                Value::Unsigned(1),
                Value::Bytes(self.tenant.0.to_vec()),
                Value::Bytes(self.domain.0.to_vec()),
                Value::Bytes(self.writer.to_vec()),
                Value::Unsigned(self.epoch),
                Value::Unsigned(self.sequence),
                Value::Bytes(self.root.0.to_vec()),
                Value::Bytes(self.event_head.0.to_vec()),
            ])
            .encode()?,
        );
        Ok(bytes)
    }
}
