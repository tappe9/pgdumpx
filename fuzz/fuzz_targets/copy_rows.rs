#![no_main]

mod support;

use libfuzzer_sys::fuzz_target;
use pgdumpx::{CopyRowReader, FieldRef, ScanLimits};
use std::io::Cursor;
use support::{MAX_INPUT_BYTES, fuzz_limits};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let scan_limits = ScanLimits::unlimited()
        .with_max_rows(256)
        .with_max_decompressed_bytes(MAX_INPUT_BYTES as u64);
    let mut reader = CopyRowReader::with_limits_and_scan_limits(
        Cursor::new(data),
        fuzz_limits(),
        scan_limits,
    );

    loop {
        match reader.next_row() {
            Ok(Some(row)) => {
                for field in row.fields() {
                    match field {
                        FieldRef::Null => {}
                        FieldRef::Bytes(bytes) => {
                            let _ = bytes.len();
                        }
                    }
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
});
