#![no_main]

use libfuzzer_sys::fuzz_target;
use mainframe_env_ir::{CicsPlanLimits, decode_cics_effect_plan, encode_cics_effect_plan};

fuzz_target!(|data: &[u8]| {
    let limits = CicsPlanLimits {
        max_encoded_bytes: 4096,
        max_operands: 32,
        max_options: 16,
        max_outputs: 32,
        max_literal_bytes: 1024,
        max_qualified_name_bytes: 256,
    };
    if let Ok(plan) = decode_cics_effect_plan(data, limits) {
        let canonical = encode_cics_effect_plan(&plan, limits).expect("decoded plan re-encodes");
        assert_eq!(decode_cics_effect_plan(&canonical, limits), Ok(plan));
        if data.get(4..6) == Some(&[0, 2]) {
            assert_eq!(canonical, data);
        }
    }
});
