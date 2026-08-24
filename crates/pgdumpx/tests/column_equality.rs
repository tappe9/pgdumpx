use pgdumpx::{
    Archive, ColumnEqualityResult, FieldRef, OwnedField, PgDumpError, ScanLimits,
};
use std::{io::Cursor, path::PathBuf};

const FIRST_PHYSICAL_ROW: &[u8] = b"1\tEARLY-100\tcustomer-a\tplain\t\n";

#[test]
fn equality_search_matches_logical_bytes_at_early_middle_late_and_absent_positions() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        assert_match_id(
            search(
                fixture_name,
                b"order_number",
                FieldRef::Bytes(b"EARLY-100"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"1",
        );
        assert_match_id(
            search(
                fixture_name,
                b"order_number",
                FieldRef::Bytes(b"MIDDLE-400"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"4",
        );
        assert_match_id(
            search(
                fixture_name,
                b"order_number",
                FieldRef::Bytes(b"LATE-700"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"7",
        );
        assert_eq!(
            search(
                fixture_name,
                b"order_number",
                FieldRef::Bytes(b"ABSENT-999"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            ColumnEqualityResult::NoMatch,
        );

        assert_match_id(
            search(
                fixture_name,
                b"note",
                FieldRef::Bytes(b"tab\tvalue"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"2",
        );
    }
}

#[test]
fn equality_search_distinguishes_null_empty_and_literal_backslash_n_bytes() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        assert_match_id(
            search(
                fixture_name,
                b"note",
                FieldRef::Null,
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"4",
        );
        assert_match_id(
            search(
                fixture_name,
                b"note",
                FieldRef::Bytes(b""),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            b"5",
        );
        assert_eq!(
            search(
                fixture_name,
                b"note",
                FieldRef::Bytes(br"\N"),
                ScanLimits::unlimited(),
            )
            .unwrap(),
            ColumnEqualityResult::NoMatch,
        );
    }
}

#[test]
fn unknown_column_is_resolved_before_scanning_and_stays_distinct_from_no_match() {
    let bytes = fixture("pg18-none-copy-basic.dump");
    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let mut rows = archive.table_rows(b"public", b"orders").unwrap();

    let result = rows
        .find_first_equal_with_limits(
            ScanLimits::unlimited().with_max_rows(0),
            b"missing_column",
            FieldRef::Bytes(b"anything"),
        )
        .unwrap();

    assert_eq!(result, ColumnEqualityResult::ColumnNotFound);
    assert_eq!(
        search(
            "pg18-none-copy-basic.dump",
            b"order_number",
            FieldRef::Bytes(b"ABSENT-999"),
            ScanLimits::unlimited(),
        )
        .unwrap(),
        ColumnEqualityResult::NoMatch,
    );
}

#[test]
fn equality_search_preserves_row_and_decompressed_byte_limit_accounting() {
    let first_row_bytes = u64::try_from(FIRST_PHYSICAL_ROW.len()).unwrap();

    assert_match_id(
        search(
            "pg18-none-copy-basic.dump",
            b"order_number",
            FieldRef::Bytes(b"EARLY-100"),
            ScanLimits::unlimited()
                .with_max_rows(1)
                .with_max_decompressed_bytes(first_row_bytes),
        )
        .unwrap(),
        b"1",
    );

    assert!(matches!(
        search(
            "pg18-none-copy-basic.dump",
            b"order_number",
            FieldRef::Bytes(b"MIDDLE-400"),
            ScanLimits::unlimited().with_max_rows(3),
        ),
        Err(PgDumpError::ScanRowLimitExceeded {
            row: 4,
            limit: 3,
            consumed: 4,
        })
    ));

    assert!(matches!(
        search(
            "pg18-none-copy-basic.dump",
            b"order_number",
            FieldRef::Bytes(b"EARLY-100"),
            ScanLimits::unlimited().with_max_decompressed_bytes(first_row_bytes - 1),
        ),
        Err(PgDumpError::ScanDecompressedByteLimitExceeded {
            row: 1,
            limit,
            consumed,
            ..
        }) if limit == first_row_bytes - 1 && consumed == first_row_bytes
    ));
}

#[test]
fn equality_search_returns_an_owned_row_that_survives_reader_teardown() {
    let row = {
        let bytes = fixture("pg18-gzip-copy-basic.dump");
        let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        match rows
            .find_first_equal(b"order_number", FieldRef::Bytes(b"MIDDLE-400"))
            .unwrap()
        {
            ColumnEqualityResult::Match(row) => row,
            other => panic!("expected matching owned row, got {other:?}"),
        }
    };

    assert_eq!(row.field(0), Some(&OwnedField::Bytes(b"4".to_vec())));
    assert_eq!(
        row.field(1),
        Some(&OwnedField::Bytes(b"MIDDLE-400".to_vec()))
    );
    assert_eq!(row.field(3), Some(&OwnedField::Null));
}

fn search(
    fixture_name: &str,
    column: &[u8],
    expected: FieldRef<'static>,
    scan_limits: ScanLimits,
) -> Result<ColumnEqualityResult, PgDumpError> {
    let bytes = fixture(fixture_name);
    let mut archive = Archive::open(Cursor::new(bytes))?;
    let mut rows = archive.table_rows(b"public", b"orders")?;
    rows.find_first_equal_with_limits(scan_limits, column, expected)
}

fn assert_match_id(result: ColumnEqualityResult, expected_id: &[u8]) {
    let ColumnEqualityResult::Match(row) = result else {
        panic!("expected matching row, got {result:?}");
    };
    assert_eq!(row.field(0), Some(&OwnedField::Bytes(expected_id.to_vec())));
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed fixture must be readable")
}
