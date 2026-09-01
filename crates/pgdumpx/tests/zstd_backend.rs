use pgdumpx::{Archive, Compression, PgDumpError};
use std::{io::Cursor, path::PathBuf};

#[cfg(feature = "zstd")]
use pgdumpx::{DataLocation, EntryReadLimits, FieldRef, OwnedField};
#[cfg(feature = "zstd")]
use std::{
    cell::Cell,
    io::{self, Read, Seek, SeekFrom},
};

#[cfg(feature = "zstd")]
const POSITION_SET: u8 = 2;
#[cfg(feature = "zstd")]
const BLK_DATA: u8 = 1;
#[cfg(feature = "zstd")]
const ZSTD_COMPRESSION: u8 = 3;
#[cfg(feature = "zstd")]
const EXPECTED_COPY_STREAM: &[u8] = b"1\tEARLY-100\tcustomer-a\tplain\t\n\
2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n\
3\tTHIRD-300\tcustomer-c\tline1\\nline2\tfilled\n\
4\tMIDDLE-400\tcustomer-d\t\\N\tfilled\n\
5\tFIFTH-500\tcustomer-e\t\tfilled\n\
6\tSIXTH-600\trepeat\tcarriage\\rreturn\tfilled\n\
7\tLATE-700\tcustomer-g\tbackslash\\\\value\tfilled\n\
\\.\n\n\n";

#[cfg(feature = "zstd")]
#[test]
fn official_zstd_fixture_streams_rows_and_stops_find_first_early() {
    let bytes = fixture();
    let mut archive = Archive::open(Cursor::new(bytes.clone())).unwrap();
    assert!(matches!(archive.header().compression(), Compression::Zstd));
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut entry = archive.entry_reader(id).unwrap().unwrap();
    let mut output = Vec::new();
    entry.read_to_end(&mut output).unwrap();
    assert_eq!(output, EXPECTED_COPY_STREAM);

    let mut archive = Archive::open(Cursor::new(bytes.clone())).unwrap();
    let mut rows = archive.table_rows(b"public", b"orders").unwrap();
    let mut row_count = 0_u64;
    while rows.next_row().unwrap().is_some() {
        row_count += 1;
    }
    assert_eq!(row_count, 7);

    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let mut rows = archive.table_rows(b"public", b"orders").unwrap();
    let order_number = rows.column_index(b"order_number").unwrap().unwrap();
    let predicate_calls = Cell::new(0_u64);
    let found = rows
        .find_first(|row| {
            predicate_calls.set(predicate_calls.get() + 1);
            row.field(order_number) == Some(FieldRef::Bytes(b"MIDDLE-400"))
        })
        .unwrap()
        .unwrap();
    assert_eq!(found.field(0), Some(&OwnedField::Bytes(b"4".to_vec())));
    assert_eq!(predicate_calls.get(), 4);
}

#[cfg(feature = "zstd")]
#[test]
fn zstd_handles_one_byte_source_reads_and_one_byte_custom_chunks() {
    let fixture = fixture();
    let reader = OneByteReader::new(fixture.clone());
    let mut archive = Archive::open(reader).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut entry = archive.entry_reader(id).unwrap().unwrap();
    let mut output = Vec::new();
    entry.read_to_end(&mut output).unwrap();
    assert_eq!(output, EXPECTED_COPY_STREAM);

    let compressed = compressed_entry_bytes(&fixture);
    let chunks: Vec<&[u8]> = compressed.chunks(1).collect();
    let block = data_block(BLK_DATA, 1, &chunks);
    let bytes = archive_with_block(ZSTD_COMPRESSION, &block);
    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive.entries()[0].id();
    let mut entry = archive.entry_reader(id).unwrap().unwrap();
    let mut output = Vec::new();
    entry.read_to_end(&mut output).unwrap();
    assert_eq!(output, EXPECTED_COPY_STREAM);
}

#[cfg(feature = "zstd")]
#[test]
fn corrupt_and_truncated_zstd_are_typed_decompression_errors() {
    let compressed = compressed_entry_bytes(&fixture());
    let mut corrupt = compressed.clone();
    corrupt[0] ^= 0xff;
    let truncated = compressed[..compressed.len() - 4].to_vec();

    for encoded in [corrupt, truncated] {
        let block = data_block(BLK_DATA, 1, &[encoded.as_slice()]);
        let bytes = archive_with_block(ZSTD_COMPRESSION, &block);
        let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
        let id = archive.entries()[0].id();
        let mut entry = archive.entry_reader(id).unwrap().unwrap();
        let error = entry.read_to_end(&mut Vec::new()).unwrap_err();
        assert!(matches!(
            pgdump_error(&error),
            PgDumpError::DecompressionFailed {
                dump_id: 1,
                algorithm: "zstandard",
                ..
            }
        ));
    }
}

#[cfg(feature = "zstd")]
#[test]
fn zstd_raw_limits_distinguish_exact_eof_from_limit_exceeded() {
    let expected = u64::try_from(EXPECTED_COPY_STREAM.len()).unwrap();
    let bytes = fixture();

    let mut archive = Archive::open(Cursor::new(bytes.clone())).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut exact = archive
        .entry_reader_with_limits(
            id,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(expected),
        )
        .unwrap()
        .unwrap();
    let mut output = Vec::new();
    exact.read_to_end(&mut output).unwrap();
    assert_eq!(output, EXPECTED_COPY_STREAM);

    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut limited = archive
        .entry_reader_with_limits(
            id,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(expected - 1),
        )
        .unwrap()
        .unwrap();
    let mut partial = Vec::new();
    let error = limited.read_to_end(&mut partial).unwrap_err();
    assert_eq!(partial.len(), EXPECTED_COPY_STREAM.len() - 1);
    assert!(matches!(
        pgdump_error(&error),
        PgDumpError::EntryDecompressedByteLimitExceeded { .. }
    ));
}

