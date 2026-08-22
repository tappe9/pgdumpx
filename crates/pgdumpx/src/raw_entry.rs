use crate::{Archive, DumpId, EntryDataReader, PgDumpError, error::into_io_error};
use std::{
    fmt,
    io::{self, Read, Seek, Write},
};

const COPY_BUFFER_BYTES: usize = 8 * 1024;

/// Optional decompressed-byte budget for one raw selected-entry read.
///
/// Raw-output limits are separate from [`crate::Limits`] (archive/row structure)
/// and [`crate::ScanLimits`] (COPY row-scan work). This budget counts decompressed
/// bytes exposed by [`BoundedEntryDataReader`] or copied by [`Archive::copy_entry_to`].
///
/// A configured value is an inclusive maximum. If an entry ends after exactly `N`
/// bytes, a limit of `N` succeeds. If byte `N + 1` exists, the next read fails with
/// [`PgDumpError::EntryDecompressedByteLimitExceeded`]; the extra byte is never
/// returned to the caller. [`EntryReadLimits::unlimited`] and [`Default`] deliberately
/// apply no library-side raw-output budget. The `pgdumpx extract` CLI chooses a
/// separate finite default of 1 GiB when its option is omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryReadLimits {
    max_decompressed_bytes: Option<u64>,
}

impl EntryReadLimits {
    /// Returns raw-entry limits with no decompressed-byte budget.
    ///
    /// This is appropriate only when the caller intentionally accepts unbounded raw
    /// decompression work, for example for a trusted local archive.
    pub const fn unlimited() -> Self {
        Self {
            max_decompressed_bytes: None,
        }
    }

    /// Returns the inclusive maximum decompressed bytes exposed, if configured.
    pub const fn max_decompressed_bytes(self) -> Option<u64> {
        self.max_decompressed_bytes
    }

    /// Returns a configuration with an inclusive decompressed-byte budget.
    ///
    /// A value of `N` permits exactly `N` bytes if the selected entry ends there.
    /// Discovering byte `N + 1` produces a typed resource-limit error rather than
    /// successful truncation.
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

/// A streaming selected-entry reader with an optional decompressed-byte budget.
///
/// The inner [`EntryDataReader`] has already validated the selected entry and performs
/// streaming decompression. This wrapper counts only bytes exposed through its [`Read`]
/// implementation. If the configured limit is crossed, `Read::read` returns an
/// [`io::Error`] whose source is the typed
/// [`PgDumpError::EntryDecompressedByteLimitExceeded`]. The error is terminal and
/// remains distinct from normal EOF; bytes beyond the configured limit are never
/// returned.
///
/// Low-level callers that need the typed error can walk [`std::error::Error::source`]
/// or use the higher-level [`Archive::copy_entry_to`], which maps the embedded
/// `PgDumpError` back to the library error type.
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
    /// Opens one validated entry as a bounded streaming decompressed reader.
    ///
    /// Entry lookup, recorded-offset seek, custom block type, and dump-ID identity use
    /// the same validation path as [`Archive::entry_reader`]. `Ok(None)` means the dump
    /// ID is absent from the TOC. If present, `limits` applies only to decompressed bytes
    /// exposed by the returned reader; structural and row-scan limits are separate.
    ///
    /// Limit exhaustion is surfaced through [`Read`] as an `io::Error` with a typed
    /// [`PgDumpError`] source. Crossing the limit never becomes clean EOF or successful
    /// truncation.
    pub fn entry_reader_with_limits(
        &mut self,
        id: DumpId,
        limits: EntryReadLimits,
    ) -> Result<Option<BoundedEntryDataReader<'_, R>>, PgDumpError> {
        self.entry_reader(id)
            .map(|reader| reader.map(|reader| BoundedEntryDataReader::new(id, reader, limits)))
    }

    /// Copies one selected entry's decompressed bytes to `writer` with a raw-output budget.
    ///
    /// This uses [`Archive::entry_reader_with_limits`] rather than a separate accounting
    /// path. The copy is streaming and binary-safe. The returned `u64` is the number of
    /// bytes successfully accepted by `writer` only when the complete entry finishes.
    /// A missing dump ID is [`PgDumpError::EntryNotFound`].
    ///
    /// # Partial output on failure
    ///
    /// Bytes already accepted by `writer` cannot be rolled back if a later limit,
    /// archive-input, decompression, counter, or writer error occurs. Such an operation
    /// returns `Err`; partial output is never reported as successful extraction. For
    /// [`PgDumpError::EntryOutputIo`], the error records the number of bytes accepted by
    /// the writer before the failure.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pgdumpx::{Archive, EntryReadLimits};
    /// use std::{fs::File, io::{BufReader, BufWriter}};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let source = File::open("backup.dump")?;
    /// let mut archive = Archive::open(BufReader::new(source))?;
    /// let table = archive.table(b"public", b"events").expect("table metadata");
    /// let data_id = table.data_entry_id().expect("table-data entry");
    /// let limits = EntryReadLimits::unlimited().with_max_decompressed_bytes(64 * 1024 * 1024);
    /// let output = File::create("events.copy")?;
    /// let mut output = BufWriter::new(output);
    /// let copied = archive.copy_entry_to(data_id, &mut output, limits)?;
    /// assert!(copied <= 64 * 1024 * 1024);
    /// # Ok(())
    /// # }
    /// ```
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
