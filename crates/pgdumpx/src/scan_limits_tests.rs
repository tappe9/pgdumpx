use crate::{
    Archive, CopyRowReader, ErrorCategory, FieldRef, Limits, OwnedField, PgDumpError,
    ResourceLimit, ScanLimits,
};
use std::{
    cell::Cell,
    fs::File,
    io::{self, BufReader, Cursor, Read},
    path::PathBuf,
};

const TWO_ROWS: &[u8] = b"a\nb\n\\.\n";
const ESCAPED_ROW_STREAM: &[u8] = b"a\\tb\tc\n\\.\n";

#[test]
fn scan_limits_are_optional_opaque_operation_budgets() {
    let unlimited = ScanLimits::unlimited();
    assert_eq!(unlimited, ScanLimits::default());
    assert_eq!(unlimited.max_rows(), None);
    assert_eq!(unlimited.max_decompressed_bytes(), None);

    let bounded = unlimited.with_max_rows(3).with_max_decompressed_bytes(17);
    assert_eq!(bounded.max_rows(), Some(3));
    assert_eq!(bounded.max_decompressed_bytes(), Some(17));
}

#[test]
fn standalone_row_budget_accepts_below_and_exact_and_rejects_crossing_row() {
    for limit in [2, 3] {
        let scan_limits = ScanLimits::unlimited().with_max_rows(limit);
        let mut rows = CopyRowReader::with_scan_limits(Cursor::new(TWO_ROWS), scan_limits);

        assert!(rows.next_row().unwrap().is_some());
        assert!(rows.next_row().unwrap().is_some());
        assert!(rows.next_row().unwrap().is_none());
    }

    let scan_limits = ScanLimits::unlimited().with_max_rows(1);
    let mut rows = CopyRowReader::with_scan_limits(Cursor::new(TWO_ROWS), scan_limits);
    assert!(rows.next_row().unwrap().is_some());

    let error = rows.next_row().unwrap_err();
    assert!(matches!(
        &error,
        PgDumpError::ScanRowLimitExceeded {
            row: 2,
            limit: 1,
            consumed: 2,
        }
    ));
    assert_eq!(error.category(), ErrorCategory::Resource);
    let context = error.limit_context().unwrap();
    assert_eq!(context.resource(), ResourceLimit::ScanRows);
    assert_eq!(context.limit(), 1);
    assert_eq!(context.consumed(), 2);
    assert_eq!(rows.consumed_input_bytes(), 4);
}

#[test]
fn decompressed_byte_budget_is_exact_and_counts_spelling_separators_and_terminator() {
    for limit in [10, 11] {
        let scan_limits = ScanLimits::unlimited().with_max_decompressed_bytes(limit);
        let mut rows =
            CopyRowReader::with_scan_limits(Cursor::new(ESCAPED_ROW_STREAM), scan_limits);

        let row = rows.next_row().unwrap().unwrap();
        assert_eq!(row.field(0), Some(FieldRef::Bytes(b"a\tb")));
        assert_eq!(row.field(1), Some(FieldRef::Bytes(b"c")));
        assert_eq!(rows.consumed_input_bytes(), 7);
        assert!(rows.next_row().unwrap().is_none());
        assert_eq!(rows.consumed_input_bytes(), 10);
    }

    let scan_limits = ScanLimits::unlimited().with_max_decompressed_bytes(9);
    let mut rows = CopyRowReader::with_scan_limits(Cursor::new(ESCAPED_ROW_STREAM), scan_limits);
    assert!(rows.next_row().unwrap().is_some());
    let error = rows.next_row().unwrap_err();
    assert!(matches!(
        &error,
        PgDumpError::ScanDecompressedByteLimitExceeded {
            row: 2,
            limit: 9,
            consumed: 10,
            byte_offset: 10,
        }
    ));
    let context = error.limit_context().unwrap();
    assert_eq!(context.resource(), ResourceLimit::ScanDecompressedBytes);
    assert_eq!(context.limit(), 9);
    assert_eq!(context.consumed(), 10);

    let scan_limits = ScanLimits::unlimited().with_max_decompressed_bytes(6);
    let mut rows = CopyRowReader::with_scan_limits(Cursor::new(ESCAPED_ROW_STREAM), scan_limits);
    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::ScanDecompressedByteLimitExceeded {
            row: 1,
            limit: 6,
            consumed: 7,
            byte_offset: 7,
        }
    ));
}

#[test]
fn no_match_completion_requires_budget_for_a_consumed_copy_terminator() {
    let stream = b"a\n\\.\n";

    let exact = ScanLimits::unlimited().with_max_decompressed_bytes(5);
    let mut rows = CopyRowReader::with_scan_limits(Cursor::new(stream), exact);
    let found = rows.find_first(|_| false).unwrap();
    assert!(found.is_none());
    assert_eq!(rows.consumed_input_bytes(), 5);

    let too_small = ScanLimits::unlimited().with_max_decompressed_bytes(4);
    let mut rows = CopyRowReader::with_scan_limits(Cursor::new(stream), too_small);
    assert!(matches!(
        rows.find_first(|_| false).unwrap_err(),
        PgDumpError::ScanDecompressedByteLimitExceeded {
            row: 2,
            limit: 4,
            consumed: 5,
            byte_offset: 5,
        }
    ));
}

