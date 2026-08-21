use pgdumpx::{Archive, CopyRowReader, Limits, PgDumpError};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

const BLK_DATA: u8 = 1;
const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;

#[test]
fn default_limits_are_finite_compatible_and_shared_by_open_paths() {
    let limits = Limits::default();

    assert_eq!(limits.max_toc_entries(), 100_000);
    assert_eq!(limits.max_string_bytes(), 16 * 1024 * 1024);
    assert_eq!(limits.max_dependencies_per_entry(), 100_000);
    assert_eq!(limits.max_row_bytes(), 16 * 1024 * 1024);
    assert_eq!(limits.max_fields_per_row(), 4 * 1024);
    assert_eq!(Limits::default_compatible(), limits);

    let bytes = fixture("pg18-none-copy-basic.dump");
    let ordinary = Archive::open(Cursor::new(bytes.clone())).unwrap();
    let configured = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();

    assert_eq!(ordinary.header(), configured.header());
    assert_eq!(ordinary.entries(), configured.entries());
}

#[test]
fn toc_entry_limit_accepts_below_and_exact_and_rejects_above_before_entries() {
    let limits = Limits::default().with_max_toc_entries(1);

    for count in [0_i32, 1] {
        let bytes = metadata_archive(count, |output| {
            if count == 1 {
                write_metadata_entry(output, 1, b"entry", &[]);
            }
        });
        let archive = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
        assert_eq!(archive.entries().len(), usize::try_from(count).unwrap());
    }

    let mut bytes = complete_header(b"database", b"18.4", b"18.4");
    write_int(&mut bytes, 2);
    bytes.extend_from_slice(b"entry-bytes-must-not-be-read");
    let expected_read =
        u64::try_from(bytes.len() - b"entry-bytes-must-not-be-read".len()).unwrap();
    let bytes_read = Rc::new(Cell::new(0));
    let error = Archive::open_with_limits(
        TrackingReader::new(bytes, Rc::clone(&bytes_read)),
        limits,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::TocEntryLimitExceeded {
            count: 2,
            limit: 1,
            ..
        }
    ));
    assert_eq!(bytes_read.get(), expected_read);
}

#[test]
fn archive_string_limit_accepts_below_and_exact_and_rejects_above_before_payload() {
    let limits = Limits::default().with_max_string_bytes(4);

    for database_name in [b"abc".as_slice(), b"abcd".as_slice()] {
        let bytes = metadata_archive_with_header(database_name, 0, |_| {});
        Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
    }

    let mut bytes = fixed_header();
    write_timestamp(&mut bytes);
    write_int(&mut bytes, 5);
    bytes.extend_from_slice(b"payload-must-not-be-read");
    let bytes_read = Rc::new(Cell::new(0));
    let error = Archive::open_with_limits(
        TrackingReader::new(bytes, Rc::clone(&bytes_read)),
        limits,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ArchiveStringLimitExceeded {
            length: 5,
            limit: 4,
            ..
        }
    ));
    assert_eq!(bytes_read.get(), 52);
}

#[test]
fn dependency_limit_accepts_below_and_exact_and_rejects_above() {
    let limits = Limits::default().with_max_dependencies_per_entry(1);

    for dependencies in [&[][..], &[2][..]] {
        let bytes = metadata_archive(1, |output| {
            write_metadata_entry(output, 1, b"entry", dependencies);
        });
        let archive = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
        assert_eq!(archive.entries()[0].dependencies().len(), dependencies.len());
    }

    let bytes = metadata_archive(1, |output| {
        write_metadata_entry(output, 1, b"entry", &[2, 3]);
    });
    let error = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::DependencyLimitExceeded {
            entry_id: 1,
            count: 2,
            limit: 1,
            ..
        }
    ));
}

#[test]
fn row_byte_limit_has_identical_standalone_and_integrated_boundaries() {
    let limits = Limits::default().with_max_row_bytes(4);

    for input in [b"abc\n\\.\n".as_slice(), b"abcd\n\\.\n".as_slice()] {
        assert_first_row_succeeds_standalone(input, limits);
        assert_first_row_succeeds_integrated(input, limits);
    }

    let input = b"abcde\n\\.\n";
    assert!(matches!(
        standalone_error(input, limits),
        PgDumpError::CopyRowByteLimitExceeded {
            row: 1,
            limit: 4,
            actual: 5,
            ..
        }
    ));
    assert!(matches!(
        integrated_error(input, limits),
        PgDumpError::CopyRowByteLimitExceeded {
            row: 1,
            limit: 4,
            actual: 5,
            ..
        }
    ));
}

