use crate::{Archive, DumpId, EntryDataReader, PgDumpError, error::into_io_error};
use std::{
    fmt,
    io::{self, Read, Seek, Write},
};

const COPY_BUFFER_BYTES: usize = 8 * 1024;

/// Optional decompressed-byte budget for one raw selected-entry read.
///
/// `None` means no raw-output budget is applied. Applications processing
/// untrusted input should normally configure an explicit finite value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryReadLimits {
    max_decompressed_bytes: Option<u64>,
}

impl EntryReadLimits {
    /// Returns raw-entry limits with no decompressed-byte budget.
    pub const fn unlimited() -> Self {
        Self {
            max_decompressed_bytes: None,
        }
    }

    /// Returns the optional maximum decompressed bytes exposed by the reader.
    pub const fn max_decompressed_bytes(self) -> Option<u64> {
        self.max_decompressed_bytes
    }

    /// Returns a configuration with a maximum decompressed-byte budget.
    #[must_use]
    pub const fn with_max_decompressed_bytes(mut self, value: u64) -> Self {
        self.max_decompressed_bytes = Some(value);
        self
    }
}

impl Default for EntryReadLimits {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// A streaming selected-entry reader that enforces a decompressed-byte budget.
///
/// If the configured limit is crossed, [`Read::read`] returns an `io::Error`
/// whose source is the typed [`PgDumpError::EntryDecompressedByteLimitExceeded`].
/// The error is distinct from normal EOF; bytes beyond the configured limit are
/// never returned to the caller.
pub struct BoundedEntryDataReader<'a, R> {
    dump_id: DumpId,
    inner: EntryDataReader<'a, R>,
    limits: EntryReadLimits,
    returned: u64,
    terminal_error: Option<TerminalError>,
}

#[derive(Debug, Clone, Copy)]
enum TerminalError {
    Limit { limit: u64, consumed: u64 },
    Overflow { consumed: u64, increment: u64 },
}

impl TerminalError {
    fn into_pg_error(self, dump_id: i32) -> PgDumpError {
        match self {
            Self::Limit { limit, consumed } => PgDumpError::EntryDecompressedByteLimitExceeded {
                dump_id,
                limit,
                consumed,
            },
            Self::Overflow {
                consumed,
                increment,
            } => PgDumpError::EntryDecompressedByteCountOverflow {
                dump_id,
                consumed,
                increment,
            },
        }
    }
}

impl<'a, R> BoundedEntryDataReader<'a, R> {
    fn new(dump_id: DumpId, inner: EntryDataReader<'a, R>, limits: EntryReadLimits) -> Self {
        Self {
            dump_id,
            inner,
            limits,
            returned: 0,
            terminal_error: None,
        }
    }

    fn fail(&mut self, error: TerminalError) -> io::Error {
        self.terminal_error = Some(error);
        into_io_error(error.into_pg_error(self.dump_id.as_i32()))
    }

    fn checked_count_after(&mut self, increment: usize) -> io::Result<u64> {
        let increment = match u64::try_from(increment) {
            Ok(value) => value,
            Err(_) => {
                return Err(self.fail(TerminalError::Overflow {
                    consumed: self.returned,
                    increment: u64::MAX,
                }));
            }
        };
        match checked_decompressed_count(self.dump_id.as_i32(), self.returned, increment) {
            Ok(value) => Ok(value),
            Err(_) => Err(self.fail(TerminalError::Overflow {
                consumed: self.returned,
                increment,
            })),
        }
    }
}

impl<R: Read> fmt::Debug for BoundedEntryDataReader<'_, R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedEntryDataReader")
            .field("dump_id", &self.dump_id)
            .field("inner", &self.inner)
            .field("limits", &self.limits)
            .field("returned", &self.returned)
            .finish_non_exhaustive()
    }
}

