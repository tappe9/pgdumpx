# pgdumpx Public API Design

Status: **Accepted direction for v0.1 implementation**

This document defines the intended public Rust API shape. Exact names may be refined during TDD, but changes to the core ownership, safety, and streaming contracts should be deliberate.

## 1. Design principles

The public API should:

- model a read-only archive;
- make metadata inspection cheap after open;
- keep payload access lazy;
- expose streaming `Read` where raw entry bytes are useful;
- expose row-aware COPY access without forcing UTF-8;
- keep the source owned by the archive so seeks are coordinated safely;
- use typed IDs and enums rather than stringly typed control flow;
- expose typed errors and resource limits;
- avoid leaking compression-library implementation types;
- remain suitable for later wrappers without designing around Python/Arrow today.

## 2. Opening an archive

Initial direction:

```rust
pub struct Archive<R> {
    // private
}

impl<R: Read + Seek> Archive<R> {
    pub fn open(reader: R) -> Result<Self, PgDumpError>;
    pub fn open_with_limits(reader: R, limits: Limits) -> Result<Self, PgDumpError>;
}
```

A path convenience constructor may be implemented for `Archive<BufReader<File>>`, but the reusable API is reader-based.

## 3. Limits

```rust
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_toc_entries: usize,
    pub max_string_bytes: usize,
    pub max_dependencies_per_entry: usize,
    pub max_row_bytes: usize,
    pub max_fields_per_row: usize,
}
```

`Default` provides compatibility-oriented finite limits. Applications processing hostile input should be able to select stricter limits.

An explicitly unbounded mode should not be the default merely for convenience.

## 4. Archive header

Representative model:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveHeader {
    pub version: ArchiveVersion,
    pub integer_size: u8,
    pub offset_size: u8,
    pub format: ArchiveFormat,
    pub compression: Compression,
    pub created_at: ArchiveTimestamp,
    pub database_name: ArchiveString,
    pub server_version: ArchiveString,
    pub dump_version: ArchiveString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveVersion {
    pub major: u8,
    pub minor: u8,
    pub revision: u8,
}
```

`ArchiveFormat` must reject non-custom input in v0.1 rather than exposing a fake supported variant.

## 5. Archive strings

Archive metadata strings should not require valid UTF-8 at the lowest level.

Representative direction:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveString(Vec<u8>);

impl ArchiveString {
    pub fn as_bytes(&self) -> &[u8];
    pub fn to_str(&self) -> Result<&str, Utf8Error>;
}
```

If compatibility evidence shows PostgreSQL guarantees a stronger encoding for a particular field, an ergonomic accessor may expose that fact without weakening the general parser.

## 6. Dump IDs and TOC entries

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DumpId(pub i32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub id: DumpId,
    pub has_data: bool,
    pub tag: ArchiveString,
    pub description: ArchiveString,
    pub section: Section,
    pub namespace: Option<ArchiveString>,
    pub owner: ArchiveString,
    pub dependencies: Vec<DumpId>,
    pub data_location: DataLocation,
    // additional supported metadata fields
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLocation {
    NoData,
    Unknown,
    Offset(u64),
}
```

The public model should preserve the upstream distinction between no data, position not recorded, and a valid stored offset.

## 7. Metadata access

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn header(&self) -> &ArchiveHeader;
    pub fn entries(&self) -> &[TocEntry];
    pub fn entry(&self, id: DumpId) -> Option<&TocEntry>;
    pub fn table(&self, schema: &[u8], name: &[u8]) -> Option<TableRef<'_>>;
}
```

A UTF-8 convenience overload may be provided, but byte-oriented lookup should remain possible.

## 8. Table reference

```rust
pub struct TableRef<'a> {
    // references metadata owned by ArchiveIndex
}

impl TableRef<'_> {
    pub fn schema(&self) -> Option<&[u8]>;
    pub fn name(&self) -> &[u8];
    pub fn table_entry_id(&self) -> DumpId;
    pub fn data_entry_id(&self) -> Option<DumpId>;
}
```

The type is a metadata handle only. It does not borrow entry payload bytes.

## 9. Raw entry data

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn entry_reader(
        &mut self,
        id: DumpId,
    ) -> Result<Option<EntryDataReader<'_, R>>, PgDumpError>;
}
```

`EntryDataReader` implements `std::io::Read` and yields **decompressed** entry data.

The mutable borrow of `Archive` intentionally prevents two readers from independently seeking the same underlying source at once. Future parallel file APIs should use separately opened/cloneable sources instead of weakening this invariant.

## 10. COPY rows

```rust
pub enum FieldRef<'a> {
    Null,
    Bytes(&'a [u8]),
}

pub struct Row<'a> {
    // borrowed from the row reader's current buffer
}

