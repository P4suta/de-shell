#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| deshell_fuzz::fuzz_schema(data));
