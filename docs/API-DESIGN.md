# pgdumpx Public API Design

Status: **Implemented v0.1 public API contract**

This document describes the implemented public Rust API shape and the ownership, safety, streaming, row-search, resource-budget, and compatibility contracts that must remain deliberate as the pre-1.0 API evolves. The crate rustdoc is authoritative for exact current signatures and error variants.

## 1. Design principles

The public API:

- models a read-only PostgreSQL Custom Format archive;
- makes metadata inspection cheap after open;
- keeps payload access lazy;
- exposes streaming `Read` where raw entry bytes are useful;
- exposes row-aware COPY access without forcing UTF-8;
- supports column-aware first-match filtering without a SQL parser;
- distinguishes a missing requested column from unavailable/malformed column metadata;
- makes the sequential-scan cost of row lookup explicit;
- provides caller-visible ways to bound structural allocations, row-scan work, and raw decompressed output;
- rejects unsupported table-data representations explicitly rather than guessing COPY input;
- keeps the source owned by the archive so seeks are coordinated safely;
- uses typed IDs and enums rather than stringly typed control flow;
- exposes typed errors and location/resource context;
- keeps public metadata types opaque enough to evolve before v1.0;
- avoids leaking compression-library implementation types;
- remains suitable for later wrappers without designing around Python or Arrow today;
- requires no running PostgreSQL server, `libpq`, or `pg_restore` at runtime.

The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency. Dependency and native-build constraints are documented separately from the public API in `PACKAGING.md` and ADR 0007.

## 2. Opening an archive

The implemented reader-based shape is:

```rust
pub struct Archive<R> {
    // private
}

impl<R: Read + Seek> Archive<R> {
    pub fn open(reader: R) -> Result<Self, PgDumpError>;
    pub fn open_with_limits(reader: R, limits: Limits) -> Result<Self, PgDumpError>;
}
```

Opening parses supported archive metadata, the TOC, relationships, and lookup indexes under finite structural limits. It does not decompress selected entry bodies. A path convenience constructor is not required by the v0.1 public contract; callers can open a `File` and pass it to `Archive::open`.

## 3. Structural limits

Configuration fields remain private so the type can evolve without turning struct-literal layout into a compatibility promise.

Implemented shape:

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

`Default` delegates to finite compatibility-oriented limits. Applications processing hostile input can select stricter values. An unbounded structural mode is not the default.

These limits protect individual allocations and metadata cardinalities. They do not by themselves bound the total CPU/decompression work of scanning many otherwise-small rows.

## 4. Row-scan work limits

Long-running row operations accept operation-level work budgets:

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

`ScanLimits::default()` and `ScanLimits::unlimited()` leave both budgets unset. Callers opt into one or both limits.

Semantics:

- `max_rows = N` permits at most `N` complete rows to be yielded or evaluated by the operation;
- a row that would exceed the row or byte budget is not yielded to the caller or predicate;
- the matching row is included in both row and byte accounting;
- decompressed-byte accounting counts bytes consumed by the row parser from the decompressed COPY stream;
- field separators, row terminators, escape spellings, and the COPY terminator count when consumed;
- decoder or `BufRead` read-ahead that has not been consumed by the parser does not count;
- counters use checked arithmetic;
- limit exhaustion returns a typed resource error;
- accounting remains incremental and never pre-reads the complete entry;
- early match termination does not consume rows after the match.

Trusted local-file callers may intentionally use unlimited scan work. Tools processing externally supplied archives should select budgets appropriate to their resource policy.

## 5. Raw entry output limits

Row-scan limits do not protect callers that only stream decompressed entry bytes. The library therefore provides a separate raw-output budget.

Implemented shape:

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

The low-level unlimited reader remains available for trusted use, and bounded access is first-class:

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
    ) -> Result<Option<BoundedEntryDataReader<'_, R>>, PgDumpError>;

    pub fn copy_entry_to<W: Write>(
        &mut self,
        id: DumpId,
        writer: &mut W,
        limits: EntryReadLimits,
    ) -> Result<u64, PgDumpError>;
}
```

`EntryDataReader` implements `std::io::Read` and yields decompressed entry bytes. `BoundedEntryDataReader` wraps the same validated/decompressed path and applies the raw-output budget. If a bounded reader exhausts its budget, the `Read` error preserves a typed pgdumpx resource error as its source; high-level pgdumpx operations map it back to `PgDumpError`.

Raw limits count decompressed bytes returned or copied. Crossing the limit is an error and is never presented as a successful truncated stream. `Archive::copy_entry_to` writes incrementally, so bytes already accepted by the destination cannot be rolled back after a later input/limit/writer failure; the operation still returns an error.

The v0.1 CLI `extract` command uses `copy_entry_to` with a finite 1 GiB default and an explicit positive-`u64` override.

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

Archive metadata strings do not require valid UTF-8 at the lowest level.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveString(Vec<u8>);

impl ArchiveString {
    pub fn as_bytes(&self) -> &[u8];
    pub fn to_str(&self) -> Result<&str, Utf8Error>;
}
```