impl Row<'_> {
    pub fn len(&self) -> usize;
    pub fn field(&self, index: usize) -> Option<FieldRef<'_>>;
    pub fn fields(&self) -> impl ExactSizeIterator<Item = FieldRef<'_>>;
}
```

A row-reader direction:

```rust
pub struct CopyRowReader<R> {
    // private
}

impl<R: Read> CopyRowReader<R> {
    pub fn new(reader: R, limits: CopyLimits) -> Self;
    pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError>;
}
```

`Row` is valid only until the next mutable operation on the row reader. This enables reuse of the row buffer and avoids per-row ownership allocation.

A later owned-row convenience type may be added separately.

## 11. Table row convenience

The archive may expose a convenience method equivalent in purpose to:

```rust
pub fn table_rows(
    &mut self,
    schema: &[u8],
    table: &[u8],
) -> Result<TableRowReader<'_, R>, PgDumpError>;
```

`TableRowReader` composes:

```text
entry seek + block validation
        ↓
EntryDataReader
        ↓
CopyRowReader
```

This composition must not duplicate archive parsing logic.

## 12. Compression model

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Lz4,
    Zstd,
}
```

The enum describes the archive, not a dependency-specific decoder type.

Unsupported or invalid compression identifiers are errors.

## 13. Error API

Representative direction:

```rust
#[derive(Debug)]
#[non_exhaustive]
pub enum PgDumpError {
    Io(std::io::Error),
    UnexpectedEof { offset: u64 },
    InvalidMagic,
    UnsupportedArchiveVersion { version: ArchiveVersion },
    UnexpectedArchiveFormat { code: u8 },
    InvalidOffset { dump_id: Option<DumpId> },
    InvalidBlockType { offset: u64, block_type: u8 },
    DumpIdMismatch { offset: u64, expected: DumpId, actual: DumpId },
    UnsupportedCompression { code: i32 },
    Decompression { algorithm: Compression },
    MalformedCopy { row: u64, byte_offset: u64 },
    ResourceLimitExceeded { resource: ResourceLimit, limit: usize },
    ArithmeticOverflow { offset: Option<u64> },
    InvalidUtf8,
}
```

Exact fields should evolve from test requirements, but callers must not need to parse `Display` strings to determine error category.

## 14. Serialization

Serde support is not required for the parser to function.

If exposed, it should be optional:

```toml
[features]
default = []
serde = ["dep:serde"]
```

The CLI may use a presentation DTO instead of freezing every internal metadata field as a JSON compatibility promise.

## 15. Threading and parallel access

`Archive<R>` itself does not promise concurrent reads from one seekable source.

Future parallel extraction should use APIs that can produce independent sources, for example reopening a file path or accepting a source factory. This avoids mutex-protected seek thrashing and preserves simple borrowing semantics.

## 16. Versioning policy

Before v1.0:

- the public API may evolve;
- breaking changes must be intentional and documented once releases begin;
- private parser internals remain private;
- archive compatibility is version-explicit;
- accepted policy changes are recorded through ADRs.

## 17. Deferred APIs

Intentionally deferred:

```text
archive writing
non-seekable sequential archive reader
parallel extraction API
binary COPY decoding
Arrow/Polars/Parquet conversion
Python bindings
recovery from corrupt archives
```
