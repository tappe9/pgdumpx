use crate::PgDumpError;
use std::{
    fmt,
    io::{BufRead, BufReader, Read},
    iter::FusedIterator,
};

const PROVISIONAL_MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;
const PROVISIONAL_MAX_FIELDS: u64 = 4 * 1024;
const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;
const COPY_TERMINATOR: &[u8] = b"\\.";

const ALPHA1_COPY_LIMITS: CopyParserLimits =
    CopyParserLimits::new(PROVISIONAL_MAX_ROW_BYTES, PROVISIONAL_MAX_FIELDS);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CopyParserLimits {
    max_row_bytes: u64,
    max_fields: u64,
}

impl CopyParserLimits {
    pub(crate) const fn new(max_row_bytes: u64, max_fields: u64) -> Self {
        Self {
            max_row_bytes,
            max_fields,
        }
    }
}

/// A borrowed logical field from a PostgreSQL COPY text row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldRef<'a> {
    /// The raw field spelling was exactly PostgreSQL's `\N` NULL marker.
    Null,
    /// Logical field bytes after PostgreSQL COPY backslash decoding.
    Bytes(&'a [u8]),
}

/// A borrowed COPY text row backed by reusable parser storage.
///
/// The row remains valid until the originating [`CopyRowReader`] is mutably
/// borrowed again. This lending shape intentionally does not implement
/// [`Iterator`].
pub struct Row<'a> {
    bytes: &'a [u8],
    fields: &'a [FieldSpan],
}

impl Row<'_> {
    /// Returns the number of fields in this row.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether this row has no fields.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Returns one field by zero-based index.
    pub fn field(&self, index: usize) -> Option<FieldRef<'_>> {
        self.fields
            .get(index)
            .map(|span| span.resolve(self.bytes))
    }

    /// Iterates over all borrowed fields in column order.
    pub fn fields(
        &self,
    ) -> impl ExactSizeIterator<Item = FieldRef<'_>> + FusedIterator + '_ {
        self.fields
            .iter()
            .map(move |span| span.resolve(self.bytes))
    }
}

impl fmt::Debug for Row<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.fields()).finish()
    }
}

#[derive(Clone, Copy, Debug)]
enum FieldSpan {
    Null,
    Bytes { start: usize, end: usize },
}

impl FieldSpan {
    fn resolve<'a>(self, bytes: &'a [u8]) -> FieldRef<'a> {
        match self {
            Self::Null => FieldRef::Null,
            Self::Bytes { start, end } => FieldRef::Bytes(&bytes[start..end]),
        }
    }
}

/// A lending, byte-oriented parser for PostgreSQL COPY text rows.
///
/// Input is consumed incrementally from any [`Read`] implementation. The
/// parser stores only the current physical row and its decoded logical fields;
/// it never buffers the complete COPY stream.
pub struct CopyRowReader<R> {
    input: CopyInput<R>,
    limits: CopyParserLimits,
    raw_row: Vec<u8>,
    logical_bytes: Vec<u8>,
    field_spans: Vec<FieldSpan>,
    next_row_number: u64,
    finished: bool,
}

impl<R: Read> CopyRowReader<R> {
    /// Creates a COPY text row reader using provisional finite v0.1 bounds.
    pub fn new(reader: R) -> Self {
        Self::with_limits(reader, ALPHA1_COPY_LIMITS)
    }

