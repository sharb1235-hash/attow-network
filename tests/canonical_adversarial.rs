//! Independent public-API probes of hostile canonical object inputs.
use attow_network::codec::{Canonical, Limits, Object, Value, references};
use std::collections::BTreeSet;

const EMPTY_OBJECT: &[u8] = &[0x84, 0x1a, 0x41, 0x54, 0x54, 0x4f, 0x01, 0x80, 0xf6];

#[test]
fn original_empty_object_vector_and_all_truncations() -> Result<(), Box<dyn std::error::Error>> {
    let expected = Object {
        refs: BTreeSet::new(),
        payload: Value::Null,
    };
    assert_eq!(expected.encode()?, EMPTY_OBJECT);
    assert_eq!(Object::decode(EMPTY_OBJECT, &Limits::default())?, expected);
    for length in 0..EMPTY_OBJECT.len() {
        assert!(Object::decode(&EMPTY_OBJECT[..length], &Limits::default()).is_err());
    }
    let mut extra = EMPTY_OBJECT.to_vec();
    extra.push(0);
    assert!(Object::decode(&extra, &Limits::default()).is_err());
    Ok(())
}

#[test]
fn hostile_length_headers_fail_without_sized_allocations() {
    // Definite lengths claiming u64::MAX bytes/items with no body.
    for header in [0x5b, 0x7b, 0x9b, 0xbb] {
        let mut data = EMPTY_OBJECT[..EMPTY_OBJECT.len() - 1].to_vec();
        data.push(header);
        data.extend_from_slice(&[0xff; 8]);
        assert!(Object::decode(&data, &Limits::default()).is_err());
    }
}

#[test]
fn duplicate_map_keys_and_non_shortest_schema_are_rejected() {
    let mut duplicate = EMPTY_OBJECT[..EMPTY_OBJECT.len() - 1].to_vec();
    duplicate.extend_from_slice(&[0xa2, 0x61, b'a', 0x00, 0x61, b'a', 0x01]);
    assert!(Object::decode(&duplicate, &Limits::default()).is_err());
    let alternate = [0x84, 0x1a, 0x41, 0x54, 0x54, 0x4f, 0x18, 0x01, 0x80, 0xf6];
    assert!(Object::decode(&alternate, &Limits::default()).is_err());
}

#[test]
fn caller_limits_cannot_remove_stack_safety_boundary() {
    let mut nested = EMPTY_OBJECT[..EMPTY_OBJECT.len() - 1].to_vec();
    nested.extend(std::iter::repeat_n(0x81, 10_000));
    nested.push(0);
    let limits = Limits {
        object_bytes: usize::MAX,
        depth: usize::MAX,
        items: usize::MAX,
    };
    assert!(Object::decode(&nested, &limits).is_err());
    assert!(Object::decode(&nested, &Limits::default()).is_err());
}

#[test]
fn unknown_reference_schemas_cannot_be_treated_as_leaves() {
    assert!(references(1, 2, EMPTY_OBJECT, &Limits::default()).is_err());
    assert!(references(999, 1, EMPTY_OBJECT, &Limits::default()).is_err());
}