#[test]
fn find_first_with_limits_counts_matches_and_never_evaluates_crossing_rows() {
    let stream = b"a\nmatch\nlater\n\\.\n";
    let evaluated = Cell::new(0_u64);
    let mut rows = CopyRowReader::new(Cursor::new(stream));
    let found = rows
        .find_first_with_limits(
            ScanLimits::unlimited()
                .with_max_rows(2)
                .with_max_decompressed_bytes(8),
            |row| {
                evaluated.set(evaluated.get() + 1);
                row.field(0) == Some(FieldRef::Bytes(b"match"))
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(evaluated.get(), 2);
    assert_eq!(found.field(0), Some(&OwnedField::Bytes(b"match".to_vec())));
    assert_eq!(rows.consumed_input_bytes(), 8);

    let evaluated = Cell::new(0_u64);
    let mut rows = CopyRowReader::new(Cursor::new(stream));
    let error = rows
        .find_first_with_limits(ScanLimits::unlimited().with_max_rows(1), |row| {
            evaluated.set(evaluated.get() + 1);
            row.field(0) == Some(FieldRef::Bytes(b"match"))
        })
        .unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::ScanRowLimitExceeded {
            row: 2,
            limit: 1,
            consumed: 2,
        }
    ));
    assert_eq!(evaluated.get(), 1);

    let evaluated = Cell::new(0_u64);
    let mut rows = CopyRowReader::new(Cursor::new(stream));
    let error = rows
        .find_first_with_limits(
            ScanLimits::unlimited().with_max_decompressed_bytes(7),
            |row| {
                evaluated.set(evaluated.get() + 1);
                row.field(0) == Some(FieldRef::Bytes(b"match"))
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::ScanDecompressedByteLimitExceeded {
            row: 2,
            limit: 7,
            consumed: 8,
            byte_offset: 8,
        }
    ));
    assert_eq!(evaluated.get(), 1);
}

#[test]
fn scan_accounting_is_independent_of_underlying_read_segmentation() {
    let limits = Limits::default()
        .with_max_row_bytes(64)
        .with_max_fields_per_row(8);
    let scan_limits = ScanLimits::unlimited()
        .with_max_rows(2)
        .with_max_decompressed_bytes(7);

    let contiguous = consume_all(CopyRowReader::with_limits_and_scan_limits(
        Cursor::new(TWO_ROWS),
        limits,
        scan_limits,
    ));
    let one_byte = consume_all(CopyRowReader::with_limits_and_scan_limits(
        ShortRead::new(Cursor::new(TWO_ROWS), 1),
        limits,
        scan_limits,
    ));

    assert_eq!(contiguous, (2, 7));
    assert_eq!(one_byte, contiguous);
}

#[test]
fn checked_scan_row_counter_overflow_is_typed_and_controlled() {
    let mut rows = CopyRowReader::with_scan_state_for_test(
        Cursor::new(b"a\n".as_slice()),
        Limits::default(),
        ScanLimits::unlimited(),
        u64::MAX,
    );

    let error = rows.next_row().unwrap_err();
    assert!(matches!(
        &error,
        PgDumpError::ScanRowCountOverflow {
            row: 1,
            consumed: u64::MAX,
        }
    ));
    assert_eq!(error.category(), ErrorCategory::Arithmetic);
}

#[test]
fn archive_integrated_search_uses_the_same_scan_budget_path() {
    let first_physical_row = b"1\tEARLY-100\tcustomer-a\tplain\t\n";
    let first_row_bytes = u64::try_from(first_physical_row.len()).unwrap();

    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let file = File::open(fixture_path(fixture_name)).unwrap();
        let mut archive = Archive::open(BufReader::new(file)).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        let evaluated = Cell::new(0_u64);
        let error = rows
            .find_first_with_limits(ScanLimits::unlimited().with_max_rows(1), |_| {
                evaluated.set(evaluated.get() + 1);
                false
            })
            .unwrap_err();
        assert!(matches!(
            error,
            PgDumpError::ScanRowLimitExceeded {
                row: 2,
                limit: 1,
                consumed: 2,
            }
        ));
        assert_eq!(evaluated.get(), 1);

        let file = File::open(fixture_path(fixture_name)).unwrap();
        let mut archive = Archive::open(BufReader::new(file)).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        let order_number = rows.column_index(b"order_number").unwrap().unwrap();
        let found = rows
            .find_first_with_limits(
                ScanLimits::unlimited()
                    .with_max_rows(1)
                    .with_max_decompressed_bytes(first_row_bytes),
                |row| row.field(order_number) == Some(FieldRef::Bytes(b"EARLY-100")),
            )
            .unwrap()
            .unwrap();
        assert_eq!(found.field(0), Some(&OwnedField::Bytes(b"1".to_vec())));
        assert_eq!(rows.consumed_input_bytes(), first_row_bytes);
    }
}

fn consume_all<R: Read>(mut rows: CopyRowReader<R>) -> (u64, u64) {
    let mut count = 0_u64;
    while rows.next_row().unwrap().is_some() {
        count += 1;
    }
    (count, rows.consumed_input_bytes())
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}

#[derive(Debug)]
struct ShortRead<R> {
    inner: R,
    max_read: usize,
}

impl<R> ShortRead<R> {
    const fn new(inner: R, max_read: usize) -> Self {
        Self { inner, max_read }
    }
}

impl<R: Read> Read for ShortRead<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let limit = output.len().min(self.max_read);
        self.inner.read(&mut output[..limit])
    }
}
