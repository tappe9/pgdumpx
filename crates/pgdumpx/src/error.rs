use std::{error::Error, fmt, io};

/// Errors produced while reading PostgreSQL dump archives.
#[derive(Debug)]
#[non_exhaustive]
pub enum PgDumpError {
    /// An underlying I/O operation failed.
    Io { offset: u64, source: io::Error },
    /// The archive ended before the requested bytes were available.
    UnexpectedEof { offset: u64 },
    /// The archive records an integer width that this parser cannot represent.
    UnsupportedArchiveIntegerSize { size: u8, offset: u64 },
    /// An archive integer cannot be represented as an `i32`.
    ArchiveIntegerOutOfRange { offset: u64 },
    /// The archive records an invalid zero-width file offset.
    InvalidArchiveOffsetSize { size: u8, offset: u64 },
    /// The archive offset state byte is not recognized.
    InvalidArchiveOffsetState { state: u8, offset: u64 },
    /// An encoded file offset cannot be represented as a `u64`.
    ArchiveOffsetOutOfRange { offset: u64 },
    /// An archive string length exceeds the explicitly supplied finite bound.
    ArchiveStringLimitExceeded {
        length: u64,
        limit: u64,
        offset: u64,
    },
    /// Memory for a bounded archive string could not be reserved.
    ArchiveStringAllocationFailed { length: u64, offset: u64 },
    /// Checked arithmetic or conversion overflowed.
    ArithmeticOverflow { offset: u64 },
}

impl fmt::Display for PgDumpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { offset, source } => {
                write!(
                    formatter,
                    "I/O error at archive byte offset {offset}: {source}"
                )
            }
            Self::UnexpectedEof { offset } => {
                write!(
                    formatter,
                    "unexpected end of archive at byte offset {offset}"
                )
            }
            Self::UnsupportedArchiveIntegerSize { size, offset } => write!(
                formatter,
                "unsupported archive integer size {size} at byte offset {offset}"
            ),
            Self::ArchiveIntegerOutOfRange { offset } => write!(
                formatter,
                "archive integer at byte offset {offset} is outside the supported i32 range"
            ),
            Self::InvalidArchiveOffsetSize { size, offset } => write!(
                formatter,
                "invalid archive offset size {size} at byte offset {offset}"
            ),
            Self::InvalidArchiveOffsetState { state, offset } => write!(
                formatter,
                "invalid archive offset state {state} at byte offset {offset}"
            ),
            Self::ArchiveOffsetOutOfRange { offset } => write!(
                formatter,
                "archive offset cannot be represented as u64 at byte offset {offset}"
            ),
            Self::ArchiveStringLimitExceeded {
                length,
                limit,
                offset,
            } => write!(
                formatter,
                "archive string length {length} at byte offset {offset} exceeds limit {limit}"
            ),
            Self::ArchiveStringAllocationFailed { length, offset } => write!(
                formatter,
                "could not reserve {length} bytes for archive string at byte offset {offset}"
            ),
            Self::ArithmeticOverflow { offset } => {
                write!(
                    formatter,
                    "arithmetic overflow at archive byte offset {offset}"
                )
            }
        }
    }
}

impl Error for PgDumpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
