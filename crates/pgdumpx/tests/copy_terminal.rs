use pgdumpx::{Archive, CopyRowReader, Limits, PgDumpError};
use std::{cell::Cell, io::Cursor, path::PathBuf};

#[test]
fn standalone_reader_never_exposes_bytes_after_a_rejected_row() {
    let limits = Limits::default().with_max_row_bytes(3);
    let mut rows = CopyRowReader::with_limits(Cursor::new(b"abcd\nnext\n".as_slice()), limits);

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyRowByteLimitExceeded {
            row: 1,
            limit: 3,
            actual: 4,
            ..
        }
    ));
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn archive_backed_reader_and_searches_are_terminal_after_a_parse_error() {
    let limits = Limits::default().with_max_row_bytes(3);
    let mut archive =
        Archive::open_with_limits(Cursor::new(fixture("pg18-none-copy-basic.dump")), limits)
            .unwrap();
    let mut rows = archive.table_rows(b"public", b"orders").unwrap();

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyRowByteLimitExceeded {
            row: 1,
            limit: 3,
            actual: 4,
            ..
        }
    ));
    assert!(rows.next_row().unwrap().is_none());

    let predicate_calls = Cell::new(0_u32);
    assert!(
        rows.find_first(|_| {
            predicate_calls.set(predicate_calls.get() + 1);
            true
        })
        .unwrap()
        .is_none()
    );
    assert_eq!(predicate_calls.get(), 0);
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}
