use crate::{
    FieldRef, PgDumpError,
    copy::{CopyParserLimits, CopyRowReader},
};
use std::{
    cell::Cell,
    io::{self, Cursor, Read},
    rc::Rc,
};

#[test]
fn row_byte_limit_accepts_below_and_exact_and_rejects_above() {
    let limits = CopyParserLimits::new(4, 8);

    for input in [b"abc\n".as_slice(), b"abcd\n".as_slice()] {
        let mut rows = CopyRowReader::with_limits(Cursor::new(input), limits);
        assert!(rows.next_row().unwrap().is_some());
        assert!(rows.next_row().unwrap().is_none());
    }

    let mut rows = CopyRowReader::with_limits(Cursor::new(b"abcde\n".as_slice()), limits);
    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyRowByteLimitExceeded {
            row: 1,
            limit: 4,
            actual: 5,
            ..
        }
    ));
}

#[test]
fn field_count_limit_accepts_below_and_exact_and_rejects_above() {
    let limits = CopyParserLimits::new(64, 3);

    for (input, expected_fields) in [(b"a\tb\n".as_slice(), 2), (b"a\tb\tc\n".as_slice(), 3)] {
        let mut rows = CopyRowReader::with_limits(Cursor::new(input), limits);
        assert_eq!(rows.next_row().unwrap().unwrap().len(), expected_fields);
    }

    let mut rows = CopyRowReader::with_limits(Cursor::new(b"a\tb\tc\td\n".as_slice()), limits);
    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyFieldCountLimitExceeded {
            row: 1,
            limit: 3,
            actual: 4,
            ..
        }
    ));
}

#[test]
fn consumed_bytes_count_physical_spellings_and_not_read_ahead() {
    let input = b"a\\tb\tc\n\\.\nTAIL";
    let bytes_read = Rc::new(Cell::new(0_u64));
    let tracking = TrackingReader::new(input.as_slice(), Rc::clone(&bytes_read));
    let limits = CopyParserLimits::new(64, 8);
    let mut rows = CopyRowReader::with_limits(tracking, limits);

    {
        let row = rows.next_row().unwrap().unwrap();
        assert_eq!(row.field(0), Some(FieldRef::Bytes(b"a\tb")));
        assert_eq!(row.field(1), Some(FieldRef::Bytes(b"c")));
    }
    assert_eq!(rows.consumed_input_bytes(), 7);
    assert!(bytes_read.get() > rows.consumed_input_bytes());

    assert!(rows.next_row().unwrap().is_none());
    assert_eq!(rows.consumed_input_bytes(), 10);
    assert!(bytes_read.get() > rows.consumed_input_bytes());
}

#[test]
fn consumed_bytes_are_independent_of_source_segmentation() {
    let input = b"a\\tb\tc\n\\.\nTAIL";
    let limits = CopyParserLimits::new(64, 8);

    let contiguous = accounting_checkpoints(CopyRowReader::with_limits(
        Cursor::new(input.as_slice()),
        limits,
    ));
    let one_byte = accounting_checkpoints(CopyRowReader::with_limits(
        ShortRead::new(Cursor::new(input.as_slice()), 1),
        limits,
    ));

    assert_eq!(contiguous, (7, 10));
    assert_eq!(one_byte, contiguous);
}

#[test]
fn consumed_byte_counter_overflow_is_typed_and_controlled() {
    let limits = CopyParserLimits::new(64, 8);
    let mut rows =
        CopyRowReader::with_limits_and_consumed(Cursor::new(b"a\n".as_slice()), limits, u64::MAX);

    assert!(matches!(
        rows.next_row().unwrap_err(),
        PgDumpError::CopyConsumedByteCountOverflow {
            row: 1,
            consumed: u64::MAX,
            increment: 1,
        }
    ));
}

fn accounting_checkpoints<R: Read>(mut rows: CopyRowReader<R>) -> (u64, u64) {
    {
        let row = rows.next_row().unwrap().unwrap();
        assert_eq!(row.len(), 2);
    }
    let after_row = rows.consumed_input_bytes();
    assert!(rows.next_row().unwrap().is_none());
    (after_row, rows.consumed_input_bytes())
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

#[derive(Debug)]
struct TrackingReader<R> {
    inner: R,
    bytes_read: Rc<Cell<u64>>,
}

impl<R> TrackingReader<R> {
    fn new(inner: R, bytes_read: Rc<Cell<u64>>) -> Self {
        Self { inner, bytes_read }
    }
}

impl<R: Read> Read for TrackingReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(output)?;
        let read = u64::try_from(read).unwrap();
        self.bytes_read.set(self.bytes_read.get() + read);
        Ok(usize::try_from(read).unwrap())
    }
}