The inner storage remains private. Metadata lookup and identity remain byte-oriented; callers opt into fallible UTF-8 conversion.

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
    pub fn catalog_table_oid(&self) -> &ArchiveString;
    pub fn catalog_oid(&self) -> &ArchiveString;
    pub fn name(&self) -> &ArchiveString;
    pub fn name_bytes(&self) -> &[u8];
    pub fn description(&self) -> &ArchiveString;
    pub fn description_bytes(&self) -> &[u8];
    pub fn section(&self) -> Section;
    pub fn definition(&self) -> Option<&ArchiveString>;
    pub fn drop_statement(&self) -> Option<&ArchiveString>;
    pub fn copy_statement(&self) -> Option<&ArchiveString>;
    pub fn namespace(&self) -> Option<&ArchiveString>;
    pub fn namespace_bytes(&self) -> Option<&[u8]>;
    pub fn tablespace(&self) -> Option<&ArchiveString>;
    pub fn table_access_method(&self) -> Option<&ArchiveString>;
    pub fn relation_kind(&self) -> Option<i32>;
    pub fn owner(&self) -> Option<&ArchiveString>;
    pub fn owner_bytes(&self) -> Option<&[u8]>;
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

The public model preserves the upstream distinction between no data, position not recorded, and a valid stored offset. Optional archive strings preserve encoded NULL as `None` rather than conflating it with an encoded empty string.

Version-dependent TOC fields preserve absence explicitly. `table_access_method()` reflects the field encoded in the supported archive 1.14+ layouts. `relation_kind()` is `None` for archive 1.14/1.15 because no relkind slot exists there; archive 1.16 values are returned as `Some(value)`, including an encoded zero as `Some(0)`.

Metadata remains byte-oriented. Callers opt into UTF-8 through `ArchiveString::to_str()` rather than the parser discarding non-UTF-8 bytes. The generic TOC surface also exposes object-type/dependency metadata needed to inspect version-conditional large-object entries without implying a flat large-object payload extraction API.

Public enums expected to grow as compatibility expands are `#[non_exhaustive]` before v1.0. Exhaustive internal enums remain private.

## 9. Metadata access

```rust
impl<R: Read + Seek> Archive<R> {
    pub fn header(&self) -> &ArchiveHeader;
    pub fn entries(&self) -> &[TocEntry];
    pub fn entry(&self, id: DumpId) -> Option<&TocEntry>;
    pub fn table(&self, schema: &[u8], name: &[u8]) -> Option<TableRef<'_>>;
}
```

Byte-oriented lookup is the stable lowest-level contract. UTF-8 conversion is an opt-in boundary rather than parser policy.

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
    pub fn data_representation(&self) -> Result<TableDataRepresentation, PgDumpError>;
    pub fn columns(&self) -> Result<&[Column], PgDumpError>;
    pub fn column_index(&self, name: &[u8]) -> Result<Option<usize>, PgDumpError>;
}
```

The type is a metadata handle only. It does not borrow entry payload bytes. Representation and column access are derived from stored TOC metadata and therefore do not decompress the table-data entry.

## 11. Raw entry data

Entry reads are lazy and coordinated through a mutable archive borrow.

The mutable borrow intentionally prevents two readers from independently seeking the same underlying source at once. Future parallel file APIs should use separately opened or cloneable sources instead of weakening this invariant.

Raw entry access is lower-level than row-aware access. A readable table-data entry may remain available through raw APIs even when its logical representation is unsupported by the COPY row parser.

Large-object entries with internal OID framing may require a different future API and are not forced into a flat `Read` abstraction.

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

Implemented row-reader surface:

```rust
pub struct CopyRowReader<R> {
    // private
}

impl<R: Read> CopyRowReader<R> {
    pub fn new(reader: R) -> Self;
    pub fn with_limits(reader: R, limits: Limits) -> Self;
    pub fn with_scan_limits(reader: R, scan_limits: ScanLimits) -> Self;
    pub fn with_limits_and_scan_limits(
        reader: R,
        limits: Limits,
        scan_limits: ScanLimits,
    ) -> Self;
    pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError>;
}
```

`Row` is valid only until the next mutable operation on the row reader. A standard `Iterator<Item = Row<'_>>` cannot express this lending relationship without changing ownership semantics. v0.1 therefore exposes `next_row(&mut self)` and documents the invalidation boundary explicitly instead of allocating every row merely to satisfy `Iterator`.

The detailed COPY parser contract is defined in `COPY-TEXT.md`.

## 13. Owned rows

An owned representation is used for a matching row that must survive reader advancement or teardown:

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

Normal iteration continues to use borrowed `Row`; `OwnedRow` does not cause allocation of every scanned row.

## 14. Table row convenience and column metadata

The archive exposes a table-row reader:

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
metadata lookup + representation validation
        ↓
