# pgdumpx Public API Design

Status: **Accepted direction for v0.1 implementation**

This document defines the intended public Rust API shape. Exact names may be refined during TDD, but changes to the core ownership, safety, streaming, row-search, and resource-budget contracts should be deliberate.

## 1. Design principles

The public API should:

- model a read-only PostgreSQL custom archive;
- make metadata inspection cheap after open;
- keep payload access lazy;
- expose streaming `Read` where raw entry bytes are useful;
- expose row-aware COPY access without forcing UTF-8;
- support column-aware first-match filtering without a SQL parser;
- distinguish a missing requested column from unavailable/malformed column metadata;
- make the sequential-scan cost of row lookup explicit;
- provide a caller-visible way to bound total scan/decompression work;
- reject unsupported table-data representations explicitly rather than guessing COPY input;
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

## 3. Structural limits

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

An explicitly unbounded structural mode should not be the default merely for convenience.

These limits protect individual parser allocations and metadata cardinalities. They do not by themselves bound the total CPU/decompression work of scanning millions of otherwise-small rows.

## 4. Scan work limits

Long-running row operations accept operation-level work budgets equivalent in purpose to:

```rust
#[derive(Debug, Clone)]
pub struct ScanLimits {
    pub max_rows: Option<u64>,
    pub max_decompressed_bytes: Option<u64>,
}
```

Exact names, constructors, and default values may evolve from TDD and real fixture sizes.

The design requirement is more important than the exact shape:

- callers can bound total rows scanned;
- callers can bound total decompressed bytes consumed by a row operation;
- counters use checked arithmetic;
- exceeding a configured budget returns a typed error;
- accounting stays on the streaming path and never requires pre-reading the complete entry.

A trusted local-file convenience path may use generous defaults, while tools that process externally supplied dumps can select strict budgets.

## 5. Archive header

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

`ArchiveFormat` must reject non-custom input in v0.1 rather than exposing fake supported variants.

## 6. Archive strings

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

## 7. Dump IDs and TOC entries

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
    // additional supported metadata fields, including COPY metadata
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLocation {
    NoData,
    Unknown,
    Offset(u64),
}
```

The public model should preserve the upstream distinction between no data, position not recorded, and a valid stored offset.

## 8. Metadata access

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn header(&self) -> &ArchiveHeader;
    pub fn entries(&self) -> &[TocEntry];
    pub fn entry(&self, id: DumpId) -> Option<&TocEntry>;
    pub fn table(&self, schema: &[u8], name: &[u8]) -> Option<TableRef<'_>>;
}
```

A UTF-8 convenience overload may be provided, but byte-oriented lookup should remain possible.

## 9. Table reference

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

## 10. Raw entry data

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

Raw entry access is intentionally lower-level than row-aware access. A readable table-data entry may still be available through this API even if its logical representation is unsupported by the COPY row parser.

## 11. COPY rows

The high-performance streaming row model remains borrowed:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRef<'a> {
    Null,
    Bytes(&'a [u8]),
}

pub struct Row<'a> {
    // borrowed from the row reader's current reusable buffer
}

impl Row<'_> {
    pub fn len(&self) -> usize;
    pub fn field(&self, index: usize) -> Option<FieldRef<'_>>;
    pub fn fields(&self) -> impl ExactSizeIterator<Item = FieldRef<'_>>;
}
```

`FieldRef::Bytes` contains the **logical field bytes after PostgreSQL COPY text escape decoding**. It does not expose the escaped on-wire spelling.

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

The detailed COPY parser contract is defined in `COPY-TEXT.md`.

## 12. Owned rows

v0.1 also needs an owned representation for a matching row that must survive reader advancement or teardown:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedField {
    Null,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRow {
    fields: Vec<OwnedField>,
}

impl OwnedRow {
    pub fn len(&self) -> usize;
    pub fn field(&self, index: usize) -> Option<&OwnedField>;
    pub fn fields(&self) -> &[OwnedField];
}
```

The conversion from the current borrowed row to `OwnedRow` copies only that row. It is bounded by the same configured row-size and field-count limits used during parsing.

Normal iteration should continue to use borrowed `Row`; `OwnedRow` is not a reason to allocate every row.

## 13. Table row convenience and column metadata

The archive exposes a table-row reader equivalent in purpose to:

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
representation validation
        ↓
