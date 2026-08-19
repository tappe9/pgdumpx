use pgdumpx::{Archive, PgDumpError};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

const POSITION_NOT_SET: u8 = 1;
const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const EXPECTED_COPY_STREAM: &[u8] = b"1\tEARLY-100\tcustomer-a\tplain\t\n\
2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n\
3\tTHIRD-300\tcustomer-c\tline1\\nline2\tfilled\n\
4\tMIDDLE-400\tcustomer-d\t\\N\tfilled\n\
5\tFIFTH-500\tcustomer-e\t\tfilled\n\
6\tSIXTH-600\trepeat\tcarriage\\rreturn\tfilled\n\
7\tLATE-700\tcustomer-g\tbackslash\\\\value\tfilled\n\
\\.\n\n\n";

#[test]
fn streams_official_none_fixture_through_the_public_path() {
    assert_official_fixture_streams("pg18-none-copy-basic.dump");
}

#[test]
fn streams_official_gzip_fixture_through_the_public_path() {
    assert_official_fixture_streams("pg18-gzip-copy-basic.dump");
}

#[test]
fn validates_block_type_and_dump_id_before_exposing_data() {
    let wrong_type = archive_with_block(0, POSITION_SET, None, &data_block(3, 1, &[b"data"]));
    let mut archive = Archive::open(Cursor::new(wrong_type)).unwrap();
    let id = archive.entries()[0].id();
    let error = archive.entry_reader(id).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::UnexpectedDataBlockType {
            dump_id: 1,
            expected: BLK_DATA,
            actual: 3,
            ..
        }
    ));

    let wrong_id = archive_with_block(0, POSITION_SET, None, &data_block(BLK_DATA, 2, &[b"data"]));
    let mut archive = Archive::open(Cursor::new(wrong_id)).unwrap();
    let id = archive.entries()[0].id();
    let error = archive.entry_reader(id).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::DataBlockDumpIdMismatch {
            expected: 1,
            actual: 2,
            ..
        }
    ));
}

#[test]
fn rejects_unusable_or_overflowing_entry_offsets() {
    let unknown = archive_with_block(0, POSITION_NOT_SET, None, &[]);
    let mut archive = Archive::open(Cursor::new(unknown)).unwrap();
    let id = archive.entries()[0].id();
    assert!(matches!(
        archive.entry_reader(id).unwrap_err(),
        PgDumpError::EntryDataOffsetUnavailable { dump_id: 1 }
    ));

    let no_data = archive_with_block(0, NO_DATA, None, &[]);
    let mut archive = Archive::open(Cursor::new(no_data)).unwrap();
    let id = archive.entries()[0].id();
    assert!(matches!(
        archive.entry_reader(id).unwrap_err(),
        PgDumpError::EntryHasNoData { dump_id: 1 }
    ));

    let overflowing = archive_with_block(0, POSITION_SET, Some(u64::MAX), &[]);
    let mut archive = Archive::open(Cursor::new(overflowing)).unwrap();
    let id = archive.entries()[0].id();
    assert!(matches!(
        archive.entry_reader(id).unwrap_err(),
        PgDumpError::InvalidDataOffset {
            dump_id: 1,
            offset: u64::MAX,
        }
    ));
}

#[test]
fn streams_zero_one_and_multiple_custom_chunks() {
    for (chunks, expected) in [
        (Vec::<&[u8]>::new(), b"".as_slice()),
        (vec![b"one".as_slice()], b"one".as_slice()),
        (
            vec![b"one".as_slice(), b"-two".as_slice(), b"-three".as_slice()],
            b"one-two-three".as_slice(),
        ),
    ] {
        let bytes = archive_with_block(0, POSITION_SET, None, &data_block(BLK_DATA, 1, &chunks));
        let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
        let id = archive.entries()[0].id();
        let mut reader = archive.entry_reader(id).unwrap().unwrap();
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn reports_truncated_chunk_length_and_payload_as_typed_errors() {
    let mut truncated_length = vec![BLK_DATA];
    write_int(&mut truncated_length, 1);
    truncated_length.extend_from_slice(&[0, 5]);
    let bytes = archive_with_block(0, POSITION_SET, None, &truncated_length);
    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive.entries()[0].id();
    let mut reader = archive.entry_reader(id).unwrap().unwrap();
    let error = reader.read_to_end(&mut Vec::new()).unwrap_err();
    assert!(matches!(
        pgdump_error(&error),
        PgDumpError::TruncatedDataChunkLength { dump_id: 1, .. }
    ));

    let mut truncated_payload = vec![BLK_DATA];
    write_int(&mut truncated_payload, 1);
    write_int(&mut truncated_payload, 5);
    truncated_payload.extend_from_slice(b"ab");
    let bytes = archive_with_block(0, POSITION_SET, None, &truncated_payload);
    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive.entries()[0].id();
    let mut reader = archive.entry_reader(id).unwrap().unwrap();
    let error = reader.read_to_end(&mut Vec::new()).unwrap_err();
    assert!(matches!(
        pgdump_error(&error),
        PgDumpError::TruncatedDataChunk {
            dump_id: 1,
            remaining: 3,
            ..
        }
    ));
}

#[test]
fn arbitrary_short_archive_reads_preserve_none_and_gzip_output() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let reader = ShortReadSeek::new(Cursor::new(fixture(fixture_name)), 1);
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
    }
}