entry seek + block validation
        ↓
EntryDataReader
        ↓
CopyRowReader
```

Metadata API:

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

If the COPY data stream is positionally readable but column metadata is unavailable or malformed, `next_row()` remains usable while column-aware operations fail explicitly rather than inventing names. Unsupported table-data representations are rejected before constructing `TableRowReader`.

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

This distinction keeps low-level archive readability separate from logical row-parser support. The official PostgreSQL 18.4 `--inserts` fixture verifies that raw selected-entry extraction remains available while `table_rows` rejects the representation before COPY parsing.

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

The default method performs the same streaming scan without an additional operation-level budget; constructor-level scan limits still apply. `find_first_with_limits` adds a budget measured from that call's current stream position.

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

- scan rows in COPY order from the reader's current position; a fresh `TableRowReader` starts at the beginning of the selected data entry;
- call the predicate once for each fully parsed row within budget;
- when it returns `true`, copy that row into `OwnedRow` and stop;
- return `Ok(None)` if the remaining stream ends without a match;
- reuse the same buffer for non-matching rows;
- do not buffer the complete table;
- preserve byte-oriented fields so non-UTF-8 values can be matched;
- enforce structural and total-work limits on the same streaming path;
- do not rewind a reader that has already yielded rows.

The closure API deliberately avoids a SQL parser. Callers implement equality, prefix, numeric parsing, or compound application-specific conditions themselves.

## 17. Row-search performance contract

`find_first` is **not** an indexed database lookup.

The custom archive's TOC lets pgdumpx select and seek to the table-data entry efficiently, but there is no required row-level value index inside that entry:

```text
TOC lookup                ~= metadata lookup
seek to table-data entry  = direct seek when offset is recorded
find row inside table     = sequential decompression + COPY scan
```

A match near the start can terminate quickly. A late or absent match may process the complete selected entry for a fresh reader, or all remaining selected data after prior row reads, unless a configured budget stops the operation. Worst-case unrestricted work is proportional to the remaining selected table-data size.

Public documentation does not imply `O(1)`/`O(log n)` row lookup or database-index semantics.

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

Compression backend selection is an implementation and packaging concern. LZ4 and Zstandard are optional library features; the default CLI enables both. A disabled backend remains identifiable in metadata and produces a typed selected-entry read error instead of leaking dependency-specific types.

## 19. Error API

`PgDumpError` is `#[non_exhaustive]` and exposes typed variants rather than requiring callers to parse `Display` strings. The exact current variants are documented in crate rustdoc; the taxonomy distinguishes at least:

- I/O and unexpected EOF;
- invalid magic/archive format/version and integer/offset encoding;
- TOC/string/dependency/relationship failures;
- invalid entry data location, block type, or dump ID after seek;
- unsupported compression/backend availability and decompression failures;
- malformed COPY framing/escapes/terminator/column metadata;
- unsupported table-data representation;
- missing table/data relationships and unknown requested columns;
- structural, scan-work, and raw-output resource exhaustion;
- checked-counter/offset overflow;
- explicit UTF-8 conversion failures.

`column_index()` returns `Ok(None)` for a missing name only when column metadata itself is valid. Failure to derive the layout is a distinct error.

Limit exhaustion identifies the resource and, where practical, consumed work. Location, dump ID, row number, and byte offset are included when they materially help diagnosis.

## 20. CLI boundary

The CLI is not part of the core Rust type system, but its encoding, limit, output, and exit choices align with the byte-oriented API.

Implemented v0.1 commands:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Rules:

- table selectors contain exactly one ASCII `.` and two non-empty components; SQL identifier quoting/escaping is not supported;
- schema/table/column/value command-line query arguments are UTF-8, while the Rust API remains byte-oriented;
- `inspect` and `list` remain metadata-only;
- `extract` writes the decompressed selected table-data body as binary-safe bytes and does not add DDL or a `COPY` statement wrapper;
- `extract` uses a bounded library path, defaults to 1,073,741,824 bytes (1 GiB), and accepts an explicit positive-`u64` override;
- streamed `extract` bytes cannot be rolled back after a later failure, so partial stdout can coexist with a non-success exit; diagnostics remain on stderr;
- `find` compares its UTF-8 value bytes with logical post-unescape field bytes;
- `find` exposes optional positive-`u64` row and parser-consumed decompressed-byte scan budgets;
- matched `find` output is one normalized ASCII-safe COPY text record;
- exit `0` means match/success, `find` exit `1` means completed no-match, and exit `2+` means usage/runtime/resource failure.

A byte-literal CLI syntax, JSON representation, or full restorable SQL output requires a separate design.

## 21. Serialization

Serde support is not required and is not part of the v0.1 public contract. Presentation/serialization DTOs can be added later without freezing every archive metadata type as a serialization compatibility promise.

## 22. Threading and parallel access

`Archive<R>` does not promise concurrent reads from one seekable source.

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

Intentionally deferred beyond v0.1:

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
