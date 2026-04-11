//! Fuzz target for YAML hook schema parser
//!
//! Run with: `cargo +nightly fuzz run fuzz_hook_yaml -- -max_total_time=300`

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = metaphor_schema::parser::parse_hook_yaml_str(s);
    }
});
