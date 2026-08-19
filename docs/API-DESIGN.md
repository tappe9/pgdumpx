# pgdumpx Public API Design

Status: **Accepted direction for v0.1 implementation**

This document defines the intended public Rust API shape. Exact names may be refined during TDD, but changes to ownership, safety, streaming, row-search, resource-budget, and compatibility contracts should be deliberate.

## 1. Design principles

The public API should:

- model a read-only PostgreSQL Custom Format archive;
- make metadata inspection cheap after open;
- keep payload access lazy;
- expose streaming `Read` where raw entry bytes are useful;
- expose row-aware COPY access without forcing UTF-8;
- support column-aware first-match filtering without a SQL parser;
- distinguish a missing requested column from unavailable/malformed column metadata;
- make the sequential-scan cost of row lookup explicit;
- provide caller-visible ways to bound structural allocations, row-scan work, and raw decompressed output;
- reject unsupported table-data representations explicitly rather than guessing COPY input;
- keep the source owned by the archive so seeks are coordinated safely;
- use typed IDs and enums rather than stringly typed control flow;
- expose typed errors and location/resource context;
- keep public metadata types opaque enough to evolve before v1.0;
- avoid leaking compression-library implementation types;
- remain suitable for later wrappers without designing around Python or Arrow today;
- require no running PostgreSQL server, `libpq`, or `pg_restore` at runtime.

The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency. Dependency and native-build constraints are documented separately from the public API.

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

Opening parses archive metadata and the TOC only. It must not decompress all data entries.

## 3. Structural limits

Configuration types may use constructors/builders, but their exact field layout is not a public compatibility commitment.

Representative direction:

```rust
#[derive(Debug, Clone)]
pub struct Limits {
    max_toc_entries: usize,
    max_string_bytes: usize,
    max_dependencies_per_entry: usize,
    max_row_bytes: usize,
    max_fields_per_row: usize,
}

impl Limits {
    pub fn default_compatible() -> Self;
    pub fn with_max_toc_entries(self, value: usize) -> Self;
    pub fn with_max_string_bytes(self, value: usize) -> Self;
    pub fn with_max_dependencies_per_entry(self, value: usize) -> Self;
    pub fn with_max_row_bytes(self, value: usize) -> Self;
    pub fn with_max_fields_per_row(self, value: usize) -> Self;
}
```

`Default` may delegate to compatibility-oriented finite limits. Applications processing hostile input should be able to select stricter values.

An explicitly unbounded structural mode must not be the default merely for convenience.

These limits protect individual allocations and metadata cardinalities. They do not by themselves bound the total CPU/decompression work of scanning millions of otherwise-small rows.

## 4. Row-scan work limits

Long-running row operations accept operation-level work budgets equivalent in purpose to:

```rust
#[derive(Debug, Clone)]
pub struct ScanLimits {
    max_rows: Option<u64>,
    max_decompressed_bytes: Option<u64>,
}

impl ScanLimits {
    pub fn unlimited() -> Self;
    pub fn with_max_rows(self, value: u64) -> Self;
    pub fn with_max_decompressed_bytes(self, value: u64) -> Self;
}
```

Exact naming and default values may evolve from TDD and real fixture sizes.

Required semantics:

- `max_rows = N` permits at most `N` complete rows to be yielded or evaluated by the operation;
- a row that would exceed the row or byte budget is not yielded to the caller or predicate;
- the matching row is included in both row and byte accounting;
- decompressed-byte accounting counts bytes consumed by the row parser from the decompressed COPY stream;
- field separators, row terminators, and the COPY terminator count when consumed;
- decoder or `BufRead` read-ahead that has not been consumed by the parser does not count;
- counters use checked arithmetic;
- limit exhaustion returns a typed resource error;
- accounting remains incremental and never pre-reads the complete entry;
- early match termination does not consume rows after the match.

A trusted local-file convenience path may use generous or unlimited scan defaults. Tools processing externally supplied archives should select explicit budgets.

## 5. Raw entry output limits

Row-scan limits do not protect callers that only stream decompressed entry bytes. The library therefore provides a separate raw-output budget.

Representative direction:

```rust
#[derive(Debug, Clone)]
pub struct EntryReadLimits {
    max_decompressed_bytes: Option<u64>,
}

impl EntryReadLimits {
    pub fn unlimited() -> Self;
    pub fn with_max_decompressed_bytes(self, value: u64) -> Self;
}
```

