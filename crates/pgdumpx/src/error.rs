use std::{error::Error, fmt, io};

/// Errors produced while reading PostgreSQL dump archives.
#[derive(Debug)]
#[non_exhaustive]
pub enum PgDumpError {
    /// An underlying archive I/O operation failed.
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
    /// The selected TOC entry explicitly has no data block.
    EntryHasNoData { dump_id: i32 },
    /// The selected TOC entry has no recorded direct-seek position.
    EntryDataOffsetUnavailable { dump_id: i32 },
    /// The selected TOC entry's recorded offset cannot safely address its block header.
    InvalidDataOffset { dump_id: i32, offset: u64 },
    /// Seeking to a selected entry did not land at the requested absolute position.
    EntrySeekPositionMismatch {
        dump_id: i32,
        expected: u64,
        actual: u64,
    },
    /// The selected entry offset points to a custom block of the wrong type.
    UnexpectedDataBlockType {
        dump_id: i32,
        expected: u8,
        actual: u8,
        offset: u64,
    },
    /// The data block header contains a dump ID other than the selected entry's ID.
    DataBlockDumpIdMismatch {
        expected: i32,
        actual: i32,
        offset: u64,
    },
    /// A custom data chunk has a negative encoded length.
    InvalidDataChunkLength {
        dump_id: i32,
        length: i32,
        offset: u64,
    },
    /// The archive ended while reading a custom data chunk length.
    TruncatedDataChunkLength { dump_id: i32, offset: u64 },
    /// The archive ended before the current custom data chunk was complete.
    TruncatedDataChunk {
        dump_id: i32,
        remaining: u64,
        offset: u64,
    },
    /// The entry uses a recognized compression mode not implemented in this slice.
    UnsupportedEntryCompression {
        dump_id: i32,
        algorithm: &'static str,
    },
    /// Memory for a bounded selected-entry buffer could not be reserved.
    EntryBufferAllocationFailed { dump_id: i32, requested: u64 },
    /// A validated entry's compressed stream could not be decoded.
    DecompressionFailed {
        dump_id: i32,
        algorithm: &'static str,
        source: io::Error,
    },
    /// An underlying decompressed COPY stream I/O operation failed.
    CopyIo {
        row: u64,
        consumed: u64,
        source: io::Error,
    },
    /// A physical COPY row ends with an incomplete backslash escape.
    MalformedCopyEscape { row: u64, byte_offset: u64 },
    /// A COPY end marker is truncated, embedded in a row, or not alone on its line.
    MalformedCopyTerminator { row: u64, byte_offset: u64 },
    /// One physical COPY row exceeds the provisional finite row-byte bound.
    CopyRowByteLimitExceeded {
        row: u64,
        limit: u64,
        actual: u64,
        byte_offset: u64,
    },
    /// One COPY row exceeds the provisional finite field-count bound.
    CopyFieldCountLimitExceeded {
        row: u64,
        limit: u64,
        actual: u64,
        byte_offset: u64,
    },
    /// Memory for bounded current-row byte storage could not be reserved.
    CopyRowAllocationFailed { row: u64, requested: u64 },
    /// Memory for bounded current-row field metadata could not be reserved.
    CopyFieldAllocationFailed { row: u64, requested: u64 },
    /// Checked parser-consumed COPY byte accounting overflowed.
    CopyConsumedByteCountOverflow {
        row: u64,
        consumed: u64,
        increment: u64,
    },
    /// Checked COPY row-number accounting overflowed.
    CopyRowNumberOverflow { row: u64 },
    /// Checked arithmetic or conversion overflowed.
    ArithmeticOverflow { offset: u64 },
}

