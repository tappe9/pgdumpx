#![no_main]

mod support;

use libfuzzer_sys::fuzz_target;
use pgdumpx::Archive;
use std::io::Cursor;
use support::{MAX_INPUT_BYTES, build_toc_archive, fuzz_limits};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let archive = build_toc_archive(data);
    let _ = Archive::open_with_limits(Cursor::new(archive), fuzz_limits());
});
