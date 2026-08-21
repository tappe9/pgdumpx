use crate::{
    Limits, PgDumpError,
    custom::primitives::{
        ArchiveIntegerSize, ArchiveOffset, ArchiveOffsetSize, read_archive_integer,
        read_archive_offset, read_archive_string,
    },
    io::archive_reader::ArchiveReader,
};
use std::{
    cell::Cell,
    error::Error as _,
    io::{self, Cursor, Read},
    rc::Rc,
};

#[test]
fn exact_read_succeeds_across_partial_reads_and_tracks_offset() {
    let mut reader = ArchiveReader::new(ChunkedReader::new(b"abcd", 2));
    let mut output = [0_u8; 4];

    reader.read_exact(&mut output).unwrap();

    assert_eq!(&output, b"abcd");
    assert_eq!(reader.offset(), 4);
}

#[test]
fn exact_read_reports_short_read_at_first_missing_byte() {
    let mut reader = ArchiveReader::new(Cursor::new(b"ab"));
    let mut output = [0_u8; 4];

    let error = reader.read_exact(&mut output).unwrap_err();

    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 2 }));
    assert_eq!(reader.offset(), 2);
}

#[test]
fn exact_read_rejects_counter_overflow_before_reading() {
    let read_bytes = Rc::new(Cell::new(0));
    let source = CountingReader::new(b"x", Rc::clone(&read_bytes));
    let mut reader = ArchiveReader::new_at(source, u64::MAX);
    let mut output = [0_u8; 1];

    let error = reader.read_exact(&mut output).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ArithmeticOverflow { offset: u64::MAX }
    ));
    assert_eq!(read_bytes.get(), 0);
    assert_eq!(reader.offset(), u64::MAX);
}

#[test]
fn io_errors_preserve_offset_and_source() {
    let mut reader = ArchiveReader::new(FailingReader);
    let mut output = [0_u8; 1];

    let error = reader.read_exact(&mut output).unwrap_err();

    assert!(matches!(&error, PgDumpError::Io { offset: 0, .. }));
    assert_eq!(
        error.source().map(|source| source.to_string()).as_deref(),
        Some("boom")
    );
}

#[test]
fn archive_integer_decodes_representative_values_and_boundaries() {
    for expected in [
        0_i64,
        1,
        -1,
        0x12_34_56,
        -0x12_34_56,
        i64::from(i32::MAX),
        i64::from(i32::MIN),
    ] {
        let mut reader = ArchiveReader::new(Cursor::new(encode_integer(expected, 4)));

        let actual = read_archive_integer(&mut reader, integer_size()).unwrap();

        assert_eq!(i64::from(actual), expected);
        assert_eq!(reader.offset(), 5);
    }
}

#[test]
fn archive_integer_treats_any_nonzero_sign_as_negative() {
    let mut encoded = encode_integer(7, 4);
    encoded[0] = 0xff;
    let mut reader = ArchiveReader::new(Cursor::new(encoded));

    assert_eq!(
        read_archive_integer(&mut reader, integer_size()).unwrap(),
        -7
    );

    let mut negative_zero = encode_integer(0, 4);
    negative_zero[0] = 1;
    let mut reader = ArchiveReader::new(Cursor::new(negative_zero));
    assert_eq!(
        read_archive_integer(&mut reader, integer_size()).unwrap(),
        0
    );
}

#[test]
fn archive_integer_rejects_unsupported_sizes() {
    for size in [0_u8, 5] {
        let error = ArchiveIntegerSize::new(size, 17).unwrap_err();
        assert!(matches!(
            error,
            PgDumpError::UnsupportedArchiveIntegerSize {
                size: actual,
                offset: 17
            } if actual == size
        ));
    }
}

#[test]
fn archive_integer_rejects_values_outside_i32() {
    for value in [i64::from(i32::MAX) + 1, i64::from(i32::MIN) - 1] {
        let mut reader = ArchiveReader::new(Cursor::new(encode_integer(value, 4)));
        let error = read_archive_integer(&mut reader, integer_size()).unwrap_err();
        assert!(matches!(
            error,
            PgDumpError::ArchiveIntegerOutOfRange { offset: 0 }
        ));
    }
}

#[test]
fn archive_integer_truncation_is_typed() {
    let mut reader = ArchiveReader::new(Cursor::new([0_u8, 1, 2]));

    let error = read_archive_integer(&mut reader, integer_size()).unwrap_err();

    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 3 }));
}

#[test]
fn archive_offset_preserves_not_set_set_and_no_data_states() {
    let size = ArchiveOffsetSize::new(8, 0).unwrap();
    let cases = [
        (1_u8, 99_u128, ArchiveOffset::PositionNotSet),
        (2, 0x12_34_56_78, ArchiveOffset::Position(0x12_34_56_78)),
        (3, 99, ArchiveOffset::NoData),
    ];

    for (state, value, expected) in cases {
        let mut reader = ArchiveReader::new(Cursor::new(encode_offset(state, 8, value)));
        assert_eq!(read_archive_offset(&mut reader, size).unwrap(), expected);
        assert_eq!(reader.offset(), 9);
    }
}