#[test]
fn field_count_limit_has_identical_standalone_and_integrated_boundaries() {
    let limits = Limits::default()
        .with_max_row_bytes(64)
        .with_max_fields_per_row(3);

    for (input, expected_fields) in [
        (b"a\tb\n\\.\n".as_slice(), 2),
        (b"a\tb\tc\n\\.\n".as_slice(), 3),
    ] {
        assert_eq!(standalone_field_count(input, limits), expected_fields);
        assert_eq!(integrated_field_count(input, limits), expected_fields);
    }

    let input = b"a\tb\tc\td\n\\.\n";
    assert!(matches!(
        standalone_error(input, limits),
        PgDumpError::CopyFieldCountLimitExceeded {
            row: 1,
            limit: 3,
            actual: 4,
            ..
        }
    ));
    assert!(matches!(
        integrated_error(input, limits),
        PgDumpError::CopyFieldCountLimitExceeded {
            row: 1,
            limit: 3,
            actual: 4,
            ..
        }
    ));
}

#[test]
fn fields_per_row_limit_also_bounds_copy_column_metadata_before_index_growth() {
    let limits = Limits::default().with_max_fields_per_row(3);

    for columns in [
        &[b"a".as_slice(), b"b".as_slice()][..],
        &[b"a".as_slice(), b"b".as_slice(), b"c".as_slice()][..],
    ] {
        let bytes = archive_with_copy_columns(columns);
        Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
    }

    let bytes = archive_with_copy_columns(&[b"a", b"b", b"c", b"d"]);
    let error = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::CopyColumnCountLimitExceeded {
            dump_id: 2,
            limit: 3,
            actual: 4,
        }
    ));
}

#[test]
fn official_none_and_gzip_table_rows_use_configured_finite_limits() {
    let first_physical_row = b"1\tEARLY-100\tcustomer-a\tplain\t\n";
    let exact_row_bytes = first_physical_row.len() - 1;

    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        for row_limit in [exact_row_bytes + 1, exact_row_bytes] {
            let limits = Limits::default().with_max_row_bytes(row_limit);
            let mut archive =
                Archive::open_with_limits(Cursor::new(fixture(fixture_name)), limits).unwrap();
            let mut rows = archive.table_rows(b"public", b"orders").unwrap();
            assert!(rows.next_row().unwrap().is_some());
        }

        let limits = Limits::default().with_max_row_bytes(exact_row_bytes - 1);
        let mut archive =
            Archive::open_with_limits(Cursor::new(fixture(fixture_name)), limits).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        assert!(matches!(
            rows.next_row().unwrap_err(),
            PgDumpError::CopyRowByteLimitExceeded {
                row: 1,
                limit,
                actual,
                ..
            } if limit == u64::try_from(exact_row_bytes - 1).unwrap()
                && actual == u64::try_from(exact_row_bytes).unwrap()
        ));
    }
}

fn assert_first_row_succeeds_standalone(input: &[u8], limits: Limits) {
    let mut rows = CopyRowReader::with_limits(Cursor::new(input), limits);
    assert!(rows.next_row().unwrap().is_some());
}

fn assert_first_row_succeeds_integrated(input: &[u8], limits: Limits) {
    let bytes = archive_with_table_data(Some(b""), input);
    let mut archive = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();
    assert!(rows.next_row().unwrap().is_some());
}

fn standalone_field_count(input: &[u8], limits: Limits) -> usize {
    let mut rows = CopyRowReader::with_limits(Cursor::new(input), limits);
    rows.next_row().unwrap().unwrap().len()
}

fn integrated_field_count(input: &[u8], limits: Limits) -> usize {
    let bytes = archive_with_table_data(Some(b""), input);
    let mut archive = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();
    rows.next_row().unwrap().unwrap().len()
}

fn standalone_error(input: &[u8], limits: Limits) -> PgDumpError {
    let mut rows = CopyRowReader::with_limits(Cursor::new(input), limits);
    rows.next_row().unwrap_err()
}

fn integrated_error(input: &[u8], limits: Limits) -> PgDumpError {
    let bytes = archive_with_table_data(Some(b""), input);
    let mut archive = Archive::open_with_limits(Cursor::new(bytes), limits).unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();
    rows.next_row().unwrap_err()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}

fn metadata_archive(entry_count: i32, write_entries: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    metadata_archive_with_header(b"database", entry_count, write_entries)
}

fn metadata_archive_with_header(
    database_name: &[u8],
    entry_count: i32,
    write_entries: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut output = complete_header(database_name, b"x", b"x");
    write_int(&mut output, entry_count);
    write_entries(&mut output);
    output
}

