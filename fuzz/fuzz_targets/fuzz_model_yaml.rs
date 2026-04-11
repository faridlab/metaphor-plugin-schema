//! Fuzz target for YAML model schema parser
//!
//! Run with: `cargo +nightly fuzz run fuzz_model_yaml -- -max_total_time=300`

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // We only care that the parser doesn't panic — errors are expected
        let _ = metaphor_schema::parser::parse_model_yaml_str(s);
    }
});
