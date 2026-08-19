use std::{error::Error, fmt, io};

/// Errors produced while reading PostgreSQL dump archives.
#[derive(Debug)]
#[non_exhaustive]
pub enum PgDumpError {
    /// An underlying I/O operation failed.
    Io { offset: u64, source: io::Error },
    /// The archive ended before the requested bytes were available.
    UnexpectedEof { offset: u64 },
    /// The archive does not begin with PostgreSQL's custom-archive magic.
    InvalidArchiveMagic { offset: u64 },
    /// The archive version is outside the exact version implemented by this parser.
    UnsupportedArchiveVersion {
        major: u8,
        minor: u8,
        revision: u8,
        offset: u64,
    },
    /// The archive header names a format other than custom format.
    UnexpectedArchiveFormat { format: u8, offset: u64 },
    /// The archive header names an unknown compression algorithm.
    UnsupportedCompressionAlgorithm { algorithm: u8, offset: u64 },
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
    /// A required header or TOC string was encoded as NULL.
    MissingRequiredArchiveString { field: &'static str, offset: u64 },
    /// The encoded TOC entry count is negative.
    InvalidTocEntryCount { value: i32, offset: u64 },
    /// The encoded TOC entry count exceeds the provisional finite bound.
    TocEntryLimitExceeded { count: u64, limit: u64, offset: u64 },
    /// Memory for the bounded TOC vector could not be reserved.
    TocAllocationFailed { count: u64, offset: u64 },
    /// A TOC dump ID is not a positive `i32`.
    InvalidDumpId { value: i32, offset: u64 },
    /// A TOC section integer is not recognized.
    InvalidSection {
        value: i32,
        entry_id: i32,
        offset: u64,
    },
    /// A textual TOC dependency is not a positive decimal dump ID.
    InvalidDependencyEncoding { entry_id: i32, offset: u64 },
    /// One TOC entry has more dependencies than the provisional finite bound.
    DependencyLimitExceeded {
        entry_id: i32,
        count: u64,
        limit: u64,
        offset: u64,
    },
    /// Memory for a bounded TOC dependency vector could not be reserved.
    DependencyAllocationFailed {
        entry_id: i32,
        count: u64,
        offset: u64,
    },
    /// Two TOC entries use the same dump ID.
    DuplicateDumpId { dump_id: i32 },
    /// Memory for an archive metadata index could not be reserved.
    ArchiveIndexAllocationFailed {
        context: &'static str,
        requested: u64,
    },
    /// Two `TABLE` entries have the same byte-oriented identity.
    DuplicateTableIdentity {
        first_table_id: i32,
        second_table_id: i32,
    },
    /// One `TABLE DATA` entry depends on multiple `TABLE` entries.
    AmbiguousTableDataRelationship { data_id: i32 },
    /// A dependency relationship conflicts with catalog or object identity metadata.
    ConflictingTableDataRelationship { table_id: i32, data_id: i32 },
    /// Multiple `TABLE DATA` entries claim the same `TABLE` entry.
    DuplicateTableDataRelationship {
        table_id: i32,
        first_data_id: i32,
        second_data_id: i32,
    },
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
            Self::InvalidArchiveMagic { offset } => {
                write!(formatter, "invalid archive magic at byte offset {offset}")
            }
            Self::UnsupportedArchiveVersion {
                major,
                minor,
                revision,
                offset,
            } => write!(
                formatter,
                "unsupported archive version {major}.{minor}.{revision} at byte offset {offset}"
            ),
            Self::UnexpectedArchiveFormat { format, offset } => write!(
                formatter,
                "unexpected archive format {format} at byte offset {offset}"
            ),
            Self::UnsupportedCompressionAlgorithm { algorithm, offset } => write!(
                formatter,
                "unsupported compression algorithm {algorithm} at byte offset {offset}"
            ),
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
            Self::MissingRequiredArchiveString { field, offset } => write!(
                formatter,
                "required {field} is NULL at archive byte offset {offset}"
            ),
            Self::InvalidTocEntryCount { value, offset } => write!(
                formatter,
                "invalid TOC entry count {value} at archive byte offset {offset}"
            ),
            Self::TocEntryLimitExceeded {
                count,
                limit,
                offset,
            } => write!(
                formatter,
                "TOC entry count {count} at byte offset {offset} exceeds limit {limit}"
            ),
            Self::TocAllocationFailed { count, offset } => write!(
                formatter,
                "could not reserve metadata for {count} TOC entries at byte offset {offset}"
            ),
            Self::InvalidDumpId { value, offset } => write!(
                formatter,
                "invalid dump ID {value} at archive byte offset {offset}"
            ),
            Self::InvalidSection {
                value,
                entry_id,
                offset,
            } => write!(
                formatter,
                "invalid TOC section {value} for dump ID {entry_id} at byte offset {offset}"
            ),
            Self::InvalidDependencyEncoding { entry_id, offset } => write!(
                formatter,
                "invalid dependency encoding for dump ID {entry_id} at byte offset {offset}"
            ),
            Self::DependencyLimitExceeded {
                entry_id,
                count,
                limit,
                offset,
            } => write!(
                formatter,
                "dependency count {count} for dump ID {entry_id} at byte offset {offset} exceeds limit {limit}"
            ),
            Self::DependencyAllocationFailed {
                entry_id,
                count,
                offset,
            } => write!(
                formatter,
                "could not reserve {count} dependencies for dump ID {entry_id} at byte offset {offset}"
            ),
            Self::DuplicateDumpId { dump_id } => {
                write!(formatter, "duplicate dump ID {dump_id} in archive TOC")
            }
            Self::ArchiveIndexAllocationFailed { context, requested } => write!(
                formatter,
                "could not reserve {requested} elements or bytes for {context}"
            ),
            Self::DuplicateTableIdentity {
                first_table_id,
                second_table_id,
            } => write!(
                formatter,
                "TABLE dump IDs {first_table_id} and {second_table_id} have the same identity"
            ),
            Self::AmbiguousTableDataRelationship { data_id } => write!(
                formatter,
                "TABLE DATA dump ID {data_id} depends on multiple TABLE entries"
            ),
            Self::ConflictingTableDataRelationship { table_id, data_id } => write!(
                formatter,
                "TABLE dump ID {table_id} conflicts with TABLE DATA dump ID {data_id}"
            ),
            Self::DuplicateTableDataRelationship {
                table_id,
                first_data_id,
                second_data_id,
            } => write!(
                formatter,
                "TABLE dump ID {table_id} is claimed by TABLE DATA dump IDs {first_data_id} and {second_data_id}"
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
