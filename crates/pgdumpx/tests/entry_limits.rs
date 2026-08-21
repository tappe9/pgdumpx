use pgdumpx::{Archive, EntryReadLimits, ErrorCategory, PgDumpError, ResourceLimit};
use std::{
    error::Error as _,
    io::{self, Cursor, Read, Write},
};

const POSITION_SET: u8 = 2;
const BLK_DATA: u8 = 1;

#[test]
fn bounded_reader_distinguishes_exact_eof_from_limit_exceeded() {
    let bytes = archive_with_payload(b"abc");
    let mut archive = Archive::open(Cursor::new(bytes.clone())).unwrap();
    let id = archive.entries()[0].id();
    let mut reader = archive
        .entry_reader_with_limits(
            id,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(3),
        )
        .unwrap()
        .unwrap();
    let mut exact = Vec::new();
    reader.read_to_end(&mut exact).unwrap();
    assert_eq!(exact, b"abc");

    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive.entries()[0].id();
    let mut reader = archive
        .entry_reader_with_limits(
            id,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(2),
        )
        .unwrap()
        .unwrap();
    let mut partial = Vec::new();
    let error = reader.read_to_end(&mut partial).unwrap_err();
    assert_eq!(partial, b"ab");

    let error = pgdump_error(&error);
    assert_eq!(error.category(), ErrorCategory::Resource);
    let context = error.limit_context().expect("raw limit context");
    assert_eq!(context.resource(), ResourceLimit::EntryDecompressedBytes);
    assert_eq!(context.limit(), 2);
    assert_eq!(context.consumed(), 3);
}

#[test]
fn copy_entry_to_handles_below_exact_and_above_limits() {
    for limit in [4, 3] {
        let mut archive = Archive::open(Cursor::new(archive_with_payload(b"abc"))).unwrap();
        let id = archive.entries()[0].id();
        let mut output = Vec::new();
        let copied = archive
            .copy_entry_to(
                id,
                &mut output,
                EntryReadLimits::unlimited().with_max_decompressed_bytes(limit),
            )
            .unwrap();
        assert_eq!(copied, 3);
        assert_eq!(output, b"abc");
    }

    let mut archive = Archive::open(Cursor::new(archive_with_payload(b"abc"))).unwrap();
    let id = archive.entries()[0].id();
    let mut output = Vec::new();
    let error = archive
        .copy_entry_to(
            id,
            &mut output,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(2),
        )
        .unwrap_err();
    assert_eq!(output, b"ab");
    assert_eq!(error.category(), ErrorCategory::Resource);
    let context = error.limit_context().expect("raw limit context");
    assert_eq!(context.resource(), ResourceLimit::EntryDecompressedBytes);
    assert_eq!(context.limit(), 2);
    assert_eq!(context.consumed(), 3);
}

#[test]
fn bounded_copy_is_binary_safe() {
    let payload = [0xff, 0x00, 0x80, b'\n'];
    let mut archive = Archive::open(Cursor::new(archive_with_payload(&payload))).unwrap();
    let id = archive.entries()[0].id();
    let mut output = Vec::new();
    let copied = archive
        .copy_entry_to(
            id,
            &mut output,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(4),
        )
        .unwrap();
    assert_eq!(copied, 4);
    assert_eq!(output, payload);
}

#[test]
fn copy_entry_to_retries_short_writes_and_preserves_writer_errors() {
    let mut archive = Archive::open(Cursor::new(archive_with_payload(b"abcdef"))).unwrap();
    let id = archive.entries()[0].id();
    let mut short = ShortWriter::new(2);
    let copied = archive
        .copy_entry_to(id, &mut short, EntryReadLimits::unlimited())
        .unwrap();
    assert_eq!(copied, 6);
    assert_eq!(short.bytes, b"abcdef");

    let mut archive = Archive::open(Cursor::new(archive_with_payload(b"abcdef"))).unwrap();
    let id = archive.entries()[0].id();
    let mut failing = FailingWriter::new(2);
    let error = archive
        .copy_entry_to(id, &mut failing, EntryReadLimits::unlimited())
        .unwrap_err();
    assert_eq!(failing.bytes, b"ab");
    assert_eq!(error.category(), ErrorCategory::Io);
    assert!(error.source().is_some());
}

fn pgdump_error(error: &io::Error) -> &PgDumpError {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<PgDumpError>())
        .expect("bounded Read errors must preserve a typed PgDumpError source")
}

#[derive(Debug)]
struct ShortWriter {
    max_write: usize,
    bytes: Vec<u8>,
}

impl ShortWriter {
    fn new(max_write: usize) -> Self {
        Self {
            max_write,
            bytes: Vec::new(),
        }
    }
}

impl Write for ShortWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = buffer.len().min(self.max_write);
        self.bytes.extend_from_slice(&buffer[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct FailingWriter {
    remaining: usize,
    bytes: Vec<u8>,
}

impl FailingWriter {
    fn new(remaining: usize) -> Self {
        Self {
            remaining,
            bytes: Vec::new(),
        }
    }
}

impl Write for FailingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "writer failed"));
        }
        let count = buffer.len().min(self.remaining);
        self.bytes.extend_from_slice(&buffer[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn archive_with_payload(payload: &[u8]) -> Vec<u8> {
    let block = data_block(BLK_DATA, 1, &[payload]);
    let mut bytes = complete_header();
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
    bytes.extend_from_slice(&block);
    bytes
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
