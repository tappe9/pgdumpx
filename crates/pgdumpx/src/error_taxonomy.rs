use crate::{Compression, DumpId, error::PgDumpError};

/// Stable high-level categories for programmatic error handling.
///
/// Callers can match this taxonomy without depending on human-readable
/// [`std::fmt::Display`] text or every fine-grained [`PgDumpError`] variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorCategory {
    /// An underlying input or output operation failed.
    Io,
    /// Archive bytes do not conform to the supported custom-format grammar.
    Format,
    /// Parsed archive metadata or selected-entry identity is inconsistent.
    Integrity,
    /// A selected entry uses unsupported compression or failed to decompress.
    Decompression,
    /// COPY metadata or row bytes are malformed or unavailable.
    Copy,
    /// A row-aware operation encountered an unsupported data representation.
    Representation,
    /// Explicit byte-to-text conversion failed.
    Encoding,
    /// A configured bound or bounded allocation was exhausted.
    Resource,
    /// Checked arithmetic or counter accounting failed.
    Arithmetic,
    /// A requested table or data entry is not available.
    Lookup,
}

/// A typed resource protected by a finite limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceLimit {
    /// Encoded bytes in one archive metadata string.
    ArchiveStringBytes,
    /// Number of archive table-of-contents entries.
    TocEntries,
    /// Number of dependencies attached to one TOC entry.
    DependenciesPerEntry,
    /// Number of columns in COPY statement metadata.
    CopyColumns,
    /// Physical bytes in one COPY text row.
    CopyRowBytes,
    /// Number of fields in one COPY row.
    CopyFieldsPerRow,
    /// Complete rows consumed by one scan operation.
    ScanRows,
    /// Decompressed bytes consumed by the COPY parser during one scan.
    ScanDecompressedBytes,
    /// Decompressed bytes returned by a bounded raw-entry reader.
    EntryDecompressedBytes,
}

/// Typed limit, configured bound, and consumed/observed work for one failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitContext {
    resource: ResourceLimit,
    limit: u64,
    consumed: u64,
}

impl LimitContext {
    pub(crate) const fn new(resource: ResourceLimit, limit: u64, consumed: u64) -> Self {
        Self {
            resource,
            limit,
            consumed,
        }
    }

    /// Returns the bounded resource that was exhausted.
    pub const fn resource(self) -> ResourceLimit {
        self.resource
    }

    /// Returns the configured inclusive maximum.
    pub const fn limit(self) -> u64 {
        self.limit
    }

    /// Returns the observed or attempted amount that crossed the limit.
    pub const fn consumed(self) -> u64 {
        self.consumed
    }
}

