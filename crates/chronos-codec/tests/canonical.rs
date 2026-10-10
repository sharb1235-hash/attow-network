use attow_codec::{
    Canonical, CodecError, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
    references,
};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

fn object(payload: Value) -> Object {
    Object {
        refs: BTreeSet::new(),
        payload,
    }
}

#[test]
fn golden_cbor_values() -> Result<(), CodecError> {
    let cases = [
        (Value::Unsigned(0), vec![0]),
        (Value::Unsigned(23), vec![23]),
        (Value::Unsigned(24), vec![0x18, 24]),
        (Value::Unsigned(256), vec![0x19, 1, 0]),
        (Value::Negative(0), vec![0x20]),
        (Value::Bytes(vec![0, 255]), vec![0x42, 0, 255]),
        (Value::Text("hi".into()), vec![0x62, b'h', b'i']),
        (Value::Bool(false), vec![0xf4]),
        (Value::Null, vec![0xf6]),
    ];
    for (value, bytes) in cases {
        assert_eq!(value.encode()?, bytes);
        assert_eq!(Value::decode(&bytes, &Limits::default())?, value);
    }
    Ok(())
}

#[test]
fn golden_versioned_object_and_sorted_map() -> Result<(), CodecError> {
    let value = object(Value::Null);
    let expected = [0x84, 0x1a, 0x41, 0x54, 0x54, 0x4f, 0x01, 0x80, 0xf6];
    assert_eq!(value.encode()?, expected);
    assert_eq!(Object::decode(&expected, &Limits::default())?, value);
    let first = BTreeMap::from([
        ("aa".into(), Value::Unsigned(1)),
        ("b".into(), Value::Unsigned(24)),
    ]);
    let mut second = BTreeMap::new();
    second.insert("b".into(), Value::Unsigned(24));
    second.insert("aa".into(), Value::Unsigned(1));
    assert_eq!(
        Value::Map(first.clone()).encode()?,
        Value::Map(second).encode()?
    );
    assert_eq!(
        Value::Map(first).encode()?,
        [0xa2, 0x61, b'b', 0x18, 24, 0x62, b'a', b'a', 1]
    );
    Ok(())
}

#[test]
fn malformed_and_ambiguous_values_are_rejected() {
    let cases: &[(&[u8], CodecError)] = &[
        (&[0x18, 0], CodecError::NonCanonical),
        (&[0x9f, 0xff], CodecError::NonCanonical),
        (&[0xbf, 0xff], CodecError::NonCanonical),
        (&[0x5f, 0xff], CodecError::NonCanonical),
        (&[0xa2, 0x61, b'a', 0, 0x61, b'a', 1], CodecError::Duplicate),
        (&[0, 0], CodecError::TrailingBytes),
        (&[0x19, 1], CodecError::Truncated),
        (&[0x42, 1], CodecError::Truncated),
        (&[0x61, 0xff], CodecError::InvalidText),
        (&[0xfa, 0, 0, 0, 0], CodecError::UnsupportedType),
        (&[0xc0, 0], CodecError::UnsupportedType),
    ];
    for (bytes, expected) in cases {
        assert_eq!(
            Value::decode(bytes, &Limits::default()),
            Err(expected.clone()),
            "input {bytes:x?}"
        );
    }
}

#[test]
fn every_truncated_object_prefix_fails() -> Result<(), CodecError> {
    let bytes = object(Value::Bytes(vec![0; 128])).encode()?;
    for end in 0..bytes.len() {
        assert!(Object::decode(&bytes[..end], &Limits::default()).is_err());
    }
    Ok(())
}