impl fmt::Display for PgDumpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { offset, source } => {
                write!(formatter, "I/O error at archive byte offset {offset}: {source}")
            }
            Self::UnexpectedEof { offset } => {
                write!(formatter, "unexpected end of archive at byte offset {offset}")
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
            Self::EntryHasNoData { dump_id } => {
                write!(formatter, "TOC dump ID {dump_id} has no data block")
            }
            Self::EntryDataOffsetUnavailable { dump_id } => write!(
                formatter,
                "TOC dump ID {dump_id} has no recorded direct-seek data offset"
            ),
            Self::InvalidDataOffset { dump_id, offset } => write!(
                formatter,
                "data offset {offset} for TOC dump ID {dump_id} cannot safely address a custom block header"
            ),
            Self::EntrySeekPositionMismatch {
                dump_id,
                expected,
                actual,
            } => write!(
                formatter,
                "seek for TOC dump ID {dump_id} requested byte offset {expected} but landed at {actual}"
            ),
            Self::UnexpectedDataBlockType {
                dump_id,
                expected,
                actual,
                offset,
            } => write!(
                formatter,
                "TOC dump ID {dump_id} expected custom block type {expected} at byte offset {offset}, found {actual}"
            ),
            Self::DataBlockDumpIdMismatch {
                expected,
                actual,
                offset,
            } => write!(
                formatter,
                "selected dump ID {expected} points to data block dump ID {actual} at byte offset {offset}"
            ),
            Self::InvalidDataChunkLength {
                dump_id,
                length,
                offset,
            } => write!(
                formatter,
                "data chunk for dump ID {dump_id} has invalid length {length} at byte offset {offset}"
            ),
            Self::TruncatedDataChunkLength { dump_id, offset } => write!(
                formatter,
                "archive ended while reading a data chunk length for dump ID {dump_id} at byte offset {offset}"
            ),
            Self::TruncatedDataChunk {
                dump_id,
                remaining,
                offset,
            } => write!(
                formatter,
                "archive ended at byte offset {offset} with {remaining} bytes remaining in a data chunk for dump ID {dump_id}"
            ),
            Self::UnsupportedEntryCompression { dump_id, algorithm } => write!(
                formatter,
                "dump ID {dump_id} uses unsupported {algorithm} entry compression"
            ),
            Self::EntryBufferAllocationFailed { dump_id, requested } => write!(
                formatter,
                "could not reserve {requested} bytes for dump ID {dump_id} entry buffering"
            ),
            Self::DecompressionFailed {
                dump_id,
                algorithm,
                source,
            } => write!(
                formatter,
                "could not decompress dump ID {dump_id} with {algorithm}: {source}"
            ),
            Self::CopyIo {
                row,
                consumed,
                source,
            } => write!(
                formatter,
                "I/O error while parsing COPY row {row} after consuming {consumed} bytes: {source}"
            ),
            Self::MalformedCopyEscape { row, byte_offset } => write!(
                formatter,
                "incomplete COPY escape in row {row} at decompressed byte offset {byte_offset}"
            ),
            Self::MalformedCopyTerminator { row, byte_offset } => write!(
                formatter,
                "malformed COPY end marker in row {row} at decompressed byte offset {byte_offset}"
            ),
            Self::CopyRowByteLimitExceeded {
                row,
                limit,
                actual,
                byte_offset,
            } => write!(
                formatter,
                "COPY row {row} reached {actual} physical bytes at offset {byte_offset}, exceeding limit {limit}"
            ),
            Self::CopyFieldCountLimitExceeded {
                row,
                limit,
                actual,
                byte_offset,
            } => write!(
                formatter,
                "COPY row {row} reached {actual} fields at offset {byte_offset}, exceeding limit {limit}"
            ),
            Self::CopyRowAllocationFailed { row, requested } => write!(
                formatter,
                "could not reserve {requested} bytes for COPY row {row}"
            ),
            Self::CopyFieldAllocationFailed { row, requested } => write!(
                formatter,
                "could not reserve {requested} field descriptors for COPY row {row}"
            ),
            Self::CopyConsumedByteCountOverflow {
                row,
                consumed,
                increment,
            } => write!(
                formatter,
                "COPY consumed-byte counter overflow in row {row}: {consumed} + {increment}"
            ),
            Self::CopyRowNumberOverflow { row } => {
                write!(formatter, "COPY row-number counter overflow after row {row}")
            }
            Self::ArithmeticOverflow { offset } => {
                write!(formatter, "arithmetic overflow at archive byte offset {offset}")
            }
        }
    }
}

impl Error for PgDumpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. }
            | Self::DecompressionFailed { source, .. }
            | Self::CopyIo { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(crate) fn into_io_error(error: PgDumpError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