#[test]
fn gzip_accepts_empty_and_arbitrarily_split_zlib_streams() {
    const EMPTY_ZLIB: &[u8] = &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
    const HELLO_ZLIB: &[u8] = &[
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];

    for (chunks, expected) in [
        (vec![EMPTY_ZLIB], b"".as_slice()),
        (
            vec![&HELLO_ZLIB[..1], &HELLO_ZLIB[1..7], &HELLO_ZLIB[7..]],
            b"hello".as_slice(),
        ),
    ] {
        let bytes = archive_with_block(1, POSITION_SET, None, &data_block(BLK_DATA, 1, &chunks));
        let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
        let id = archive.entries()[0].id();
        let mut reader = archive.entry_reader(id).unwrap().unwrap();
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn corrupt_and_truncated_gzip_are_typed_decompression_errors() {
    const HELLO_ZLIB: &[u8] = &[
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];

    for compressed in [b"not-zlib".as_slice(), &HELLO_ZLIB[..10]] {
        let bytes = archive_with_block(
            1,
            POSITION_SET,
            None,
            &data_block(BLK_DATA, 1, &[compressed]),
        );
        let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
        let id = archive.entries()[0].id();
        let mut reader = archive.entry_reader(id).unwrap().unwrap();
        let error = reader.read_to_end(&mut Vec::new()).unwrap_err();
        assert!(matches!(
            pgdump_error(&error),
            PgDumpError::DecompressionFailed {
                dump_id: 1,
                algorithm: "gzip",
                ..
            }
        ));
    }
}

#[test]
fn reading_one_byte_does_not_buffer_the_complete_entry() {
    let payload = vec![b'x'; 1024 * 1024];
    let block = data_block(BLK_DATA, 1, &[&payload]);
    let bytes = archive_with_block(0, POSITION_SET, None, &block);
    let total_len = bytes.len();
    let bytes_read = Rc::new(Cell::new(0_u64));
    let reader = TrackingReader::new(bytes, Rc::clone(&bytes_read));
    let mut archive = Archive::open(reader).unwrap();
    let id = archive.entries()[0].id();
    let after_metadata = bytes_read.get();

    let mut entry = archive.entry_reader(id).unwrap().unwrap();
    let mut one = [0_u8; 1];
    entry.read_exact(&mut one).unwrap();

    assert_eq!(one, [b'x']);
    assert!(bytes_read.get() <= after_metadata + 12);
    assert!(usize::try_from(bytes_read.get()).unwrap() < total_len);
}

fn assert_official_fixture_streams(name: &str) {
    let mut archive = Archive::open(Cursor::new(fixture(name))).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut entry = archive.entry_reader(id).unwrap().unwrap();
    let mut output = Vec::new();
    entry.read_to_end(&mut output).unwrap();
    assert_eq!(output, EXPECTED_COPY_STREAM);
}

fn pgdump_error(error: &io::Error) -> &PgDumpError {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<PgDumpError>())
        .expect("entry Read errors must preserve a typed PgDumpError source")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}

fn archive_with_block(
    compression: u8,
    offset_state: u8,
    offset_override: Option<u64>,
    block: &[u8],
) -> Vec<u8> {
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
    write_string(
        &mut bytes,
        Some(b"COPY public.data (value) FROM stdin;\n"),
    );
    write_string(&mut bytes, Some(b"public"));
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_int(&mut bytes, 0);
    write_string(&mut bytes, Some(b"postgres"));
    write_string(&mut bytes, Some(b"false"));
    write_string(&mut bytes, None);
    bytes.push(offset_state);
    let offset_start = bytes.len();
    bytes.extend_from_slice(&[0; 8]);

    let data_offset = u64::try_from(bytes.len()).unwrap();
    let stored_offset = offset_override.unwrap_or(data_offset);
    bytes[offset_start..offset_start + 8].copy_from_slice(&stored_offset.to_le_bytes());
    bytes.extend_from_slice(block);
    bytes
}

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
struct ShortReadSeek<R> {
    inner: R,
    max_read: usize,
}

impl<R> ShortReadSeek<R> {
    fn new(inner: R, max_read: usize) -> Self {
        Self { inner, max_read }
    }
}

impl<R: Read> Read for ShortReadSeek<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let limit = buffer.len().min(self.max_read);
        self.inner.read(&mut buffer[..limit])
    }
}

impl<R: Seek> Seek for ShortReadSeek<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
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
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
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