#[test]
fn archive_offset_rejects_invalid_size_and_state() {
    assert!(matches!(
        ArchiveOffsetSize::new(0, 23).unwrap_err(),
        PgDumpError::InvalidArchiveOffsetSize {
            size: 0,
            offset: 23
        }
    ));

    let size = ArchiveOffsetSize::new(8, 0).unwrap();
    let mut reader = ArchiveReader::new(Cursor::new(encode_offset(0, 8, 0)));
    let error = read_archive_offset(&mut reader, size).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::InvalidArchiveOffsetState {
            state: 0,
            offset: 0
        }
    ));
}

#[test]
fn archive_offset_accepts_zero_extension_and_rejects_overflow() {
    let size = ArchiveOffsetSize::new(9, 0).unwrap();
    let mut valid = ArchiveReader::new(Cursor::new(encode_offset(2, 9, u128::from(u64::MAX))));
    assert_eq!(
        read_archive_offset(&mut valid, size).unwrap(),
        ArchiveOffset::Position(u64::MAX)
    );

    let mut overflowing = ArchiveReader::new(Cursor::new(encode_offset(2, 9, 1_u128 << 64)));
    let error = read_archive_offset(&mut overflowing, size).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::ArchiveOffsetOutOfRange { offset: 9 }
    ));
}

#[test]
fn archive_offset_truncation_is_typed() {
    let size = ArchiveOffsetSize::new(8, 0).unwrap();
    let mut reader = ArchiveReader::new(Cursor::new([2_u8, 1, 2]));

    let error = read_archive_offset(&mut reader, size).unwrap_err();

    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 3 }));
}

#[test]
fn archive_string_decodes_null_empty_non_utf8_and_exact_limit() {
    let cases = [
        (-7_i64, Vec::new(), 3, None),
        (0, Vec::new(), 0, Some(Vec::new())),
        (
            2,
            b"ok".to_vec(),
            3,
            Some(b"ok".to_vec()),
        ),
        (
            3,
            vec![0xff, 0x00, 0xfe],
            3,
            Some(vec![0xff, 0x00, 0xfe]),
        ),
    ];

    for (length, payload, limit, expected) in cases {
        let mut encoded = encode_integer(length, 4);
        encoded.extend_from_slice(&payload);
        let mut reader = ArchiveReader::new(Cursor::new(encoded));
        assert_eq!(
            read_archive_string(&mut reader, integer_size(), limit).unwrap(),
            expected
        );
    }
}

#[test]
fn archive_string_rejects_oversize_before_payload_read() {
    let read_bytes = Rc::new(Cell::new(0));
    let mut encoded = encode_integer(4, 4);
    encoded.extend_from_slice(b"data");
    let source = CountingReader::new(&encoded, Rc::clone(&read_bytes));
    let mut reader = ArchiveReader::new(source);

    let error =
        read_archive_string(&mut reader, integer_size(), 3).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ArchiveStringLimitExceeded {
            length: 4,
            limit: 3,
            offset: 0
        }
    ));
    assert_eq!(reader.offset(), 5);
    assert_eq!(read_bytes.get(), 5);
}

#[test]
fn archive_string_payload_truncation_is_typed() {
    let mut encoded = encode_integer(3, 4);
    encoded.extend_from_slice(b"ab");
    let mut reader = ArchiveReader::new(Cursor::new(encoded));

    let error =
        read_archive_string(&mut reader, integer_size(), 3).unwrap_err();

    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 7 }));
}

#[test]
fn alpha1_archive_string_path_has_an_explicit_finite_limit() {
    assert!(Limits::default().max_string_bytes().max_bytes() > 0);
    assert!(Limits::default().max_string_bytes().max_bytes() < usize::MAX);
}

fn integer_size() -> ArchiveIntegerSize {
    ArchiveIntegerSize::new(4, 0).unwrap()
}

fn encode_integer(value: i64, size: u8) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(usize::from(size) + 1);
    encoded.push(u8::from(value.is_negative()));
    let mut magnitude = value.unsigned_abs();
    for _ in 0..size {
        encoded.push((magnitude & 0xff) as u8);
        magnitude >>= 8;
    }
    encoded
}

fn encode_offset(state: u8, size: u8, mut value: u128) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(usize::from(size) + 1);
    encoded.push(state);
    for _ in 0..size {
        encoded.push((value & 0xff) as u8);
        value >>= 8;
    }
    encoded
}

struct ChunkedReader<'a> {
    bytes: &'a [u8],
    position: usize,
    chunk_size: usize,
}

impl<'a> ChunkedReader<'a> {
    fn new(bytes: &'a [u8], chunk_size: usize) -> Self {
        Self {
            bytes,
            position: 0,
            chunk_size,
        }
    }
}

impl Read for ChunkedReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let remaining = &self.bytes[self.position..];
        let count = remaining.len().min(output.len()).min(self.chunk_size);
        output[..count].copy_from_slice(&remaining[..count]);
        self.position += count;
        Ok(count)
    }
}

struct CountingReader<'a> {
    bytes: Cursor<&'a [u8]>,
    read_bytes: Rc<Cell<usize>>,
}

impl<'a> CountingReader<'a> {
    fn new(bytes: &'a [u8], read_bytes: Rc<Cell<usize>>) -> Self {
        Self {
            bytes: Cursor::new(bytes),
            read_bytes,
        }
    }
}

impl Read for CountingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.bytes.read(output)?;
        self.read_bytes.set(self.read_bytes.get() + count);
        Ok(count)
    }
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _output: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("boom"))
    }
}