The low-level unlimited reader may remain available for trusted use, but a bounded path is first-class:

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn entry_reader(
        &mut self,
        id: DumpId,
    ) -> Result<Option<EntryDataReader<'_, R>>, PgDumpError>;

    pub fn entry_reader_with_limits(
        &mut self,
        id: DumpId,
        limits: EntryReadLimits,
    ) -> Result<Option<EntryDataReader<'_, R>>, PgDumpError>;

    pub fn copy_entry_to<W: Write>(
        &mut self,
        id: DumpId,
        writer: &mut W,
        limits: EntryReadLimits,
    ) -> Result<u64, PgDumpError>;
}
```

`EntryDataReader` implements `std::io::Read` and yields decompressed entry bytes. If a limit-aware reader exhausts its budget, the `Read` error must preserve a typed pgdumpx resource error as its source; high-level pgdumpx operations map it back to `PgDumpError`.

Raw limits count decompressed bytes returned or copied. Crossing the limit is an error and must not be presented as a successful truncated stream. A deliberately truncating API, if ever added, must be named separately.

The v0.1 CLI `extract` command uses `copy_entry_to` or an equivalent bounded high-level path.

## 6. Archive header

Public metadata structs are opaque and exposed through accessors.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveHeader {
    // private
}

impl ArchiveHeader {
    pub fn version(&self) -> ArchiveVersion;
    pub fn integer_size(&self) -> u8;
    pub fn offset_size(&self) -> u8;
    pub fn compression(&self) -> Compression;
    pub fn created_at(&self) -> &ArchiveTimestamp;
    pub fn database_name(&self) -> &ArchiveString;
    pub fn server_version(&self) -> &ArchiveString;
    pub fn dump_version(&self) -> &ArchiveString;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveVersion {
    // private
}

impl ArchiveVersion {
    pub const fn new(major: u8, minor: u8, revision: u8) -> Self;
    pub const fn major(self) -> u8;
    pub const fn minor(self) -> u8;
    pub const fn revision(self) -> u8;
}
```

v0.1 rejects non-custom input rather than exposing an `ArchiveFormat` variant set that implies broader support.

## 7. Archive strings

Archive metadata strings must not require valid UTF-8 at the lowest level.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveString(Vec<u8>);

impl ArchiveString {
    pub fn as_bytes(&self) -> &[u8];
    pub fn to_str(&self) -> Result<&str, Utf8Error>;
}
```

The inner storage remains private. If compatibility evidence proves a stronger encoding guarantee for a particular field, a convenience accessor may expose it without weakening the general parser.

## 8. Dump IDs and TOC entries

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DumpId(i32);

impl DumpId {
    pub const fn as_i32(self) -> i32;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    // private
}

impl TocEntry {
    pub fn id(&self) -> DumpId;
    pub fn has_data(&self) -> bool;
    pub fn name_bytes(&self) -> &[u8];
    pub fn description_bytes(&self) -> &[u8];
    pub fn section(&self) -> Section;
    pub fn namespace_bytes(&self) -> Option<&[u8]>;
    pub fn owner_bytes(&self) -> &[u8];
    pub fn dependencies(&self) -> &[DumpId];
    pub fn data_location(&self) -> DataLocation;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataLocation {
    NoData,
    Unknown,
    Offset(u64),
}
```

The public model preserves the upstream distinction between no data, position not recorded, and a valid stored offset.

Public enums expected to grow as compatibility expands should be `#[non_exhaustive]` before v1.0. Exhaustive internal enums may remain private.

## 9. Metadata access

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn header(&self) -> &ArchiveHeader;
    pub fn entries(&self) -> &[TocEntry];
    pub fn entry(&self, id: DumpId) -> Option<&TocEntry>;
    pub fn table(&self, schema: &[u8], name: &[u8]) -> Option<TableRef<'_>>;
}
```

A UTF-8 convenience overload may be provided, but byte-oriented lookup remains available.

## 10. Table reference

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

## 11. Raw entry data

Entry reads are lazy and coordinated through a mutable archive borrow.

The mutable borrow intentionally prevents two readers from independently seeking the same underlying source at once. Future parallel file APIs should use separately opened or cloneable sources instead of weakening this invariant.

Raw entry access is lower-level than row-aware access. A readable table-data entry may remain available through raw APIs even when its logical representation is unsupported by the COPY row parser.

Large-object entries with internal OID framing may require a different API and are not forced into a flat `Read` abstraction.

## 12. COPY rows and lending semantics

The high-performance streaming row model is borrowed:

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

`FieldRef::Bytes` contains logical field bytes after PostgreSQL COPY text escape decoding. It does not expose the escaped on-wire spelling.

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

`Row` is valid only until the next mutable operation on the row reader. A standard `Iterator<Item = Row<'_>>` cannot express this lending relationship without changing ownership semantics. v0.1 therefore documents `next_row(&mut self)` explicitly instead of allocating every row merely to satisfy `Iterator`.

The detailed COPY parser contract is defined in `COPY-TEXT.md`.

## 13. Owned rows

An owned representation is required for a matching row that must survive reader advancement or teardown:

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

Conversion from the current borrowed row copies only that row. It remains bounded by the configured row-size and field-count limits.

Normal iteration continues to use borrowed `Row`; `OwnedRow` is not a reason to allocate every row.

## 14. Table row convenience and column metadata

The archive exposes a table-row reader equivalent in purpose to:

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn table_rows(
        &mut self,
        schema: &[u8],
        table: &[u8],
    ) -> Result<TableRowReader<'_, R>, PgDumpError>;
}
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
    // private
}

