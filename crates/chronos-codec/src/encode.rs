use crate::{CodecError, Limits, OBJECT_SCHEMA, OBJECT_TAG, Object, Value, limits::Budget};
use minicbor::Encoder;

fn head_len(value: u64) -> usize {
    match value {
        0..=23 => 1,
        24..=255 => 2,
        256..=65535 => 3,
        65536..=0xffff_ffff => 5,
        _ => 9,
    }
}

fn size(value: &Value, depth: usize, budget: &mut Budget<'_>) -> Result<(), CodecError> {
    budget.node(depth)?;
    match value {
        Value::Null | Value::Bool(_) => budget.add_bytes(1)?,
        Value::Unsigned(number) | Value::Negative(number) => budget.add_bytes(head_len(*number))?,
        Value::Bytes(bytes) => {
            budget.add_bytes(head_len(bytes.len() as u64))?;
            budget.add_bytes(bytes.len())?;
        }
        Value::Text(text) => {
            budget.add_bytes(head_len(text.len() as u64))?;
            budget.add_bytes(text.len())?;
        }
        Value::Array(values) => {
            budget.ensure_items(values.len())?;
            budget.add_bytes(head_len(values.len() as u64))?;
            for value in values {
                size(value, depth + 1, budget)?;
            }
        }
        Value::Map(values) => {
            budget.ensure_items(values.len().checked_mul(2).ok_or(CodecError::ItemLimit)?)?;
            budget.add_bytes(head_len(values.len() as u64))?;
            for (key, value) in values {
                budget.node(depth + 1)?;
                budget.add_bytes(head_len(key.len() as u64))?;
                budget.add_bytes(key.len())?;
                size(value, depth + 1, budget)?;
            }
        }
    }
    Ok(())
}

fn emit(encoder: &mut Encoder<Vec<u8>>, value: &Value) -> Result<(), CodecError> {
    match value {
        Value::Null => {
            encoder.null().map_err(|_| CodecError::Encode)?;
        }
        Value::Bool(value) => {
            encoder.bool(*value).map_err(|_| CodecError::Encode)?;
        }
        Value::Unsigned(value) => {
            encoder.u64(*value).map_err(|_| CodecError::Encode)?;
        }
        Value::Negative(value) => {
            encoder
                .i128(-1 - i128::from(*value))
                .map_err(|_| CodecError::Encode)?;
        }
        Value::Bytes(value) => {
            encoder.bytes(value).map_err(|_| CodecError::Encode)?;
        }
        Value::Text(value) => {
            encoder.str(value).map_err(|_| CodecError::Encode)?;
        }
        Value::Array(values) => {
            encoder
                .array(values.len() as u64)
                .map_err(|_| CodecError::Encode)?;
            for value in values {
                emit(encoder, value)?;
            }
        }
        Value::Map(values) => {
            encoder
                .map(values.len() as u64)
                .map_err(|_| CodecError::Encode)?;
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left.as_bytes().cmp(right.as_bytes()))
            });
            for (key, value) in entries {
                encoder.str(key).map_err(|_| CodecError::Encode)?;
                emit(encoder, value)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn value(value: &Value, limits: &Limits) -> Result<Vec<u8>, CodecError> {
    let mut budget = Budget::new(limits)?;
    size(value, 1, &mut budget)?;
    let mut encoder = Encoder::new(Vec::with_capacity(budget.bytes));
    emit(&mut encoder, value)?;
    Ok(encoder.into_writer())
}

pub(crate) fn object(object: &Object, limits: &Limits) -> Result<Vec<u8>, CodecError> {
    let mut budget = Budget::new(limits)?;
    budget.node(1)?;
    budget.add_bytes(1)?;
    budget.node(2)?;
    budget.add_bytes(head_len(OBJECT_TAG))?;
    budget.node(2)?;
    budget.add_bytes(head_len(u64::from(OBJECT_SCHEMA)))?;
    budget.node(2)?;
    budget.ensure_items(object.refs.len())?;
    budget.add_bytes(head_len(object.refs.len() as u64))?;
    for _ in &object.refs {
        budget.node(3)?;
        budget.add_bytes(34)?;
    }
    size(&object.payload, 2, &mut budget)?;
    let mut encoder = Encoder::new(Vec::with_capacity(budget.bytes));
    encoder.array(4).map_err(|_| CodecError::Encode)?;
    encoder.u64(OBJECT_TAG).map_err(|_| CodecError::Encode)?;
    encoder.u16(OBJECT_SCHEMA).map_err(|_| CodecError::Encode)?;
    encoder
        .array(object.refs.len() as u64)
        .map_err(|_| CodecError::Encode)?;
    for id in &object.refs {
        encoder.bytes(&id.0).map_err(|_| CodecError::Encode)?;
    }
    emit(&mut encoder, &object.payload)?;
    Ok(encoder.into_writer())
}
