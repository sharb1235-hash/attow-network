use crate::{
    CodecError, Limits, OBJECT_SCHEMA, OBJECT_TAG, Object, ObjectId, Value, limits::Budget,
};
use std::collections::{BTreeMap, BTreeSet};

struct Reader<'a, 'limits> {
    input: &'a [u8],
    position: usize,
    budget: Budget<'limits>,
}

impl<'a> Reader<'a, '_> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(CodecError::Truncated)?;
        let value = self
            .input
            .get(self.position..end)
            .ok_or(CodecError::Truncated)?;
        self.position = end;
        Ok(value)
    }

    fn argument(&mut self, additional: u8) -> Result<u64, CodecError> {
        let (size, minimum) = match additional {
            0..=23 => return Ok(u64::from(additional)),
            24 => (1, 24),
            25 => (2, 256),
            26 => (4, 65536),
            27 => (8, 0x1_0000_0000),
            _ => return Err(CodecError::NonCanonical),
        };
        let mut number = 0_u64;
        for byte in self.take(size)? {
            number = (number << 8) | u64::from(*byte);
        }
        if number < minimum {
            return Err(CodecError::NonCanonical);
        }
        Ok(number)
    }

    fn count(&self, count: u64, multiplier: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(count).map_err(|_| CodecError::ItemLimit)?;
        let items = count.checked_mul(multiplier).ok_or(CodecError::ItemLimit)?;
        self.budget.ensure_items(items)?;
        if items > self.input.len().saturating_sub(self.position) {
            return Err(CodecError::Truncated);
        }
        Ok(count)
    }

    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        self.budget.node(depth)?;
        let initial = self.take(1)?[0];
        let major = initial >> 5;
        let additional = initial & 31;
        if major == 7 {
            return match initial {
                0xf4 => Ok(Value::Bool(false)),
                0xf5 => Ok(Value::Bool(true)),
                0xf6 => Ok(Value::Null),
                _ => Err(CodecError::UnsupportedType),
            };
        }
        if major == 6 {
            return Err(CodecError::UnsupportedType);
        }
        let argument = self.argument(additional)?;
        match major {
            0 => Ok(Value::Unsigned(argument)),
            1 => Ok(Value::Negative(argument)),
            2 | 3 => {
                let length = usize::try_from(argument).map_err(|_| CodecError::ByteLimit)?;
                if length > self.budget.limits.object_bytes {
                    return Err(CodecError::ByteLimit);
                }
                let bytes = self.take(length)?;
                if major == 2 {
                    Ok(Value::Bytes(bytes.to_vec()))
                } else {
                    Ok(Value::Text(
                        std::str::from_utf8(bytes)
                            .map_err(|_| CodecError::InvalidText)?
                            .to_owned(),
                    ))
                }
            }
            4 => {
                let count = self.count(argument, 1)?;
                if count > 0 && depth >= self.budget.limits.depth {
                    return Err(CodecError::DepthLimit);
                }
                // A length claim must not reserve children before they have
                // consumed the shared item budget: nested claims could otherwise
                // multiply peak allocation by depth before any child completes.
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(values))
            }
            5 => {
                let count = self.count(argument, 2)?;
                if count > 0 && depth >= self.budget.limits.depth {
                    return Err(CodecError::DepthLimit);
                }
                let mut values = BTreeMap::new();
                let mut previous: Option<String> = None;
                for _ in 0..count {
                    let Value::Text(key) = self.value(depth + 1)? else {
                        return Err(CodecError::UnsupportedType);
                    };
                    if values.contains_key(&key) {
                        return Err(CodecError::Duplicate);
                    }
                    if let Some(previous) = &previous
                        && previous
                            .len()
                            .cmp(&key.len())
                            .then_with(|| previous.as_bytes().cmp(key.as_bytes()))
                            .is_ge()
                    {
                        return Err(CodecError::NonCanonical);
                    }
                    previous = Some(key.clone());
                    values.insert(key, self.value(depth + 1)?);
                }
                Ok(Value::Map(values))
            }
            _ => Err(CodecError::UnsupportedType),
        }
    }
}

pub(crate) fn value(bytes: &[u8], limits: &Limits) -> Result<Value, CodecError> {
    let budget = Budget::new(limits)?;
    if bytes.len() > limits.object_bytes {
        return Err(CodecError::ByteLimit);
    }
    let mut reader = Reader {
        input: bytes,
        position: 0,
        budget,
    };
    let value = reader.value(1)?;
    if reader.position != bytes.len() {
        return Err(CodecError::TrailingBytes);
    }
    if crate::encode::value(&value, limits)? != bytes {
        return Err(CodecError::NonCanonical);
    }
    Ok(value)
}

pub(crate) fn object(bytes: &[u8], limits: &Limits) -> Result<Object, CodecError> {
    let Value::Array(mut fields) = value(bytes, limits)? else {
        return Err(CodecError::InvalidObject);
    };
    if fields.len() != 4 {
        return Err(CodecError::InvalidObject);
    }
    let payload = fields.pop().ok_or(CodecError::InvalidObject)?;
    let Value::Array(encoded_refs) = fields.pop().ok_or(CodecError::InvalidObject)? else {
        return Err(CodecError::InvalidObject);
    };
    if fields.pop() != Some(Value::Unsigned(u64::from(OBJECT_SCHEMA))) {
        return Err(CodecError::UnknownSchema);
    }
    if fields.pop() != Some(Value::Unsigned(OBJECT_TAG)) {
        return Err(CodecError::InvalidObject);
    }
    let mut refs = BTreeSet::new();
    let mut previous = None;
    for encoded in encoded_refs {
        let Value::Bytes(bytes) = encoded else {
            return Err(CodecError::InvalidObject);
        };
        let id = ObjectId(bytes.try_into().map_err(|_| CodecError::InvalidId)?);
        if previous == Some(id) {
            return Err(CodecError::Duplicate);
        }
        if previous.is_some_and(|previous| previous > id) {
            return Err(CodecError::NonCanonical);
        }
        refs.insert(id);
        previous = Some(id);
    }
    Ok(Object { refs, payload })
}