    pub(crate) fn with_limits(reader: R, limits: CopyParserLimits) -> Self {
        Self {
            input: CopyInput::new(reader),
            limits,
            raw_row: Vec::new(),
            logical_bytes: Vec::new(),
            field_spans: Vec::new(),
            next_row_number: 1,
            finished: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_limits_and_consumed(
        reader: R,
        limits: CopyParserLimits,
        consumed: u64,
    ) -> Self {
        let mut parser = Self::with_limits(reader, limits);
        parser.input.consumed = consumed;
        parser
    }

    /// Parses and lends the next logical row.
    ///
    /// `Ok(None)` is returned after a standalone `\.` terminator or after the
    /// underlying COPY stream reaches EOF. A returned row borrows reusable
    /// parser storage, so another call is rejected by the borrow checker until
    /// that row is no longer used.
    pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError> {
        if self.finished {
            return Ok(None);
        }

        self.raw_row.clear();
        self.logical_bytes.clear();
        self.field_spans.clear();

        let row = self.next_row_number;
        let row_start = self.input.consumed();
        let record_end = self.read_record(row)?;

        if self.raw_row.is_empty() && record_end == RecordEnd::Eof {
            self.finished = true;
            return Ok(None);
        }

        if self.raw_row == COPY_TERMINATOR {
            if record_end == RecordEnd::Line {
                self.finished = true;
                return Ok(None);
            }
            return Err(PgDumpError::MalformedCopyTerminator {
                row,
                byte_offset: row_start,
            });
        }

        let field_count = inspect_field_layout(
            &self.raw_row,
            self.limits.max_fields,
            row,
            row_start,
        )?;
        self.prepare_decoded_storage(row, field_count)?;
        decode_fields(
            &self.raw_row,
            &mut self.logical_bytes,
            &mut self.field_spans,
            row,
            row_start,
        )?;

        if record_end == RecordEnd::Eof {
            self.finished = true;
        } else {
            self.next_row_number = row
                .checked_add(1)
                .ok_or(PgDumpError::CopyRowNumberOverflow { row })?;
        }

        Ok(Some(Row {
            bytes: &self.logical_bytes,
            fields: &self.field_spans,
        }))
    }

    pub(crate) const fn consumed_input_bytes(&self) -> u64 {
        self.input.consumed()
    }

    fn read_record(&mut self, row: u64) -> Result<RecordEnd, PgDumpError> {
        let mut escaped = false;
        loop {
            let Some(byte) = self.input.next_byte(row)? else {
                if escaped {
                    return Err(PgDumpError::MalformedCopyEscape {
                        row,
                        byte_offset: self.input.consumed().saturating_sub(1),
                    });
                }
                return Ok(RecordEnd::Eof);
            };

            if escaped {
                self.push_raw_byte(row, byte)?;
                escaped = false;
                continue;
            }

            match byte {
                b'\\' => {
                    self.push_raw_byte(row, byte)?;
                    escaped = true;
                }
                b'\n' => return Ok(RecordEnd::Line),
                b'\r' => {
                    if self.input.peek_byte(row)? == Some(b'\n') {
                        let consumed = self.input.next_byte(row)?;
                        debug_assert_eq!(consumed, Some(b'\n'));
                    }
                    return Ok(RecordEnd::Line);
                }
                _ => self.push_raw_byte(row, byte)?,
            }
        }
    }

    fn push_raw_byte(&mut self, row: u64, byte: u8) -> Result<(), PgDumpError> {
        let actual_usize = self
            .raw_row
            .len()
            .checked_add(1)
            .ok_or(PgDumpError::ArithmeticOverflow {
                offset: self.input.consumed(),
            })?;
        let actual = u64::try_from(actual_usize).map_err(|_| PgDumpError::ArithmeticOverflow {
            offset: self.input.consumed(),
        })?;
        if actual > self.limits.max_row_bytes {
            return Err(PgDumpError::CopyRowByteLimitExceeded {
                row,
                limit: self.limits.max_row_bytes,
                actual,
                byte_offset: self.input.consumed(),
            });
        }

        if actual_usize > self.raw_row.capacity() {
            let max_capacity = usize::try_from(self.limits.max_row_bytes).unwrap_or(usize::MAX);
            let proposed = if self.raw_row.capacity() == 0 {
                INITIAL_ROW_CAPACITY_BYTES
            } else {
                self.raw_row.capacity().saturating_mul(2)
            };
            let target = proposed.max(actual_usize).min(max_capacity);
            let additional = target.checked_sub(self.raw_row.len()).ok_or(
                PgDumpError::ArithmeticOverflow {
                    offset: self.input.consumed(),
                },
            )?;
            self.raw_row
                .try_reserve_exact(additional)
                .map_err(|_| PgDumpError::CopyRowAllocationFailed {
                    row,
                    requested: u64::try_from(target).unwrap_or(u64::MAX),
                })?;
        }

        self.raw_row.push(byte);
        Ok(())
    }

    fn prepare_decoded_storage(
        &mut self,
        row: u64,
        field_count: u64,
    ) -> Result<(), PgDumpError> {
        if self.logical_bytes.capacity() < self.raw_row.len() {
            self.logical_bytes
                .try_reserve_exact(self.raw_row.len())
                .map_err(|_| PgDumpError::CopyRowAllocationFailed {
                    row,
                    requested: u64::try_from(self.raw_row.len()).unwrap_or(u64::MAX),
                })?;
        }

        let field_count = usize::try_from(field_count).map_err(|_| {
            PgDumpError::CopyFieldAllocationFailed {
                row,
                requested: field_count,
            }
        })?;
        if self.field_spans.capacity() < field_count {
            self.field_spans
                .try_reserve_exact(field_count)
                .map_err(|_| PgDumpError::CopyFieldAllocationFailed {
                    row,
                    requested: u64::try_from(field_count).unwrap_or(u64::MAX),
                })?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordEnd {
    Line,
    Eof,
}

struct CopyInput<R> {
    reader: BufReader<R>,
    consumed: u64,
}

impl<R: Read> CopyInput<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            consumed: 0,
        }
    }

    const fn consumed(&self) -> u64 {
        self.consumed
    }

    fn peek_byte(&mut self, row: u64) -> Result<Option<u8>, PgDumpError> {
        self.reader
            .fill_buf()
            .map(|buffer| buffer.first().copied())
            .map_err(|source| PgDumpError::CopyIo {
                row,
                consumed: self.consumed,
                source,
            })
    }

    fn next_byte(&mut self, row: u64) -> Result<Option<u8>, PgDumpError> {
        let byte = self.peek_byte(row)?;
        if byte.is_some() {
            let increment = 1;
            let next = self.consumed.checked_add(increment).ok_or(
                PgDumpError::CopyConsumedByteCountOverflow {
                    row,
                    consumed: self.consumed,
                    increment,
                },
            )?;
            self.reader.consume(1);
            self.consumed = next;
        }
        Ok(byte)
    }
}

fn inspect_field_layout(
    raw: &[u8],
    max_fields: u64,
    row: u64,
    row_start: u64,
) -> Result<u64, PgDumpError> {
    let mut fields = 1_u64;
    let mut index = 0_usize;
    while index < raw.len() {
        if raw[index] == b'\\' {
            let Some(escaped) = raw.get(index + 1).copied() else {
                return Err(PgDumpError::MalformedCopyEscape {
                    row,
                    byte_offset: byte_offset(row_start, index)?,
                });
            };
            if escaped == b'.' {
                return Err(PgDumpError::MalformedCopyTerminator {
                    row,
                    byte_offset: byte_offset(row_start, index)?,
                });
            }
            index += 2;
            continue;
        }

        if raw[index] == b'\t' {
            fields = fields
                .checked_add(1)
                .ok_or(PgDumpError::ArithmeticOverflow {
                    offset: byte_offset(row_start, index)?,
                })?;
            if fields > max_fields {
                return Err(PgDumpError::CopyFieldCountLimitExceeded {
                    row,
                    limit: max_fields,
                    actual: fields,
                    byte_offset: byte_offset(row_start, index)?,
                });
            }
        }
        index += 1;
    }

    if fields > max_fields {
        return Err(PgDumpError::CopyFieldCountLimitExceeded {
            row,
            limit: max_fields,
            actual: fields,
            byte_offset: row_start,
        });
    }
    Ok(fields)
}

fn decode_fields(
    raw: &[u8],
    logical: &mut Vec<u8>,
    spans: &mut Vec<FieldSpan>,
    row: u64,
    row_start: u64,
) -> Result<(), PgDumpError> {
    let mut field_start = 0_usize;
    let mut index = 0_usize;
    while index < raw.len() {
        match raw[index] {
            b'\\' => index += 2,
            b'\t' => {
                decode_field(
                    &raw[field_start..index],
                    logical,
                    spans,
                    row,
                    byte_offset(row_start, field_start)?,
                )?;
                field_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    decode_field(
        &raw[field_start..],
        logical,
        spans,
        row,
        byte_offset(row_start, field_start)?,
    )
}

fn decode_field(
    raw: &[u8],
    logical: &mut Vec<u8>,
    spans: &mut Vec<FieldSpan>,
    row: u64,
    field_offset: u64,
) -> Result<(), PgDumpError> {
    if raw == b"\\N" {
        spans.push(FieldSpan::Null);
        return Ok(());
    }

    let start = logical.len();
    let mut index = 0_usize;
    while index < raw.len() {
        let byte = raw[index];
        if byte != b'\\' {
            logical.push(byte);
            index += 1;
            continue;
        }

        let Some(escaped) = raw.get(index + 1).copied() else {
            return Err(PgDumpError::MalformedCopyEscape {
                row,
                byte_offset: byte_offset(field_offset, index)?,
            });
        };
        index += 2;

        let decoded = match escaped {
            b'0'..=b'7' => {
                let mut value = u16::from(escaped - b'0');
                let mut digits = 1_u8;
                while digits < 3 {
                    let Some(next) = raw.get(index).copied() else {
                        break;
                    };
                    if !(b'0'..=b'7').contains(&next) {
                        break;
                    }
                    value = (value << 3) + u16::from(next - b'0');
                    index += 1;
                    digits += 1;
                }
                (value & 0xff) as u8
            }
            b'x' => {
                let Some(first) = raw.get(index).copied().and_then(hex_value) else {
                    logical.push(b'x');
                    continue;
                };
                index += 1;
                let mut value = first;
                if let Some(second) = raw.get(index).copied().and_then(hex_value) {
                    value = (value << 4) | second;
                    index += 1;
                }
                value
            }
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            other => other,
        };
        logical.push(decoded);
    }

    spans.push(FieldSpan::Bytes {
        start,
        end: logical.len(),
    });
    Ok(())
}

fn byte_offset(start: u64, index: usize) -> Result<u64, PgDumpError> {
    let index = u64::try_from(index).map_err(|_| PgDumpError::ArithmeticOverflow {
        offset: start,
    })?;
    start
        .checked_add(index)
        .ok_or(PgDumpError::ArithmeticOverflow { offset: start })
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
