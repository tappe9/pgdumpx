#![no_main]

mod support;

use libfuzzer_sys::fuzz_target;
use pgdumpx::Archive;
use std::io::Cursor;
use support::{MAX_INPUT_BYTES, fuzz_limits};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let _ = Archive::open_with_limits(Cursor::new(data), fuzz_limits());
});
