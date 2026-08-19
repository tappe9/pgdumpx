use pgdumpx::{Archive, FieldRef, PgDumpError};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;

#[test]
fn official_none_and_gzip_fixtures_stream_identical_rows_through_table_rows() {
    let expected = expected_rows();

    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let mut archive = Archive::open(Cursor::new(fixture(fixture_name))).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();

        let column_names = rows
            .columns()
            .unwrap()
            .iter()
            .map(|column| column.name_bytes())
            .collect::<Vec<_>>();
        assert_eq!(
            column_names,
            [
                b"order_id".as_slice(),
                b"order_number".as_slice(),
                b"customer_code".as_slice(),
                b"note".as_slice(),
                b"empty_text".as_slice(),
            ]
        );
        assert_eq!(rows.column_index(b"order_number").unwrap(), Some(1));
        assert_eq!(rows.column_index(b"missing").unwrap(), None);

        assert_eq!(collect_rows(&mut rows), expected, "fixture {fixture_name}");
    }
}

#[test]
fn table_rows_is_independent_of_arbitrary_short_archive_reads() {
    let bytes = fixture("pg18-none-copy-basic.dump");
    let bytes_read = Rc::new(Cell::new(0_u64));
    let reader = ShortTrackingReader::new(bytes.clone(), Rc::clone(&bytes_read));
    let mut archive = Archive::open(reader).unwrap();
    let after_open = bytes_read.get();
    let mut rows = archive.table_rows(b"public", b"orders").unwrap();

    {
        let first = rows.next_row().unwrap().unwrap();
        assert_eq!(first.field(0), Some(FieldRef::Bytes(b"1")));
        assert_eq!(first.field(1), Some(FieldRef::Bytes(b"EARLY-100")));
    }

    assert!(bytes_read.get() > after_open);
    assert!(
        bytes_read.get() < u64::try_from(bytes.len()).unwrap(),
        "reading one row must not consume or buffer the complete table-data entry"
    );

    assert_eq!(collect_rows(&mut rows).len(), 6);
}

#[test]
fn positional_rows_remain_available_when_column_metadata_is_unavailable() {
    let payload = b"\\377\t\n\\.\n";
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(Some(b""), payload))).unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.columns().unwrap_err(),
        PgDumpError::CopyColumnMetadataUnavailable { dump_id: 2 }
    ));
    assert!(matches!(
        rows.column_index(b"value").unwrap_err(),
        PgDumpError::CopyColumnMetadataUnavailable { dump_id: 2 }
    ));

    let row = rows.next_row().unwrap().unwrap();
    assert_eq!(row.field(0), Some(FieldRef::Bytes(&[0xff])));
    assert_eq!(row.field(1), Some(FieldRef::Bytes(b"")));
}

#[test]
fn unsupported_representation_is_rejected_before_payload_access() {
    let bytes_read = Rc::new(Cell::new(0_u64));
    let reader = TrackingReader::new(
        archive_with_table_data(None, b"this is not COPY text"),
        Rc::clone(&bytes_read),
    );
    let mut archive = Archive::open(reader).unwrap();
    let after_open = bytes_read.get();

    let error = archive
        .table_rows(b"public", b"data")
        .err()
        .expect("unsupported representation must fail");
    assert!(matches!(
        error,
        PgDumpError::UnsupportedTableDataRepresentation { dump_id: 2, .. }
    ));
    assert_eq!(
        bytes_read.get(),
        after_open,
        "unsupported metadata must fail before validated seek or payload parsing"
    );
}

#[test]
fn missing_table_and_table_without_data_return_typed_errors() {
    let mut official = Archive::open(Cursor::new(fixture("pg18-none-copy-basic.dump"))).unwrap();
    assert!(matches!(
        official
            .table_rows(b"public", b"missing")
            .err()
            .expect("missing table must fail"),
        PgDumpError::TableNotFound
    ));

    let mut missing_data = Archive::open(Cursor::new(archive_without_table_data())).unwrap();
    assert!(matches!(
        missing_data
            .table_rows(b"public", b"data")
            .err()
            .expect("table without data must fail"),
        PgDumpError::TableDataEntryUnavailable { table_id: 1 }
    ));
}

