use std::collections::BTreeSet;

use chronos_codec::{Canonical, Limits, Object, ObjectId, Value};

use crate::SandboxError;

/// Guest byte streams are deliberately smaller than the canonical object cap.
pub const MAX_DATA_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Continuation {
    pub code: ObjectId,
    pub profile: ObjectId,
    pub bytes: Vec<u8>,
}

impl Continuation {
    pub fn encode(&self) -> Result<Vec<u8>, SandboxError> {
        bounded(&self.bytes)?;
        Ok(Object {
            refs: BTreeSet::from([self.code, self.profile]),
            payload: Value::Array(vec![
                Value::Text("attow-wasm-continuation".into()),
                Value::Unsigned(1),
                Value::Bytes(self.code.0.to_vec()),
                Value::Bytes(self.profile.0.to_vec()),
                Value::Bytes(self.bytes.clone()),
            ]),
        }
        .encode()?)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, SandboxError> {
        let object = Object::decode(bytes, &Limits::default())?;
        let Value::Array(fields) = object.payload else {
            return Err(SandboxError::InvalidContinuation);
        };
        let [
            Value::Text(tag),
            Value::Unsigned(1),
            Value::Bytes(code),
            Value::Bytes(profile),
            Value::Bytes(bytes),
        ]: [Value; 5] = fields
            .try_into()
            .map_err(|_| SandboxError::InvalidContinuation)?
        else {
            return Err(SandboxError::InvalidContinuation);
        };
        if tag != "attow-wasm-continuation" {
            return Err(SandboxError::InvalidContinuation);
        }
        let code = ObjectId(
            code.try_into()
                .map_err(|_| SandboxError::InvalidContinuation)?,
        );
        let profile = ObjectId(
            profile
                .try_into()
                .map_err(|_| SandboxError::InvalidContinuation)?,
        );
        if object.refs != BTreeSet::from([code, profile]) {
            return Err(SandboxError::InvalidContinuation);
        }
        bounded(&bytes)?;
        Ok(Self {
            code,
            profile,
            bytes,
        })
    }
}

pub(crate) fn bounded(bytes: &[u8]) -> Result<(), SandboxError> {
    if bytes.len() > MAX_DATA_BYTES {
        Err(SandboxError::Limit("guest byte stream"))
    } else {
        Ok(())
    }
}
pub(crate) fn encode_data(bytes: &[u8]) -> Result<Vec<u8>, SandboxError> {
    bounded(bytes)?;
    Ok(Object {
        refs: BTreeSet::new(),
        payload: Value::Array(vec![
            Value::Text("attow-wasm-data".into()),
            Value::Unsigned(1),
            Value::Bytes(bytes.to_vec()),
        ]),
    }
    .encode()?)
}
pub(crate) fn decode_data(bytes: &[u8]) -> Result<Vec<u8>, SandboxError> {
    let object = Object::decode(bytes, &Limits::default())?;
    if !object.refs.is_empty() {
        return Err(SandboxError::InvalidObject);
    }
    let Value::Array(fields) = object.payload else {
        return Err(SandboxError::InvalidObject);
    };
    let [Value::Text(tag), Value::Unsigned(1), Value::Bytes(bytes)]: [Value; 3] =
        fields.try_into().map_err(|_| SandboxError::InvalidObject)?
    else {
        return Err(SandboxError::InvalidObject);
    };
    if tag != "attow-wasm-data" {
        return Err(SandboxError::InvalidObject);
    }
    bounded(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_checkpoint_roundtrips_and_cannot_hide_reference_bindings()
    -> Result<(), SandboxError> {
        let checkpoint = Continuation {
            code: ObjectId([1; 32]),
            profile: ObjectId([2; 32]),
            bytes: vec![3, 4],
        };
        assert_eq!(Continuation::decode(&checkpoint.encode()?)?, checkpoint);
        let mut malicious = Object::decode(&checkpoint.encode()?, &Limits::default())?;
        malicious.refs.clear();
        assert!(matches!(
            Continuation::decode(&malicious.encode()?),
            Err(SandboxError::InvalidContinuation)
        ));
        Ok(())
    }
    #[test]
    fn guest_data_has_a_hard_bound_and_no_implicit_references() -> Result<(), SandboxError> {
        assert_eq!(decode_data(&encode_data(&[7, 8])?)?, vec![7, 8]);
        assert!(encode_data(&vec![0; MAX_DATA_BYTES + 1]).is_err());
        let mut object = Object::decode(&encode_data(&[])?, &Limits::default())?;
        object.refs.insert(ObjectId([9; 32]));
        assert!(decode_data(&object.encode()?).is_err());
        Ok(())
    }
}