#[test]
fn decoder_enforces_bounds_before_allocation() {
    assert_eq!(
        Value::decode(
            &[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            &Limits::default()
        ),
        Err(CodecError::ItemLimit)
    );
    let small = Limits {
        object_bytes: 2,
        ..Limits::default()
    };
    assert_eq!(
        Value::decode(&[0x82, 0, 1], &small),
        Err(CodecError::ByteLimit)
    );
    let small = Limits {
        items: 2,
        ..Limits::default()
    };
    assert_eq!(
        Value::decode(&[0x82, 0, 1], &small),
        Err(CodecError::ItemLimit)
    );
    let small = Limits {
        depth: 2,
        ..Limits::default()
    };
    assert_eq!(
        Value::decode(&[0x81, 0x81, 0], &small),
        Err(CodecError::DepthLimit)
    );
    let enormous = Limits {
        depth: usize::MAX,
        ..Limits::default()
    };
    assert_eq!(
        Value::decode(&[0], &enormous),
        Err(CodecError::InvalidLimits)
    );
}

#[test]
fn encoding_is_bounded_too() {
    let mut value = Value::Null;
    for _ in 0..100 {
        value = Value::Array(vec![value]);
    }
    assert_eq!(value.encode(), Err(CodecError::DepthLimit));
    assert_eq!(
        Value::Bytes(vec![0; 1024 * 1024]).encode(),
        Err(CodecError::ByteLimit)
    );
}

#[test]
fn nested_large_array_claims_do_not_preallocate_each_claim() {
    let mut bytes = Vec::new();
    for level in 0..64_u32 {
        bytes.push(0x9a);
        bytes.extend_from_slice(&(99_999 - level).to_be_bytes());
    }
    // Enough remaining input exists to make each individual length claim
    // plausible. No array has completed a child before the depth failure.
    bytes.resize(bytes.len() + 100_000, 0);
    assert_eq!(
        Value::decode(&bytes, &Limits::default()),
        Err(CodecError::DepthLimit)
    );
}

#[test]
fn reference_schema_is_explicit_and_unknown_fails_closed() -> Result<(), CodecError> {
    let value = Object {
        refs: BTreeSet::from([ObjectId([2; 32]), ObjectId([1; 32])]),
        payload: Value::Null,
    };
    let bytes = value.encode()?;
    assert_eq!(
        references(OBJECT_KIND, OBJECT_SCHEMA, &bytes, &Limits::default())?,
        vec![ObjectId([1; 32]), ObjectId([2; 32])]
    );
    assert_eq!(
        references(999, 1, &bytes, &Limits::default()),
        Err(CodecError::UnknownSchema)
    );
    assert_eq!(
        references(OBJECT_KIND, 2, &bytes, &Limits::default()),
        Err(CodecError::UnknownSchema)
    );
    assert_eq!(
        Object::decode(&[0x80], &Limits::default()),
        Err(CodecError::InvalidObject)
    );
    Ok(())
}

#[test]
fn content_hash_binds_kind_schema_and_length() {
    let id = object_id(1, 1, b"abc");
    assert_ne!(id, object_id(2, 1, b"abc"));
    assert_ne!(id, object_id(1, 2, b"abc"));
    assert_ne!(id, object_id(1, 1, b"abc\0"));
}

#[test]
fn committed_compatibility_vectors_are_stable() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = include_str!("../../../fixtures/original/protocol/codec-v1.txt");
    let mut count = 0;
    for line in fixture
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 5);
        let kind: u16 = fields[1].parse()?;
        let schema: u16 = fields[2].parse()?;
        let hex = fields[3];
        assert!(hex.len().is_multiple_of(2) && hex.is_ascii());
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16))
            .collect::<Result<_, _>>()?;
        let expected: ObjectId = fields[4].parse()?;
        assert_eq!(
            object_id(kind, schema, &bytes),
            expected,
            "{} hash",
            fields[0]
        );
        let object = Object::decode(&bytes, &Limits::default())?;
        assert_eq!(object.encode()?, bytes, "{} bytes", fields[0]);
        assert_eq!(expected.to_string(), fields[4]);
        count += 1;
    }
    assert_eq!(count, 3);
    Ok(())
}

#[test]
fn identifiers_require_exact_ascii_hex() -> Result<(), CodecError> {
    let id = ObjectId([0xab; 32]);
    assert_eq!(id.to_string().parse::<ObjectId>()?, id);
    assert_eq!(id.to_string().to_uppercase().parse::<ObjectId>()?, id);
    for bad in [
        String::new(),
        "0".repeat(63),
        "g".repeat(64),
        "é".repeat(32),
    ] {
        assert_eq!(bad.parse::<ObjectId>(), Err(CodecError::InvalidId));
    }
    Ok(())
}

fn values() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<u64>().prop_map(Value::Unsigned),
        any::<u64>().prop_map(Value::Negative),
        proptest::collection::vec(any::<u8>(), 0..40).prop_map(Value::Bytes),
        "[a-zA-Z0-9 ]{0,40}".prop_map(Value::Text)
    ];
    leaf.prop_recursive(5, 256, 16, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            proptest::collection::btree_map("[a-z]{1,10}", inner, 0..8).prop_map(Value::Map)
        ]
    })
}

proptest! {
    #[test]
    fn values_roundtrip(value in values()) {
        let bytes = value.encode()?;
        let decoded = Value::decode(&bytes, &Limits::default())?;
        prop_assert_eq!(&decoded, &value);
        prop_assert_eq!(decoded.encode()?, bytes);
    }

    #[test]
    fn accepted_bytes_equal_reencoding(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        if let Ok(value) = Value::decode(&bytes, &Limits::default()) { prop_assert_eq!(value.encode()?, bytes); }
    }
}
