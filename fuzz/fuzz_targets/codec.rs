#![no_main]

use attow_codec::{Canonical, Limits, Object, Value};
use libfuzzer_sys::fuzz_target;

fn accepted_input_roundtrips<T: Canonical + PartialEq + std::fmt::Debug>(
    bytes: &[u8],
    limits: &Limits,
) {
    if let Ok(value) = T::decode(bytes, limits) {
        let encoded = value.encode().expect("accepted input must encode");
        assert_eq!(encoded, bytes, "accepted bytes must already be canonical");
        let decoded = T::decode(&encoded, limits).expect("accepted input must decode again");
        assert_eq!(decoded, value, "canonical values must roundtrip");
    }
}

fuzz_target!(|bytes: &[u8]| {
    // Keep the harness bounded even when invoked without the documented flags.
    if bytes.len() > 65_536 {
        return;
    }
    for limits in [
        Limits {
            object_bytes: 65_536,
            depth: 32,
            items: 4_096,
        },
        Limits {
            object_bytes: 256,
            depth: 4,
            items: 64,
        },
        Limits {
            object_bytes: 0,
            depth: 0,
            items: 0,
        },
    ] {
        accepted_input_roundtrips::<Value>(bytes, &limits);
        accepted_input_roundtrips::<Object>(bytes, &limits);
    }
});
