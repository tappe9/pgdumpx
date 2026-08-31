use pgdumpx::{Archive, CopyRowReader, FieldRef, PgDumpError};
use std::io::Cursor;

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const THREE_COLUMNS: &[u8] = b"COPY public.data (first, second, third) FROM stdin;\n";

#[test]
fn exact_metadata_field_count_preserves_nulls_and_escaped_delimiters() {
    let payload = b"a\\tb\t\\N\tc\n\\.\n";
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(THREE_COLUMNS),
        payload,
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    let row = rows.next_row().unwrap().unwrap();
    assert_eq!(row.len(), 3);
    assert_eq!(row.field(0), Some(FieldRef::Bytes(b"a\tb")));
    assert_eq!(row.field(1), Some(FieldRef::Null));
    assert_eq!(row.field(2), Some(FieldRef::Bytes(b"c")));
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn short_archive_backed_row_is_rejected_before_exposure() {
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(THREE_COLUMNS),
        b"a\tb\nnext\trow\n\\.\n",
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyRowFieldCountMismatch {
            dump_id: 2,
            row: 1,
            expected: 3,
            actual: 2,
        }
    ));
    assert!(rows.next_row().unwrap().is_none());
}

#[test]
fn long_archive_backed_row_is_rejected_before_exposure() {
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(THREE_COLUMNS),
        b"a\tb\tc\td\n\\.\n",
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyRowFieldCountMismatch {
            dump_id: 2,
            row: 1,
            expected: 3,
            actual: 4,
        }
    ));
}

#[test]
fn named_equality_search_reports_short_row_corruption_instead_of_no_match() {
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(THREE_COLUMNS),
        b"a\tb\n\\.\n",
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.find_first_equal(b"third", FieldRef::Bytes(b"c"))
            .unwrap_err(),
        PgDumpError::CopyRowFieldCountMismatch {
            dump_id: 2,
            row: 1,
            expected: 3,
            actual: 2,
        }
    ));
}

#[test]
fn positional_iteration_remains_available_without_valid_column_metadata() {
    let mut archive = Archive::open(Cursor::new(archive_with_table_data(
        Some(b""),
        b"a\tb\n\\.\n",
    )))
    .unwrap();
    let mut rows = archive.table_rows(b"public", b"data").unwrap();

    assert!(matches!(
        rows.columns().unwrap_err(),
        PgDumpError::CopyColumnMetadataUnavailable { dump_id: 2 }
    ));
    let row = rows.next_row().unwrap().unwrap();
    assert_eq!(row.len(), 2);
    assert_eq!(row.field(0), Some(FieldRef::Bytes(b"a")));
    assert_eq!(row.field(1), Some(FieldRef::Bytes(b"b")));
}

#[test]
fn standalone_copy_reader_remains_schema_agnostic() {
    let mut rows = CopyRowReader::new(Cursor::new(b"one\ntwo\tfields\n\\.\n".as_slice()));

    assert_eq!(rows.next_row().unwrap().unwrap().len(), 1);
    assert_eq!(rows.next_row().unwrap().unwrap().len(), 2);
    assert!(rows.next_row().unwrap().is_none());
}

fn archive_with_table_data(copy_statement: Option<&[u8]>, payload: &[u8]) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes, copy_statement);

    let offset = u64::try_from(bytes.len()).unwrap();
    let offset_start = bytes.len() - 8;
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
    write_string(
        bytes,
        Some(b"CREATE TABLE public.data (first text, second text, third text);\n"),
    );
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