#[test]
fn integrated_table_rows_enforces_provisional_field_limit() {
    let mut payload = Vec::new();
    payload.extend(std::iter::repeat_n(b'\t', 4_096));
    payload.extend_from_slice(b"\n\\.\n");
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(b"COPY public.data (value) FROM stdin;\n"),
        &payload,
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyFieldCountLimitExceeded {
            limit: 4_096,
            actual: 4_097,
            ..
        }
    ));
}

fn collect_rows<R: Read>(rows: &mut pgdumpx::TableRowReader<'_, R>) -> Vec<Vec<Option<Vec<u8>>>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next_row().unwrap() {
        result.push(
            row.fields()
                .map(|field| match field {
                    FieldRef::Null => None,
                    FieldRef::Bytes(bytes) => Some(bytes.to_vec()),
                })
                .collect(),
        );
    }
    result
}

fn expected_rows() -> Vec<Vec<Option<Vec<u8>>>> {
    [
        [Some(b"1".as_slice()), Some(b"EARLY-100"), Some(b"customer-a"), Some(b"plain"), Some(b"".as_slice())],
        [Some(b"2".as_slice()), Some(b"SECOND-200"), Some(b"repeat"), Some(b"tab\tvalue"), Some(b"filled")],
        [Some(b"3".as_slice()), Some(b"THIRD-300"), Some(b"customer-c"), Some(b"line1\nline2"), Some(b"filled")],
        [Some(b"4".as_slice()), Some(b"MIDDLE-400"), Some(b"customer-d"), None, Some(b"filled")],
        [Some(b"5".as_slice()), Some(b"FIFTH-500"), Some(b"customer-e"), Some(b"".as_slice()), Some(b"filled")],
        [Some(b"6".as_slice()), Some(b"SIXTH-600"), Some(b"repeat"), Some(b"carriage\rreturn"), Some(b"filled")],
        [Some(b"7".as_slice()), Some(b"LATE-700"), Some(b"customer-g"), Some(b"backslash\\value"), Some(b"filled")],
    ]
    .into_iter()
    .map(|row| {
        row.into_iter()
            .map(|field| field.map(<[u8]>::to_vec))
            .collect()
    })
    .collect()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}

fn archive_without_table_data() -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 1);
    write_table_entry(&mut bytes);
    bytes
}

fn archive_with_table_data(copy_statement: Option<&[u8]>, payload: &[u8]) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes, copy_statement);

    let offset = u64::try_from(bytes.len()).unwrap();
    let offset_start = table_data_offset_start(&bytes);
    bytes[offset_start..offset_start + 8].copy_from_slice(&offset.to_le_bytes());

    bytes.push(BLK_DATA);
    write_int(&mut bytes, 2);
    write_int(&mut bytes, i32::try_from(payload.len()).unwrap());
    bytes.extend_from_slice(payload);
    write_int(&mut bytes, 0);
    bytes
}

fn write_table_entry(bytes: &mut Vec<u8>) {
    write_int(bytes, 1);
    write_int(bytes, 0);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE"));
    write_int(bytes, 2);
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
    write_string(bytes, None);
    bytes.push(NO_DATA);
    bytes.extend_from_slice(&[0; 8]);
}

fn write_table_data_entry(bytes: &mut Vec<u8>, copy_statement: Option<&[u8]>) {
    write_int(bytes, 2);
    write_int(bytes, 1);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE DATA"));
    write_int(bytes, 3);
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
    bytes.push(POSITION_SET);
    bytes.extend_from_slice(&[0; 8]);
}

fn table_data_offset_start(bytes: &[u8]) -> usize {
    bytes.len() - 8
}

fn complete_header() -> Vec<u8> {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 16, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
    bytes
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

#[derive(Debug)]
struct ShortTrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<u64>>,
}

impl ShortTrackingReader {
    fn new(bytes: Vec<u8>, bytes_read: Rc<Cell<u64>>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read,
        }
    }
}

impl Read for ShortTrackingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let read = self.inner.read(&mut output[..1])?;
        self.bytes_read
            .set(self.bytes_read.get() + u64::try_from(read).unwrap());
        Ok(read)
    }
}

impl Seek for ShortTrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