impl PgDumpError {
    /// Returns a stable high-level error category.
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Io { .. } | Self::EntryOutputIo { .. } | Self::CopyIo { .. } => ErrorCategory::Io,
            Self::UnexpectedEof { .. }
            | Self::InvalidArchiveMagic { .. }
            | Self::UnsupportedArchiveVersion { .. }
            | Self::UnexpectedArchiveFormat { .. }
            | Self::UnsupportedCompressionAlgorithm { .. }
            | Self::UnsupportedArchiveIntegerSize { .. }
            | Self::ArchiveIntegerOutOfRange { .. }
            | Self::InvalidArchiveOffsetSize { .. }
            | Self::InvalidArchiveOffsetState { .. }
            | Self::ArchiveOffsetOutOfRange { .. }
            | Self::MissingRequiredArchiveString { .. }
            | Self::InvalidTocEntryCount { .. }
            | Self::InvalidDumpId { .. }
            | Self::InvalidSection { .. }
            | Self::InvalidDependencyEncoding { .. } => ErrorCategory::Format,
            Self::DuplicateDumpId { .. }
            | Self::DuplicateTableIdentity { .. }
            | Self::AmbiguousTableDataRelationship { .. }
            | Self::ConflictingTableDataRelationship { .. }
            | Self::DuplicateTableDataRelationship { .. }
            | Self::InvalidDataOffset { .. }
            | Self::EntrySeekPositionMismatch { .. }
            | Self::UnexpectedDataBlockType { .. }
            | Self::DataBlockDumpIdMismatch { .. }
            | Self::InvalidDataChunkLength { .. }
            | Self::TruncatedDataChunkLength { .. }
            | Self::TruncatedDataChunk { .. } => ErrorCategory::Integrity,
            Self::UnsupportedEntryCompression { .. } | Self::DecompressionFailed { .. } => {
                ErrorCategory::Decompression
            }
            Self::CopyColumnMetadataUnavailable { .. }
            | Self::MalformedCopyStatement { .. }
            | Self::MalformedCopyEscape { .. }
            | Self::MalformedCopyTerminator { .. } => ErrorCategory::Copy,
            Self::UnsupportedTableDataRepresentation { .. } => ErrorCategory::Representation,
            Self::InvalidUtf8 { .. } => ErrorCategory::Encoding,
            Self::ArchiveStringLimitExceeded { .. }
            | Self::ArchiveStringAllocationFailed { .. }
            | Self::TocEntryLimitExceeded { .. }
            | Self::TocAllocationFailed { .. }
            | Self::DependencyLimitExceeded { .. }
            | Self::DependencyAllocationFailed { .. }
            | Self::ArchiveIndexAllocationFailed { .. }
            | Self::CopyColumnCountLimitExceeded { .. }
            | Self::CopyColumnMetadataAllocationFailed { .. }
            | Self::EntryBufferAllocationFailed { .. }
            | Self::EntryDecompressedByteLimitExceeded { .. }
            | Self::CopyRowByteLimitExceeded { .. }
            | Self::CopyFieldCountLimitExceeded { .. }
            | Self::ScanRowLimitExceeded { .. }
            | Self::ScanDecompressedByteLimitExceeded { .. }
            | Self::CopyRowAllocationFailed { .. }
            | Self::CopyFieldAllocationFailed { .. } => ErrorCategory::Resource,
            Self::EntryDecompressedByteCountOverflow { .. }
            | Self::CopyConsumedByteCountOverflow { .. }
            | Self::CopyRowNumberOverflow { .. }
            | Self::ScanRowCountOverflow { .. }
            | Self::ArithmeticOverflow { .. } => ErrorCategory::Arithmetic,
            Self::TableNotFound
            | Self::TableDataEntryUnavailable { .. }
            | Self::EntryNotFound { .. }
            | Self::EntryHasNoData { .. }
            | Self::EntryDataOffsetUnavailable { .. } => ErrorCategory::Lookup,
        }
    }

    /// Returns the most relevant archive or decompressed byte offset, when present.
    pub const fn byte_offset(&self) -> Option<u64> {
        match self {
            Self::Io { offset, .. }
            | Self::UnexpectedEof { offset }
            | Self::InvalidArchiveMagic { offset }
            | Self::UnsupportedArchiveVersion { offset, .. }
            | Self::UnexpectedArchiveFormat { offset, .. }
            | Self::UnsupportedCompressionAlgorithm { offset, .. }
            | Self::UnsupportedArchiveIntegerSize { offset, .. }
            | Self::ArchiveIntegerOutOfRange { offset }
            | Self::InvalidArchiveOffsetSize { offset, .. }
            | Self::InvalidArchiveOffsetState { offset, .. }
            | Self::ArchiveOffsetOutOfRange { offset }
            | Self::ArchiveStringLimitExceeded { offset, .. }
            | Self::ArchiveStringAllocationFailed { offset, .. }
            | Self::MissingRequiredArchiveString { offset, .. }
            | Self::InvalidTocEntryCount { offset, .. }
            | Self::TocEntryLimitExceeded { offset, .. }
            | Self::TocAllocationFailed { offset, .. }
            | Self::InvalidDumpId { offset, .. }
            | Self::InvalidSection { offset, .. }
            | Self::InvalidDependencyEncoding { offset, .. }
            | Self::DependencyLimitExceeded { offset, .. }
            | Self::DependencyAllocationFailed { offset, .. }
            | Self::InvalidDataOffset { offset, .. }
            | Self::UnexpectedDataBlockType { offset, .. }
            | Self::DataBlockDumpIdMismatch { offset, .. }
            | Self::InvalidDataChunkLength { offset, .. }
            | Self::TruncatedDataChunkLength { offset, .. }
            | Self::TruncatedDataChunk { offset, .. }
            | Self::MalformedCopyEscape {
                byte_offset: offset,
                ..
            }
            | Self::MalformedCopyTerminator {
                byte_offset: offset,
                ..
            }
            | Self::CopyRowByteLimitExceeded {
                byte_offset: offset,
                ..
            }
            | Self::CopyFieldCountLimitExceeded {
                byte_offset: offset,
                ..
            }
            | Self::ScanDecompressedByteLimitExceeded {
                byte_offset: offset,
                ..
            }
            | Self::ArithmeticOverflow { offset } => Some(*offset),
            Self::EntrySeekPositionMismatch { expected, .. } => Some(*expected),
            Self::EntryOutputIo { written, .. } => Some(*written),
            Self::CopyIo { consumed, .. } => Some(*consumed),
            Self::DuplicateDumpId { .. }
            | Self::ArchiveIndexAllocationFailed { .. }
            | Self::DuplicateTableIdentity { .. }
            | Self::AmbiguousTableDataRelationship { .. }
            | Self::ConflictingTableDataRelationship { .. }
            | Self::DuplicateTableDataRelationship { .. }
            | Self::TableNotFound
            | Self::TableDataEntryUnavailable { .. }
            | Self::CopyColumnMetadataUnavailable { .. }
            | Self::MalformedCopyStatement { .. }
            | Self::UnsupportedTableDataRepresentation { .. }
            | Self::CopyColumnCountLimitExceeded { .. }
            | Self::CopyColumnMetadataAllocationFailed { .. }
            | Self::EntryNotFound { .. }
            | Self::EntryHasNoData { .. }
            | Self::EntryDataOffsetUnavailable { .. }
            | Self::UnsupportedEntryCompression { .. }
            | Self::EntryBufferAllocationFailed { .. }
            | Self::DecompressionFailed { .. }
            | Self::EntryDecompressedByteLimitExceeded { .. }
            | Self::EntryDecompressedByteCountOverflow { .. }
            | Self::ScanRowLimitExceeded { .. }
            | Self::CopyRowAllocationFailed { .. }
            | Self::CopyFieldAllocationFailed { .. }
            | Self::CopyConsumedByteCountOverflow { .. }
            | Self::CopyRowNumberOverflow { .. }
            | Self::ScanRowCountOverflow { .. }
            | Self::InvalidUtf8 { .. } => None,
        }
    }

    /// Returns the primary valid dump ID associated with the failure, when present.
    pub const fn dump_id(&self) -> Option<DumpId> {
        let value = match self {
            Self::InvalidSection { entry_id, .. }
            | Self::InvalidDependencyEncoding { entry_id, .. }
            | Self::DependencyLimitExceeded { entry_id, .. }
            | Self::DependencyAllocationFailed { entry_id, .. } => *entry_id,
            Self::DuplicateDumpId { dump_id }
            | Self::CopyColumnMetadataUnavailable { dump_id }
            | Self::MalformedCopyStatement { dump_id, .. }
            | Self::UnsupportedTableDataRepresentation { dump_id, .. }
            | Self::CopyColumnCountLimitExceeded { dump_id, .. }
            | Self::CopyColumnMetadataAllocationFailed { dump_id, .. }
            | Self::EntryNotFound { dump_id }
            | Self::EntryHasNoData { dump_id }
            | Self::EntryDataOffsetUnavailable { dump_id }
            | Self::InvalidDataOffset { dump_id, .. }
            | Self::EntrySeekPositionMismatch { dump_id, .. }
            | Self::UnexpectedDataBlockType { dump_id, .. }
            | Self::InvalidDataChunkLength { dump_id, .. }
            | Self::TruncatedDataChunkLength { dump_id, .. }
            | Self::TruncatedDataChunk { dump_id, .. }
            | Self::UnsupportedEntryCompression { dump_id, .. }
            | Self::EntryBufferAllocationFailed { dump_id, .. }
            | Self::DecompressionFailed { dump_id, .. }
            | Self::EntryDecompressedByteLimitExceeded { dump_id, .. }
            | Self::EntryDecompressedByteCountOverflow { dump_id, .. }
            | Self::EntryOutputIo { dump_id, .. } => *dump_id,
            Self::DuplicateTableIdentity { first_table_id, .. } => *first_table_id,
            Self::AmbiguousTableDataRelationship { data_id }
            | Self::ConflictingTableDataRelationship { data_id, .. } => *data_id,
            Self::DuplicateTableDataRelationship { table_id, .. }
            | Self::TableDataEntryUnavailable { table_id } => *table_id,
            Self::DataBlockDumpIdMismatch { expected, .. } => *expected,
            Self::Io { .. }
            | Self::UnexpectedEof { .. }
            | Self::InvalidArchiveMagic { .. }
            | Self::UnsupportedArchiveVersion { .. }
            | Self::UnexpectedArchiveFormat { .. }
            | Self::UnsupportedCompressionAlgorithm { .. }
            | Self::UnsupportedArchiveIntegerSize { .. }
            | Self::ArchiveIntegerOutOfRange { .. }
            | Self::InvalidArchiveOffsetSize { .. }
            | Self::InvalidArchiveOffsetState { .. }
            | Self::ArchiveOffsetOutOfRange { .. }
            | Self::ArchiveStringLimitExceeded { .. }
            | Self::ArchiveStringAllocationFailed { .. }
            | Self::MissingRequiredArchiveString { .. }
            | Self::InvalidTocEntryCount { .. }
            | Self::TocEntryLimitExceeded { .. }
            | Self::TocAllocationFailed { .. }
            | Self::InvalidDumpId { .. }
            | Self::ArchiveIndexAllocationFailed { .. }
            | Self::TableNotFound
            | Self::CopyIo { .. }
            | Self::MalformedCopyEscape { .. }
            | Self::MalformedCopyTerminator { .. }
            | Self::CopyRowByteLimitExceeded { .. }
            | Self::CopyFieldCountLimitExceeded { .. }
            | Self::ScanRowLimitExceeded { .. }
            | Self::ScanDecompressedByteLimitExceeded { .. }
            | Self::CopyRowAllocationFailed { .. }
            | Self::CopyFieldAllocationFailed { .. }
            | Self::CopyConsumedByteCountOverflow { .. }
            | Self::CopyRowNumberOverflow { .. }
            | Self::ScanRowCountOverflow { .. }
            | Self::ArithmeticOverflow { .. }
            | Self::InvalidUtf8 { .. } => return None,
        };
        if value > 0 {
            Some(DumpId::from_valid(value))
        } else {
            None
        }
    }

    /// Returns the COPY row number associated with the failure, when present.
    pub const fn row_number(&self) -> Option<u64> {
        match self {
            Self::CopyIo { row, .. }
            | Self::MalformedCopyEscape { row, .. }
            | Self::MalformedCopyTerminator { row, .. }
            | Self::CopyRowByteLimitExceeded { row, .. }
            | Self::CopyFieldCountLimitExceeded { row, .. }
            | Self::ScanRowLimitExceeded { row, .. }
            | Self::ScanDecompressedByteLimitExceeded { row, .. }
            | Self::CopyRowAllocationFailed { row, .. }
            | Self::CopyFieldAllocationFailed { row, .. }
            | Self::CopyConsumedByteCountOverflow { row, .. }
            | Self::CopyRowNumberOverflow { row }
            | Self::ScanRowCountOverflow { row, .. } => Some(*row),
            _ => None,
        }
    }

    /// Returns a known compression algorithm associated with the failure.
    pub fn compression(&self) -> Option<Compression> {
        let algorithm = match self {
            Self::UnsupportedEntryCompression { algorithm, .. }
            | Self::DecompressionFailed { algorithm, .. } => *algorithm,
            _ => return None,
        };
        match algorithm {
            "none" => Some(Compression::None),
            "gzip" => Some(Compression::Gzip),
            "lz4" => Some(Compression::Lz4),
            "zstandard" | "zstd" => Some(Compression::Zstd),
            _ => None,
        }
    }

    /// Returns typed finite-limit context for limit-exhaustion variants.
    pub const fn limit_context(&self) -> Option<LimitContext> {
        match self {
            Self::ArchiveStringLimitExceeded { length, limit, .. } => Some(LimitContext::new(
                ResourceLimit::ArchiveStringBytes,
                *limit,
                *length,
            )),
            Self::TocEntryLimitExceeded { count, limit, .. } => {
                Some(LimitContext::new(ResourceLimit::TocEntries, *limit, *count))
            }
            Self::DependencyLimitExceeded { count, limit, .. } => Some(LimitContext::new(
                ResourceLimit::DependenciesPerEntry,
                *limit,
                *count,
            )),
            Self::CopyColumnCountLimitExceeded { limit, actual, .. } => Some(LimitContext::new(
                ResourceLimit::CopyColumns,
                *limit,
                *actual,
            )),
            Self::CopyRowByteLimitExceeded { limit, actual, .. } => Some(LimitContext::new(
                ResourceLimit::CopyRowBytes,
                *limit,
                *actual,
            )),
            Self::CopyFieldCountLimitExceeded { limit, actual, .. } => Some(LimitContext::new(
                ResourceLimit::CopyFieldsPerRow,
                *limit,
                *actual,
            )),
            Self::ScanRowLimitExceeded {
                limit, consumed, ..
            } => Some(LimitContext::new(
                ResourceLimit::ScanRows,
                *limit,
                *consumed,
            )),
            Self::ScanDecompressedByteLimitExceeded {
                limit, consumed, ..
            } => Some(LimitContext::new(
                ResourceLimit::ScanDecompressedBytes,
                *limit,
                *consumed,
            )),
            Self::EntryDecompressedByteLimitExceeded {
                limit, consumed, ..
            } => Some(LimitContext::new(
                ResourceLimit::EntryDecompressedBytes,
                *limit,
                *consumed,
            )),
            _ => None,
        }
    }
}
