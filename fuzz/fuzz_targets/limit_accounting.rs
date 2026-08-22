#![no_main]

mod support;

use libfuzzer_sys::fuzz_target;
use pgdumpx::{Archive, CopyRowReader, EntryReadLimits, ScanLimits};
use std::io::{Cursor, Read};
use support::{MAX_INPUT_BYTES, build_raw_payload_archive, fuzz_limits};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let selector = data.first().copied().unwrap_or(0);
    let payload = data.get(1..).unwrap_or_default();
    let payload_len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    let byte_limit = boundary(selector, payload_len);
    let row_limit = boundary(selector >> 2, 4);

    let scan_limits = ScanLimits::unlimited()
        .with_max_rows(row_limit)
        .with_max_decompressed_bytes(byte_limit);
    let mut rows = CopyRowReader::with_limits_and_scan_limits(
        Cursor::new(payload),
        fuzz_limits(),
        scan_limits,
    );
    loop {
        match rows.next_row() {
            Ok(Some(row)) => {
                let _ = row.len();
            }
            Ok(None) | Err(_) => break,
        }
    }

    let bytes = build_raw_payload_archive(payload);
    let Ok(mut archive) = Archive::open_with_limits(Cursor::new(bytes), fuzz_limits()) else {
        return;
    };
    let Some(id) = archive.entries().first().map(|entry| entry.id()) else {
        return;
    };
    let limits = EntryReadLimits::unlimited().with_max_decompressed_bytes(byte_limit);
    let Ok(Some(mut reader)) = archive.entry_reader_with_limits(id, limits) else {
        return;
    };
    let mut output = [0_u8; 257];
    loop {
        match reader.read(&mut output) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
});

fn boundary(selector: u8, value: u64) -> u64 {
    match selector & 0b11 {
        0 => 0,
        1 => value.saturating_sub(1),
        2 => value,
        _ => value.saturating_add(1),
    }
}
