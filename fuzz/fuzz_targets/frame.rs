#![no_main]

use attow_protocol::{ProtocolLimits, decode_frame, decode_header, encode_frame};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 131_072 {
        return;
    }
    for limits in [
        ProtocolLimits::default(),
        ProtocolLimits {
            control_bytes: 256,
            chunk_bytes: 16,
            object_bytes: 256,
            max_ids: 2,
        },
    ] {
        if let Some(header) = bytes.get(..5) {
            let header: &[u8; 5] = header.try_into().expect("five-byte header");
            let _ = decode_header(header, &limits);
        }
        if let Ok(message) = decode_frame(bytes, &limits) {
            assert_eq!(encode_frame(&message).expect("valid message"), bytes);
        }
    }
});