CopyRowReader
```

and owns/references the parsed column layout for that table-data entry.

Representative metadata API:

```rust
impl<R: Read> TableRowReader<'_, R> {
    pub fn columns(&self) -> Result<&[Column], PgDumpError>;

    pub fn column_index(
        &self,
        name: &[u8],
    ) -> Result<Option<usize>, PgDumpError>;

    pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: ArchiveString,
}
```

This intentionally distinguishes three states:

```text
Ok(Some(index))  metadata valid, column found
Ok(None)         metadata valid, requested column absent
Err(...)         supported column layout unavailable/malformed
```

The implementation should derive the supported pg_dump-generated column list from the TOC entry's recorded COPY statement. Name lookup should be prepared once rather than reparsing the COPY statement for every row.

If the data stream can be read positionally but column metadata is unavailable or unsupported, `next_row()` may remain usable while column-aware operations fail explicitly rather than inventing names.

## 14. Supported table-data representation

The row-aware API targets normal pg_dump-generated COPY text table data.

Before constructing a `TableRowReader`, pgdumpx should validate that available TOC/table-data metadata is consistent with the supported COPY path.

INSERT-based dump modes such as:

```text
--inserts
--column-inserts
--rows-per-insert (when producing INSERT table data)
```

are not sent through `CopyRowReader` in v0.1. They return a typed unsupported-representation error from row-aware APIs.

Binary COPY decoding is also deferred.

This distinction keeps low-level archive readability separate from logical row-parser support.

## 15. First-match filtering

A primary v0.1 API is first-match filtering over the selected table stream:

```rust
impl<R: Read> TableRowReader<'_, R> {
    pub fn find_first<F>(
        &mut self,
        predicate: F,
    ) -> Result<Option<OwnedRow>, PgDumpError>
    where
        F: FnMut(&Row<'_>) -> bool;

    pub fn find_first_with_limits<F>(
        &mut self,
        scan_limits: ScanLimits,
        predicate: F,
    ) -> Result<Option<OwnedRow>, PgDumpError>
    where
        F: FnMut(&Row<'_>) -> bool;
}
```

The exact split between convenience/default methods may change during implementation. The required contract is that callers have a first-class way to supply scan limits to the same streaming search path.

Example:

```rust
let mut rows = archive.table_rows(b"public", b"orders")?;
let order_number = rows
    .column_index(b"order_number")?
    .ok_or(/* application error */)?;

let row = rows.find_first(|row| {
    row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
})?;
```

Semantics:

- scan rows in COPY order from the beginning of the selected data entry;
- call the predicate once for each parsed row;
- when it returns `true`, copy that current row into `OwnedRow` and stop;
- return `Ok(None)` if the stream ends without a match;
- reuse the same row buffer for non-matching rows;
- do not buffer the complete table;
- preserve byte-oriented fields so non-UTF-8 values can be matched;
- enforce configured row and total-work limits on the same streaming path.

The closure API deliberately avoids a SQL parser. Callers may implement equality, prefix, numeric parsing, or compound application-specific conditions themselves.

A small equality convenience helper may be added if it is demonstrably useful, but v0.1 does not require a condition DSL.

## 16. Row-search performance contract

`find_first` is **not** an indexed database lookup.

The custom archive's TOC lets pgdumpx select and seek to the table-data entry efficiently, but there is no required row-level value index inside that entry. Consequently:

```text
TOC lookup                ~= metadata lookup
seek to table-data entry  = direct seek when offset is recorded
find row inside table     = sequential decompression + COPY scan
```

A match near the start can terminate quickly. A late or absent match may require processing the complete selected table-data entry unless a configured scan budget stops the operation first. Worst-case unrestricted work is proportional to selected table data size.

The public documentation must not imply `O(1)`/`O(log n)` row lookup or database-index semantics.

A future sidecar index or restart-point design, if pursued, requires a separate architecture decision because compressed streams cannot generally be treated as arbitrary row-seekable byte arrays.

## 17. Compression model

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

## 18. Error API

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
    CopyColumnMetadataUnavailable { dump_id: DumpId },
    MalformedCopyStatement { dump_id: DumpId },
    UnsupportedTableDataRepresentation { dump_id: DumpId },
    ResourceLimitExceeded { resource: ResourceLimit, limit: u64 },
    ArithmeticOverflow { offset: Option<u64> },
    InvalidUtf8,
}
```

Exact fields should evolve from test requirements, but callers must not need to parse `Display` strings to determine error category.

`column_index()` returns `Ok(None)` for a missing requested name only when column metadata itself is valid. Failure to derive the column layout is a distinct error.

Scan-budget exhaustion should identify which budget was exceeded and, where practical, the amount of work already consumed.

## 19. Serialization

Serde support is not required for the parser to function.

If exposed, it should be optional:

```toml
[features]
default = []
serde = ["dep:serde"]
```

The CLI may use a presentation DTO instead of freezing every internal metadata field as a JSON compatibility promise.

## 20. Threading and parallel access

`Archive<R>` itself does not promise concurrent reads from one seekable source.

Future parallel extraction should use APIs that can produce independent sources, for example reopening a file path or accepting a source factory. This avoids mutex-protected seek thrashing and preserves simple borrowing semantics.

## 21. Versioning policy

Before v1.0:

- the public API may evolve;
- breaking changes must be intentional and documented once releases begin;
- private parser internals remain private;
- archive compatibility is version-explicit;
- verified compatibility is recorded separately from design targets;
- accepted policy changes are recorded through ADRs.

## 22. Deferred APIs

Intentionally deferred:

```text
archive writing
non-seekable sequential archive reader
parallel extraction API
SQL WHERE / condition DSL
persistent or sidecar row indexes
Binary COPY decoding
INSERT statement row parser
Arrow/Polars/Parquet conversion
Python bindings
recovery from corrupt archives
```
