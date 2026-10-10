//! Reproduce the original version-one compatibility vectors.
use attow_codec::{
    Canonical, CodecError, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<(), CodecError> {
    println!("# Attow Network codec v1 original compatibility vectors");
    println!("# name kind schema canonical_cbor_hex object_id_hex");
    let cases = [
        (
            "null",
            Object {
                refs: BTreeSet::new(),
                payload: Value::Null,
            },
        ),
        (
            "map",
            Object {
                refs: BTreeSet::new(),
                payload: Value::Map(BTreeMap::from([
                    ("aa".into(), Value::Unsigned(1)),
                    ("b".into(), Value::Unsigned(24)),
                ])),
            },
        ),
        (
            "reference",
            Object {
                refs: BTreeSet::from([ObjectId([1; 32])]),
                payload: Value::Bytes(vec![0, 255]),
            },
        ),
    ];
    for (name, object) in cases {
        let bytes = object.encode()?;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        println!(
            "{name} {OBJECT_KIND} {OBJECT_SCHEMA} {hex} {}",
            object_id(OBJECT_KIND, OBJECT_SCHEMA, &bytes)
        );
    }
    Ok(())
}
