use pgdumpx::{Archive, CopyRowReader, FieldRef, PgDumpError, Row};
use std::{
    io::{self, Cursor, Read},
    path::PathBuf,
};

#[test]
fn parses_official_none_fixture_into_borrowed_logical_fields() {
    assert_official_fixture_rows("pg18-none-copy-basic.dump");
}

#[test]
fn parses_official_gzip_fixture_into_borrowed_logical_fields() {
    assert_official_fixture_rows("pg18-gzip-copy-basic.dump");
}

#[test]
fn distinguishes_null_empty_and_escaped_null_spelling() {
    let mut rows = CopyRowReader::new(Cursor::new(b"\\N\t\t\\\\N\n\\.\n".as_slice()));

    let row = rows.next_row().unwrap().unwrap();
    assert_row(
        row,
        &[
            None,
            Some(b"".as_slice()),
            Some(b"\\N".as_slice()),
        ],
    );
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn decodes_standard_numeric_and_unknown_postgresql_escapes() {
    let input = b"\\b\\f\\n\\r\\t\\v\\\\\\101\\x42\\x4\\x\\q\n\\.\n";
    let mut rows = CopyRowReader::new(Cursor::new(input.as_slice()));

    let row = rows.next_row().unwrap().unwrap();
    assert_eq!(
        row.field(0),
        Some(FieldRef::Bytes(&[
            0x08, 0x0c, b'\n', b'\r', b'\t', 0x0b, b'\\', b'A', b'B', 0x04, b'x', b'q',
        ]))
    );
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn accepts_non_utf8_logical_field_bytes() {
    let mut rows = CopyRowReader::new(Cursor::new(b"\\377\\x80\n\\.\n".as_slice()));

    let row = rows.next_row().unwrap().unwrap();
    assert_eq!(row.field(0), Some(FieldRef::Bytes(&[0xff, 0x80])));
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn escaped_physical_delimiters_survive_one_byte_source_reads() {
    let input = b"left\\\tright\tline1\\\nline2\tbackslash\\\\value\n\\.\n";
    let mut rows = CopyRowReader::new(ShortRead::new(Cursor::new(input.as_slice()), 1));

    let row = rows.next_row().unwrap().unwrap();
    assert_row(
        row,
        &[
            Some(b"left\tright".as_slice()),
            Some(b"line1\nline2".as_slice()),
            Some(b"backslash\\value".as_slice()),
        ],
    );
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn standalone_terminator_stops_before_following_bytes() {
    let mut rows = CopyRowReader::new(Cursor::new(b"value\n\\.\nignored\n".as_slice()));

    let row = rows.next_row().unwrap().unwrap();
    assert_row(row, &[Some(b"value".as_slice())]);
    assert!(rows.next_row().unwrap().is_none());
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn malformed_or_truncated_escape_and_terminator_are_typed() {
    let mut escaped_eof = CopyRowReader::new(Cursor::new(b"value\\".as_slice()));
    assert!(matches!(
        escaped_eof.next_row().unwrap_err(),
        PgDumpError::MalformedCopyEscape { row: 1, .. }
    ));

    let mut truncated_terminator = CopyRowReader::new(Cursor::new(b"\\.".as_slice()));
    assert!(matches!(
        truncated_terminator.next_row().unwrap_err(),
        PgDumpError::MalformedCopyTerminator { row: 1, .. }
    ));

    let mut embedded_terminator =
        CopyRowReader::new(Cursor::new(b"prefix\\.\n".as_slice()));
    assert!(matches!(
        embedded_terminator.next_row().unwrap_err(),
        PgDumpError::MalformedCopyTerminator { row: 1, .. }
    ));
}

#[test]
fn reuses_borrowed_row_storage_across_advances() {
    let mut rows = CopyRowReader::new(Cursor::new(b"same\nsame\n\\.\n".as_slice()));

    let first_address = {
        let row = rows.next_row().unwrap().unwrap();
        match row.field(0).unwrap() {
            FieldRef::Null => panic!("expected bytes"),
            FieldRef::Bytes(bytes) => bytes.as_ptr() as usize,
        }
    };
    let second_address = {
        let row = rows.next_row().unwrap().unwrap();
        match row.field(0).unwrap() {
            FieldRef::Null => panic!("expected bytes"),
            FieldRef::Bytes(bytes) => bytes.as_ptr() as usize,
        }
    };

    assert_eq!(first_address, second_address);
    assert!(rows.next_row().unwrap().is_none());
}

fn assert_official_fixture_rows(name: &str) {
    let mut archive = Archive::open(Cursor::new(fixture(name))).unwrap();
    let data_id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let entry = archive.entry_reader(data_id).unwrap().unwrap();
    let mut rows = CopyRowReader::new(entry);

    let expected: &[&[Option<&[u8]>]] = &[
        &[
            Some(b"1"),
            Some(b"EARLY-100"),
            Some(b"customer-a"),
            Some(b"plain"),
            Some(b""),
        ],
        &[
            Some(b"2"),
            Some(b"SECOND-200"),
            Some(b"repeat"),
            Some(b"tab\tvalue"),
            Some(b"filled"),
        ],
        &[
            Some(b"3"),
            Some(b"THIRD-300"),
            Some(b"customer-c"),
            Some(b"line1\nline2"),
            Some(b"filled"),
        ],
        &[
            Some(b"4"),
            Some(b"MIDDLE-400"),
            Some(b"customer-d"),
            None,
            Some(b"filled"),
        ],
        &[
            Some(b"5"),
            Some(b"FIFTH-500"),
            Some(b"customer-e"),
            Some(b""),
            Some(b"filled"),
        ],
        &[
            Some(b"6"),
            Some(b"SIXTH-600"),
            Some(b"repeat"),
            Some(b"carriage\rreturn"),
            Some(b"filled"),
        ],
        &[
            Some(b"7"),
            Some(b"LATE-700"),
            Some(b"customer-g"),
            Some(b"backslash\\value"),
            Some(b"filled"),
        ],
    ];

    for expected_row in expected {
        let row = rows.next_row().unwrap().unwrap();
        assert_row(row, expected_row);
    }
    assert!(rows.next_row().unwrap().is_none());
}

fn assert_row(row: Row<'_>, expected: &[Option<&[u8]>]) {
    assert_eq!(row.len(), expected.len());
    assert!(!row.is_empty());

    let mut fields = row.fields();
    assert_eq!(fields.len(), expected.len());
    for (index, expected_field) in expected.iter().enumerate() {
        let actual = fields.next().unwrap();
        assert_eq!(row.field(index), Some(actual));
        match (actual, expected_field) {
            (FieldRef::Null, None) => {}
            (FieldRef::Bytes(actual), Some(expected)) => assert_eq!(actual, *expected),
            (actual, expected) => panic!("field {index} mismatch: {actual:?} != {expected:?}"),
        }
    }
    assert!(fields.next().is_none());
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
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
