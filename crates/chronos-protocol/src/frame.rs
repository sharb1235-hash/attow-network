use crate::{Message, ProtocolError, ProtocolLimits};
use chronos_codec::{Canonical, Limits, Value};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    /// Bytes after the length prefix, including the already-read message tag.
    pub payload_bytes: usize,
    pub message_type: u8,
}
/// Inspect the fixed header before allocating or reading its claimed body.
pub fn decode_header(
    bytes: &[u8; 5],
    limits: &ProtocolLimits,
) -> Result<FrameHeader, ProtocolError> {
    limits.validate()?;
    let payload_bytes = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if payload_bytes > limits.control_bytes {
        return Err(ProtocolError::Limit);
    }
    if payload_bytes < 2 {
        return Err(ProtocolError::InvalidMessage);
    }
    let message_type = bytes[4];
    if !(1..=9).contains(&message_type) {
        return Err(ProtocolError::UnknownMessage);
    }
    // A chunk's fixed CBOR metadata requires at most 64 bytes. A conservative
    // 128-byte allowance bounds reading even before its byte-string is decoded.
    if message_type == 4 && payload_bytes > limits.chunk_bytes + 128 {
        return Err(ProtocolError::Limit);
    }
    Ok(FrameHeader {
        payload_bytes,
        message_type,
    })
}
fn codec_limits(limits: &ProtocolLimits) -> Limits {
    Limits {
        object_bytes: limits.control_bytes,
        depth: 8,
        items: limits.max_ids + 128,
    }
}
pub fn encode_frame(message: &Message) -> Result<Vec<u8>, ProtocolError> {
    let limits = ProtocolLimits::default();
    message.validate(&limits)?;
    let body = message
        .to_value()
        .encode_with_limits(&codec_limits(&limits))?;
    let length = body.len().checked_add(1).ok_or(ProtocolError::Limit)?;
    if length > limits.control_bytes {
        return Err(ProtocolError::Limit);
    }
    let mut bytes = Vec::with_capacity(length + 4);
    bytes.extend_from_slice(&(length as u32).to_be_bytes());
    bytes.push(message.tag());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}
pub fn decode_frame(bytes: &[u8], limits: &ProtocolLimits) -> Result<Message, ProtocolError> {
    limits.validate()?;
    let header: &[u8; 5] = bytes
        .get(..5)
        .ok_or(ProtocolError::Truncated)?
        .try_into()
        .map_err(|_| ProtocolError::Truncated)?;
    let header = decode_header(header, limits)?;
    let expected = header.payload_bytes + 4;
    if bytes.len() < expected {
        return Err(ProtocolError::Truncated);
    }
    if bytes.len() > expected {
        return Err(ProtocolError::TrailingBytes);
    }
    let value = Value::decode(&bytes[5..], &codec_limits(limits))?;
    let message = Message::from_value(header.message_type, value)?;
    message.validate(limits)?;
    Ok(message)
}
