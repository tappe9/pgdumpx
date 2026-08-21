use crate::{PgDumpError, raw_entry::checked_decompressed_count};

#[test]
fn raw_entry_decompressed_counter_overflow_is_typed() {
    let error = checked_decompressed_count(7, u64::MAX, 1).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::EntryDecompressedByteCountOverflow {
            dump_id: 7,
            consumed: u64::MAX,
            increment: 1,
        }
    ));
}