fn archive_with_copy_columns(columns: &[&[u8]]) -> Vec<u8> {
    let mut statement = b"COPY public.data (".to_vec();
    for (index, column) in columns.iter().enumerate() {
        if index != 0 {
            statement.extend_from_slice(b", ");
        }
        statement.extend_from_slice(column);
    }
    statement.extend_from_slice(b") FROM stdin;\n");

    let mut bytes = complete_header(b"database", b"18.4", b"18.4");
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes, Some(&statement), false);
    bytes
}

fn archive_with_table_data(copy_statement: Option<&[u8]>, payload: &[u8]) -> Vec<u8> {
    let mut bytes = complete_header(b"database", b"18.4", b"18.4");
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes, copy_statement, true);

    let offset = u64::try_from(bytes.len()).unwrap();
    let offset_start = bytes.len() - 8;
    bytes[offset_start..].copy_from_slice(&offset.to_le_bytes());

    bytes.push(BLK_DATA);
    write_int(&mut bytes, 2);
    write_int(&mut bytes, i32::try_from(payload.len()).unwrap());
    bytes.extend_from_slice(payload);
    write_int(&mut bytes, 0);
    bytes
}

fn write_metadata_entry(output: &mut Vec<u8>, id: i32, tag: &[u8], dependencies: &[i32]) {
    write_int(output, id);
    write_int(output, 0);
    write_string(output, Some(b"0"));
    write_string(output, Some(b"0"));
    write_string(output, Some(tag));
    write_string(output, Some(b"COMMENT"));
    write_int(output, SECTION_PRE_DATA);
    for _ in 0..6 {
        write_string(output, None);
    }
    write_int(output, 0);
    write_string(output, None);
    write_string(output, Some(b"false"));
    write_dependencies(output, dependencies);
    output.push(NO_DATA);
    output.extend_from_slice(&[0; 8]);
}

fn write_table_entry(bytes: &mut Vec<u8>) {
    write_int(bytes, 1);
    write_int(bytes, 0);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE"));
    write_int(bytes, SECTION_PRE_DATA);
    write_string(bytes, Some(b"CREATE TABLE public.data (value text);\n"));
    write_string(bytes, Some(b"DROP TABLE public.data;\n"));
    write_string(bytes, None);
    write_string(bytes, Some(b"public"));
    write_string(bytes, None);
    write_string(bytes, Some(b"heap"));
    write_int(bytes, 0);
    write_string(bytes, Some(b"postgres"));
    write_string(bytes, Some(b"false"));
    write_string(bytes, None);
    bytes.push(NO_DATA);
    bytes.extend_from_slice(&[0; 8]);
}

fn write_table_data_entry(
    bytes: &mut Vec<u8>,
    copy_statement: Option<&[u8]>,
    position_set: bool,
) {
    write_int(bytes, 2);
    write_int(bytes, 1);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE DATA"));
    write_int(bytes, SECTION_DATA);
    write_string(bytes, None);
    write_string(bytes, None);
    write_string(bytes, copy_statement);
    write_string(bytes, Some(b"public"));
    write_string(bytes, None);
    write_string(bytes, None);
    write_int(bytes, 0);
    write_string(bytes, Some(b"postgres"));
    write_string(bytes, Some(b"false"));
    write_string(bytes, Some(b"1"));
    write_string(bytes, None);
    bytes.push(if position_set { POSITION_SET } else { NO_DATA });
    bytes.extend_from_slice(&[0; 8]);
}

fn write_dependencies(output: &mut Vec<u8>, dependencies: &[i32]) {
    for dependency in dependencies {
        let dependency = dependency.to_string();
        write_string(output, Some(dependency.as_bytes()));
    }
    write_string(output, None);
}

fn complete_header(
    database_name: &[u8],
    server_version: &[u8],
    dump_version: &[u8],
) -> Vec<u8> {
    let mut output = fixed_header();
    write_timestamp(&mut output);
    write_string(&mut output, Some(database_name));
    write_string(&mut output, Some(server_version));
    write_string(&mut output, Some(dump_version));
    output
}

fn fixed_header() -> Vec<u8> {
    let mut output = b"PGDMP".to_vec();
    output.extend_from_slice(&[1, 16, 0]);
    output.push(4);
    output.push(8);
    output.push(1);
    output.push(0);
    output
}

fn write_timestamp(output: &mut Vec<u8>) {
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(output, value);
    }
}

fn write_int(output: &mut Vec<u8>, value: i32) {
    output.push(u8::from(value.is_negative()));
    output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
}

fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_int(output, i32::try_from(bytes.len()).unwrap());
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}

#[derive(Debug)]
struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<u64>>,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>, bytes_read: Rc<Cell<u64>>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read,
        }
    }
}

impl Read for TrackingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(output)?;
        self.bytes_read
            .set(self.bytes_read.get() + u64::try_from(read).unwrap());
        Ok(read)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
