//! Public facade regressions for content identity; no database or runtime needed.
use attow_network::codec::object_id;

#[test]
fn identity_binds_kind_schema_and_exact_payload() {
    let id = object_id(1, 1, b"same payload");
    assert_eq!(id, object_id(1, 1, b"same payload"));
    assert_ne!(id, object_id(2, 1, b"same payload"));
    assert_ne!(id, object_id(1, 2, b"same payload"));
    assert_ne!(id, object_id(1, 1, b"same payload\0"));
    assert_ne!(object_id(1, 1, b""), object_id(1, 1, &[0]));
}

#[test]
fn fixed_width_domain_fields_cannot_alias_concatenation() {
    assert_ne!(object_id(1, 256, &[0]), object_id(256, 1, &[0]));
    assert_ne!(object_id(1, 1, &[1, 0]), object_id(1, 256, &[1]));
}
