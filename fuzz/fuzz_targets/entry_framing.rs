#![no_main]

mod support;

use libfuzzer_sys::fuzz_target;
use pgdumpx::Archive;
use std::io::{Cursor, Read};
use support::{MAX_INPUT_BYTES, build_data_block_archive, fuzz_limits};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let bytes = build_data_block_archive(data);
    let Ok(mut archive) = Archive::open_with_limits(Cursor::new(bytes), fuzz_limits()) else {
        return;
    };
    let Some(id) = archive.entries().first().map(|entry| entry.id()) else {
        return;
    };
    let Ok(Some(mut reader)) = archive.entry_reader(id) else {
        return;
    };

    let mut output = [0_u8; 1024];
    loop {
        match reader.read(&mut output) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
});