#[cfg(not(feature = "zstd"))]
#[test]
fn zstd_metadata_opens_but_selected_read_reports_backend_unavailable() {
    let mut archive = Archive::open(Cursor::new(fixture())).unwrap();
    assert!(matches!(archive.header().compression(), Compression::Zstd));
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let error = archive.entry_reader(id).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::UnsupportedEntryCompression {
            algorithm: "zstandard",
            ..
        }
    ));
}

#[cfg(feature = "zstd")]
fn pgdump_error(error: &io::Error) -> &PgDumpError {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<PgDumpError>())
        .expect("entry Read errors must preserve a typed PgDumpError source")
}

fn fixture() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives/pg18-zstd-copy-basic.dump");
    std::fs::read(path).expect("official Zstandard fixture must be readable")
}

#[cfg(feature = "zstd")]
fn compressed_entry_bytes(bytes: &[u8]) -> Vec<u8> {
    let archive = Archive::open(Cursor::new(bytes.to_vec())).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let entry = archive.entry(id).unwrap();
    let offset = match entry.data_location() {
        DataLocation::Offset(offset) => offset,
        other => panic!("official fixture must have an offset, got {other:?}"),
    };
    drop(archive);

    let mut cursor = usize::try_from(offset).unwrap();
    assert_eq!(bytes[cursor], BLK_DATA);
    cursor += 1;
    let (dump_id, next) = read_archive_int(bytes, cursor);
    assert_eq!(dump_id, id.as_i32());
    cursor = next;

    let mut compressed = Vec::new();
    loop {
        let (length, next) = read_archive_int(bytes, cursor);
        cursor = next;
        if length == 0 {
            break;
        }
        assert!(length > 0);
        let length = usize::try_from(length).unwrap();
        compressed.extend_from_slice(&bytes[cursor..cursor + length]);
        cursor += length;
    }
    compressed
}

#[cfg(feature = "zstd")]
fn read_archive_int(bytes: &[u8], offset: usize) -> (i32, usize) {
    let negative = bytes[offset] != 0;
    let magnitude = u32::from_le_bytes(bytes[offset + 1..offset + 5].try_into().unwrap());
    let value = i32::try_from(magnitude).unwrap();
    (if negative { -value } else { value }, offset + 5)
}

#[cfg(feature = "zstd")]
fn archive_with_block(compression: u8, block: &[u8]) -> Vec<u8> {
    let mut bytes = complete_header(compression);
    write_int(&mut bytes, 1);
    write_int(&mut bytes, 1);
    write_int(&mut bytes, 1);
    write_string(&mut bytes, Some(b"0"));
    write_string(&mut bytes, Some(b"1"));
    write_string(&mut bytes, Some(b"data"));
    write_string(&mut bytes, Some(b"TABLE DATA"));
    write_int(&mut bytes, 3);
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_string(&mut bytes, Some(b"COPY public.data (value) FROM stdin;\n"));
    write_string(&mut bytes, Some(b"public"));
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_int(&mut bytes, 0);
    write_string(&mut bytes, Some(b"postgres"));
    write_string(&mut bytes, Some(b"false"));
    write_string(&mut bytes, None);
    bytes.push(POSITION_SET);
    let offset_start = bytes.len();
    bytes.extend_from_slice(&[0; 8]);
    let data_offset = u64::try_from(bytes.len()).unwrap();
    bytes[offset_start..offset_start + 8].copy_from_slice(&data_offset.to_le_bytes());
    bytes.extend_from_slice(block);
    bytes
}

#[cfg(feature = "zstd")]
fn complete_header(compression: u8) -> Vec<u8> {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 16, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(compression);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
    bytes
}

#[cfg(feature = "zstd")]
fn data_block(marker: u8, dump_id: i32, chunks: &[&[u8]]) -> Vec<u8> {
    let mut block = vec![marker];
    write_int(&mut block, dump_id);
    for chunk in chunks {
        write_int(&mut block, i32::try_from(chunk.len()).unwrap());
        block.extend_from_slice(chunk);
    }
    write_int(&mut block, 0);
    block
}

#[cfg(feature = "zstd")]
fn write_int(output: &mut Vec<u8>, value: i32) {
    output.push(u8::from(value.is_negative()));
    output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
}

#[cfg(feature = "zstd")]
fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_int(output, i32::try_from(bytes.len()).unwrap());
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}

#[cfg(feature = "zstd")]
#[derive(Debug)]
struct OneByteReader {
    inner: Cursor<Vec<u8>>,
}

#[cfg(feature = "zstd")]
impl OneByteReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            inner: Cursor::new(bytes),
        }
    }
}

#[cfg(feature = "zstd")]
impl Read for OneByteReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.inner.read(&mut output[..1])
    }
}

#[cfg(feature = "zstd")]
impl Seek for OneByteReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