impl<R: Read> Read for BoundedEntryDataReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if let Some(error) = self.terminal_error {
            return Err(into_io_error(error.into_pg_error(self.dump_id.as_i32())));
        }

        let requested = match self.limits.max_decompressed_bytes() {
            None => output.len(),
            Some(limit) if self.returned < limit => {
                let remaining = limit - self.returned;
                output
                    .len()
                    .min(usize::try_from(remaining).unwrap_or(usize::MAX))
            }
            Some(limit) => {
                let mut probe = [0_u8; 1];
                match self.inner.read(&mut probe) {
                    Ok(0) => return Ok(0),
                    Ok(read) => {
                        let consumed = self.checked_count_after(read)?;
                        return Err(self.fail(TerminalError::Limit { limit, consumed }));
                    }
                    Err(error) => return Err(error),
                }
            }
        };

        let read = self.inner.read(&mut output[..requested])?;
        self.returned = self.checked_count_after(read)?;
        Ok(read)
    }
}

impl<R: Read + Seek> Archive<R> {
    /// Opens one validated entry as a streaming decompressed reader with an
    /// optional raw-output byte budget.
    pub fn entry_reader_with_limits(
        &mut self,
        id: DumpId,
        limits: EntryReadLimits,
    ) -> Result<Option<BoundedEntryDataReader<'_, R>>, PgDumpError> {
        self.entry_reader(id)
            .map(|reader| reader.map(|reader| BoundedEntryDataReader::new(id, reader, limits)))
    }

    /// Copies one selected entry's decompressed bytes to `writer` using the
    /// same bounded reader path exposed by [`Archive::entry_reader_with_limits`].
    ///
    /// Bytes already accepted by the destination cannot be rolled back if a
    /// later limit, input, decompression, or writer error occurs. Such partial
    /// output is nevertheless returned as an error, never as successful
    /// truncation.
    pub fn copy_entry_to<W: Write>(
        &mut self,
        id: DumpId,
        writer: &mut W,
        limits: EntryReadLimits,
    ) -> Result<u64, PgDumpError> {
        let Some(mut reader) = self.entry_reader_with_limits(id, limits)? else {
            return Err(PgDumpError::EntryNotFound {
                dump_id: id.as_i32(),
            });
        };
        let mut buffer = [0_u8; COPY_BUFFER_BYTES];
        let mut copied = 0_u64;

        loop {
            let read = reader.read(&mut buffer).map_err(map_entry_read_error)?;
            if read == 0 {
                return Ok(copied);
            }
            write_all_counted(writer, id, &buffer[..read], &mut copied)?;
        }
    }
}

fn write_all_counted<W: Write>(
    writer: &mut W,
    id: DumpId,
    mut bytes: &[u8],
    written: &mut u64,
) -> Result<(), PgDumpError> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => {
                return Err(PgDumpError::EntryOutputIo {
                    dump_id: id.as_i32(),
                    written: *written,
                    source: io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write the complete raw entry buffer",
                    ),
                });
            }
            Ok(count) => {
                let increment = u64::try_from(count).map_err(|_| {
                    PgDumpError::EntryDecompressedByteCountOverflow {
                        dump_id: id.as_i32(),
                        consumed: *written,
                        increment: u64::MAX,
                    }
                })?;
                *written = checked_decompressed_count(id.as_i32(), *written, increment)?;
                bytes = &bytes[count..];
            }
            Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(PgDumpError::EntryOutputIo {
                    dump_id: id.as_i32(),
                    written: *written,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn checked_decompressed_count(
    dump_id: i32,
    consumed: u64,
    increment: u64,
) -> Result<u64, PgDumpError> {
    consumed
        .checked_add(increment)
        .ok_or(PgDumpError::EntryDecompressedByteCountOverflow {
            dump_id,
            consumed,
            increment,
        })
}

fn map_entry_read_error(error: io::Error) -> PgDumpError {
    let kind = error.kind();
    let message = error.to_string();
    match error.into_inner() {
        Some(source) => match source.downcast::<PgDumpError>() {
            Ok(error) => *error,
            Err(source) => PgDumpError::Io {
                offset: 0,
                source: io::Error::new(kind, source),
            },
        },
        None => PgDumpError::Io {
            offset: 0,
            source: io::Error::new(kind, message),
        },
    }
}
