use pgdumpx::{Archive, PgDumpError, TableDataRepresentation};
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
fn official_fixture_exposes_ordered_columns_and_prepared_lookup() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let archive = Archive::open(Cursor::new(fixture(fixture_name))).unwrap();
        let table = archive.table(b"public", b"orders").unwrap();

        assert_eq!(
            table.data_representation().unwrap(),
            TableDataRepresentation::CopyText
        );
        let columns = table.columns().unwrap();
        let names = columns
            .iter()
            .map(|column| column.name_bytes())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                b"order_id".as_slice(),
                b"order_number".as_slice(),
                b"customer_code".as_slice(),
                b"note".as_slice(),
                b"empty_text".as_slice(),
            ]
        );
        assert_eq!(table.column_index(b"order_id").unwrap(), Some(0));
        assert_eq!(table.column_index(b"note").unwrap(), Some(3));
        assert_eq!(table.column_index(b"missing").unwrap(), None);

        let first = columns.as_ptr();
        let second = table.columns().unwrap().as_ptr();
        assert_eq!(
            first, second,
            "column metadata must be parsed and stored once"
        );
    }
}

#[test]
fn parses_pg_dump_quoted_unquoted_and_non_utf8_column_names_as_bytes() {
    let copy_statement =
        b"COPY public.data (plain, \"spaced name\", \"quote\"\"name\", \"\xff\") FROM stdin;\n";
    let archive =
        Archive::open(Cursor::new(archive_with_table_data(Some(copy_statement)))).unwrap();
    let table = archive.table(b"public", b"data").unwrap();

    let columns = table.columns().unwrap();
    let names = columns
        .iter()
        .map(|column| column.name_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            b"plain".as_slice(),
            b"spaced name".as_slice(),
            b"quote\"name".as_slice(),
            [0xff].as_slice(),
        ]
    );
    assert_eq!(table.column_index(b"quote\"name").unwrap(), Some(2));
    assert_eq!(table.column_index(&[0xff]).unwrap(), Some(3));
    assert!(columns[3].name_str().is_err());
}

#[test]
fn accepts_pg_dump_zero_column_copy_shape() {
    let archive = Archive::open(Cursor::new(archive_with_table_data(Some(
        b"COPY public.data  FROM stdin;\n",
    ))))
    .unwrap();
    let table = archive.table(b"public", b"data").unwrap();

    assert_eq!(
        table.data_representation().unwrap(),
        TableDataRepresentation::CopyText
    );
    assert!(table.columns().unwrap().is_empty());
    assert_eq!(table.column_index(b"anything").unwrap(), None);
}

#[test]
fn distinguishes_unavailable_malformed_and_missing_table_data_metadata() {
    let unavailable = Archive::open(Cursor::new(archive_with_table_data(Some(b"")))).unwrap();
    let table = unavailable.table(b"public", b"data").unwrap();
    assert!(matches!(
        table.columns().unwrap_err(),
        PgDumpError::CopyColumnMetadataUnavailable { dump_id: 2 }
    ));

    let malformed = Archive::open(Cursor::new(archive_with_table_data(Some(
        b"COPY public.data (plain, \"unterminated) FROM stdin;\n",
    ))))
    .unwrap();
    let table = malformed.table(b"public", b"data").unwrap();
    assert!(matches!(
        table.column_index(b"plain").unwrap_err(),
        PgDumpError::MalformedCopyStatement { dump_id: 2, .. }
    ));

    let missing = Archive::open(Cursor::new(archive_without_table_data())).unwrap();
    let table = missing.table(b"public", b"data").unwrap();
    assert!(matches!(
        table.columns().unwrap_err(),
        PgDumpError::TableDataEntryUnavailable { table_id: 1 }
    ));
}

#[test]
fn rejects_insert_and_binary_representations_before_payload_access() {
    let cases = [
        (None, TableDataRepresentation::Insert),
        (
            Some(b"COPY public.data (value) FROM stdin WITH (FORMAT binary);\n".as_slice()),
            TableDataRepresentation::Binary,
        ),
    ];

    for (copy_statement, representation) in cases {
        let bytes_read = Rc::new(Cell::new(0_u64));
        let reader = TrackingReader::new(
            archive_with_table_data(copy_statement),
            Rc::clone(&bytes_read),
        );
        let archive = Archive::open(reader).unwrap();
        let after_open = bytes_read.get();
        let table = archive.table(b"public", b"data").unwrap();

        assert!(matches!(
            table.columns().unwrap_err(),
            PgDumpError::UnsupportedTableDataRepresentation {
                dump_id: 2,
                representation: actual,
            } if actual == representation
        ));
        assert_eq!(
            bytes_read.get(),
            after_open,
            "representation validation must not seek to or parse payload bytes"
        );
    }
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

fn archive_with_table_data(copy_statement: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);

    write_int(&mut bytes, 2);
    write_int(&mut bytes, 1);
    write_string(&mut bytes, Some(b"1259"));
    write_string(&mut bytes, Some(b"16385"));
    write_string(&mut bytes, Some(b"data"));
    write_string(&mut bytes, Some(b"TABLE DATA"));
    write_int(&mut bytes, 3);
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_string(&mut bytes, copy_statement);
    write_string(&mut bytes, Some(b"public"));
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_int(&mut bytes, 0);
    write_string(&mut bytes, Some(b"postgres"));
    write_string(&mut bytes, Some(b"false"));
    write_string(&mut bytes, Some(b"1"));
    write_string(&mut bytes, None);
    bytes.push(POSITION_SET);
    let offset_start = bytes.len();
    bytes.extend_from_slice(&[0; 8]);

    let data_offset = u64::try_from(bytes.len()).unwrap();
    bytes[offset_start..offset_start + 8].copy_from_slice(&data_offset.to_le_bytes());
    bytes.extend_from_slice(&data_block());
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
    bytes.push(NO_DATA);
    bytes.extend_from_slice(&[0; 8]);
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

fn data_block() -> Vec<u8> {
    let mut bytes = vec![BLK_DATA];
    write_int(&mut bytes, 2);
    write_int(&mut bytes, 3);
    bytes.extend_from_slice(b"\\.\n");
    write_int(&mut bytes, 0);
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
        let read = u64::try_from(read).unwrap();
        self.bytes_read.set(self.bytes_read.get() + read);
        Ok(usize::try_from(read).unwrap())
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