impl Column {
    pub fn name_bytes(&self) -> &[u8];
    pub fn name_str(&self) -> Result<&str, Utf8Error>;
}
```

Column lookup distinguishes:

```text
Ok(Some(index))  metadata valid, column found
Ok(None)         metadata valid, requested column absent
Err(...)         supported column layout unavailable or malformed
```

The implementation derives the supported pg_dump-generated column list from the TOC entry's recorded COPY statement. Name lookup is prepared once rather than reparsing the statement for every row.

If the data stream is positionally readable but column metadata is unavailable or unsupported, `next_row()` may remain usable while column-aware operations fail explicitly rather than inventing names.

## 15. Supported table-data representation

The row-aware API targets normal pg_dump-generated COPY text table data.

Before constructing a `TableRowReader`, pgdumpx validates that available TOC/table-data metadata is consistent with the supported COPY path.

INSERT-based dump modes such as:

```text
--inserts
--column-inserts
--rows-per-insert (when producing INSERT table data)
```

are not sent through `CopyRowReader` in v0.1. They return a typed unsupported-representation error from row-aware APIs.

Binary COPY decoding is also deferred.

This distinction keeps low-level archive readability separate from logical row-parser support.

## 16. First-match filtering

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

The exact split between default and explicit-limit methods may change during implementation. Callers must have a first-class way to supply scan limits to the same streaming search path.

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
- call the predicate once for each fully parsed row within budget;
- when it returns `true`, copy that row into `OwnedRow` and stop;
- return `Ok(None)` if the stream ends without a match;
- reuse the same buffer for non-matching rows;
- do not buffer the complete table;
- preserve byte-oriented fields so non-UTF-8 values can be matched;
- enforce structural and total-work limits on the same streaming path.

The closure API deliberately avoids a SQL parser. Callers may implement equality, prefix, numeric parsing, or compound application-specific conditions themselves.

A small equality helper may be added if demonstrated usage justifies it, but v0.1 does not require a condition DSL.

## 17. Row-search performance contract

`find_first` is **not** an indexed database lookup.

The custom archive's TOC lets pgdumpx select and seek to the table-data entry efficiently, but there is no required row-level value index inside that entry:

```text
TOC lookup                ~= metadata lookup
seek to table-data entry  = direct seek when offset is recorded
find row inside table     = sequential decompression + COPY scan
```

A match near the start can terminate quickly. A late or absent match may process the complete selected entry unless a configured budget stops the operation. Worst-case unrestricted work is proportional to selected table-data size.

Public documentation must not imply `O(1)`/`O(log n)` row lookup or database-index semantics.

A future sidecar index or restart-point design requires a separate ADR because compressed streams cannot generally be treated as arbitrary row-seekable byte arrays.

## 18. Compression model

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compression {
    None,
    Gzip,
    Lz4,
    Zstd,
}
```

The enum describes the archive, not a dependency-specific decoder type. Unsupported or invalid identifiers are errors.

Compression backend selection is an implementation and packaging concern. A backend that introduces a material native build/runtime constraint must be documented and, where practical, feature-gated; it is not exposed through the archive model.

## 19. Error API

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
    ResourceLimitExceeded {
        resource: ResourceLimit,
        limit: u64,
        consumed: Option<u64>,
    },
    ArithmeticOverflow { offset: Option<u64> },
    InvalidUtf8,
}
```

Exact fields evolve from tests, but callers must not parse `Display` strings to determine error category.

`column_index()` returns `Ok(None)` for a missing name only when column metadata itself is valid. Failure to derive the layout is a distinct error.

Limit exhaustion identifies the resource and, where practical, consumed work. Location, dump ID, row number, and byte offset are included when they materially help diagnosis.

## 20. CLI boundary

The CLI is not part of the core Rust type system, but its encoding and output choices must align with the byte-oriented API.

v0.1 rules:

- schema/table/column/value command-line arguments are UTF-8;
- `extract` writes the decompressed selected table-data body as binary-safe bytes;
- `extract` does not add DDL or a `COPY` statement wrapper;
- `extract` uses a bounded library path and fails rather than silently truncating;
- `find` compares its UTF-8 value bytes with logical post-unescape field bytes;
- exit `0` means match, exit `1` means no match, and exit `2+` means failure.

A byte-literal CLI syntax, JSON representation, or full restorable SQL output requires a separate design.

## 21. Serialization

Serde support is not required for the parser to function.

If exposed, it should be optional:

```toml
[features]
default = []
serde = ["dep:serde"]
```

The CLI may use presentation DTOs instead of freezing every archive metadata type as a JSON compatibility promise.

## 22. Threading and parallel access

`Archive<R>` itself does not promise concurrent reads from one seekable source.

Future parallel extraction should use APIs that can produce independent sources, such as reopening a path or accepting a source factory. This avoids mutex-protected seek thrashing and preserves simple borrowing semantics.

## 23. Versioning policy

Before v1.0:

- the public API may evolve;
- breaking changes must still be intentional and documented once releases begin;
- public metadata fields remain private unless direct construction is a deliberate contract;
- extensible public enums are `#[non_exhaustive]`;
- private parser internals remain private;
- archive compatibility is version-explicit;
- verified compatibility is recorded separately from design targets;
- accepted policy changes are recorded through ADRs.

## 24. Deferred APIs

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
byte-literal CLI query syntax
complete restorable SQL generation
```
