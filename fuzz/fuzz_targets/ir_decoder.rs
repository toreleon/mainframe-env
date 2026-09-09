#![no_main]

use libfuzzer_sys::fuzz_target;
use mainframe_env_ir::{CodecLimits, decode_binary, parse_text};

fuzz_target!(|data: &[u8]| {
    let mut limits = CodecLimits::default();
    limits.max_envelope_bytes = 4096;
    limits.max_payload_bytes = 4096;
    limits.max_string_bytes = 256;
    let _ = decode_binary(data, limits);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse_text(text, limits);
    }
});
