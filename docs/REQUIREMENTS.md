# pgdumpx Requirements

Status: **Accepted for v0.1 implementation**

This document defines the functional, safety, compatibility, and quality contract for pgdumpx v0.1. It describes intended behavior, not existing implementation.

## 1. Product definition

pgdumpx is a read-only Rust library and CLI for bounded, byte-oriented row inspection of PostgreSQL custom-format (`pg_dump -Fc`) archives.

The primary product is the reusable Rust library. The CLI is a consumer and an end-to-end acceptance path for the same library behavior.

The default build must not require a running PostgreSQL server, `libpq`, `pg_restore`, or another PostgreSQL executable at runtime. The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency.

## 2. Goals

### G-001 — Open large archives without loading their payloads

Opening an archive must parse metadata/TOC only and must not decompress all table data.

### G-002 — Selective entry access

Given a TOC entry with a usable data offset, callers must be able to seek directly to the entry and stream its decompressed bytes.

### G-003 — Row-aware table extraction

For supported table-data entries using PostgreSQL COPY text representation, callers must be able to iterate rows and fields without loading the complete table.

### G-004 — First-match row retrieval

Given a table and a row predicate, callers must be able to scan the selected table's streamed COPY rows and return the first matching row without restoring the archive or buffering the complete table.

### G-005 — Safe untrusted-input handling

Malformed archive or COPY data must produce a typed error rather than unchecked memory access, arithmetic wraparound, or parser panic.

### G-006 — Reusable, runtime-independent core

Archive behavior must not depend on terminal output, Python, Arrow, a PostgreSQL connection, SQL execution, a SQL query parser, `libpq`, or invocation of `pg_restore`.

### G-007 — Measurable performance

The project must include repeatable benchmarks before making comparative performance claims.

### G-008 — Bounded row-scan work

The library must provide a way for applications to bound total row-scan/decompression work in addition to bounding individual metadata and row allocations.

### G-009 — Bounded raw extraction

The library must provide a high-level way to bound decompressed bytes when extracting a raw selected entry. Callers must not be forced to reimplement safe output accounting around a low-level `Read` adapter.

### G-010 — Early end-to-end value

Implementation must prioritize a narrow archive 1.16 + none/gzip path that reaches COPY rows, column lookup, `find_first`, and `pgdumpx find` before broad compatibility expansion.

This goal changes delivery order, not the final v0.1 compatibility target.

## 3. Non-goals for v0.1

v0.1 will not:

- write PostgreSQL dump archives;
- replace `pg_dump` or `pg_restore`;
- restore SQL into a database;
- execute SQL stored in the archive;
- support Directory (`-Fd`) or Tar (`-Ft`) formats;
- support arbitrary historical archive versions older than 1.14;
- guarantee Binary COPY decoding;
- provide row-aware parsing for INSERT-based dump modes such as `--inserts`, `--column-inserts`, or INSERT output produced by `--rows-per-insert`;
- provide a SQL `WHERE` parser or general SQL expression engine;
- provide a persistent row-level index or guarantee constant-time row lookup;
- expose Arrow, Polars, Parquet, or Python APIs from the core crate;
- promise parallel extraction;
- generate a complete restorable SQL script from `extract`;
- provide arbitrary byte-literal CLI query syntax;
- promise that every transitive dependency contains no native code or internal `unsafe`;
- promise compatibility with malformed archives accepted accidentally by a particular PostgreSQL release.

## 4. Functional requirements

### FR-001 — Open `Read + Seek`

The library must expose an entry point equivalent in purpose to:

```rust
pub fn open<R: Read + Seek>(reader: R) -> Result<Archive<R>, PgDumpError>;
```

A path convenience API may exist, but the parser's fundamental API is source-oriented rather than filesystem-global.

### FR-002 — Validate custom-format magic

The parser must recognize the `PGDMP` custom-archive magic and reject other input with a typed error.

### FR-003 — Parse version and size metadata

The parser must decode the archive version, integer-size field, offset-size field, archive format code, and version-dependent compression metadata.

### FR-004 — Supported archive versions

The final v0.1 release must support archive versions 1.14, 1.15, and 1.16.

Implementation may initially verify only archive 1.16 while the first vertical slice is developed. Any newer or older unsupported version must fail explicitly with `UnsupportedArchiveVersion` or equivalent. The parser must not guess a newer format's layout.

### FR-005 — Parse archive metadata

The parser must decode version-appropriate metadata required for inspection, including creation timestamp fields, database name, originating PostgreSQL server version string, and `pg_dump` version string where present.

### FR-006 — Parse TOC entries

The parser must parse the TOC metadata required to identify and relate objects, including at least:

- dump ID;
- whether the entry owns dump data;
- catalog/table OID metadata when encoded;
- tag/name;
- object description/type text;
- section;
- definition/drop/copy statements when encoded;
- namespace/schema;
- tablespace;
- table access method where present;
- owner;
- dependency dump IDs;
- custom-format data offset state and offset.

Version-conditional fields must be handled explicitly.

The first vertical slice may implement the minimum 1.16 fields required for table lookup and row access, but the v0.1 release must satisfy the complete supported-version requirements.

### FR-007 — Build entry indexes

The archive must support efficient lookup by dump ID and practical table/table-data lookup by `(schema, name)` without rescanning payload data.

These indexes are entry-level. v0.1 does not require an index mapping field values to individual rows.

### FR-008 — Preserve raw metadata bytes where encoding is not guaranteed

The lowest-level parser must not require arbitrary archive strings or table data to be valid UTF-8 merely for the archive to be structurally readable.

Ergonomic UTF-8 accessors may return conversion errors.

### FR-009 — Seek to entry data

For an entry whose stored offset state indicates a valid position, the library must seek with checked conversion/arithmetic and validate the block header found there.

### FR-010 — Validate data block identity

After seeking, the parser must verify the custom block type and encoded dump ID before exposing payload bytes. A mismatch is a typed integrity error.

### FR-011 — Stream entry data

The library must provide a `Read`-compatible streaming view of decompressed table-data bytes without buffering the complete entry.

A low-level unlimited reader may exist for trusted callers, but the library must also expose a bounded path equivalent in purpose to:

```rust
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
```

Raw extraction limits count decompressed bytes returned or copied. Exceeding the limit must return an error rather than reporting successful truncation.

### FR-012 — Compression support

The final v0.1 release must support and test:

- none;
- gzip;
- LZ4;
- Zstandard.

The first vertical slice targets none and gzip. The implementation must honor archive-version-specific compression representation rather than treating all versions identically.

Compression backend choices must not leak dependency-specific types into the public archive API. Material native build/runtime constraints must be documented.

### FR-013 — Table/table-data relationship

The public model must make it convenient to locate the table-data entry associated with a table identity where the archive contains that relationship.

### FR-014 — COPY text rows

The library must provide a row reader for supported table-data entries encoded in PostgreSQL COPY text form.

The parser must correctly handle at least:

- tab-separated fields in the normal pg_dump COPY text representation;
- newline record boundaries;
- `\N` NULL marker;
- PostgreSQL COPY text backslash escapes;
- empty non-NULL fields;
- `\.` end-of-data marker when present in the stream representation;
- rows larger than the configured row limit;
- non-UTF-8 field bytes at the byte-oriented API layer.

Detailed byte semantics live in `docs/COPY-TEXT.md` and must remain consistent with this requirement.

### FR-015 — Byte-oriented field access

The core row API must allow fields to be consumed as bytes. String conversion is an explicit convenience operation, not a parser prerequisite.

`FieldRef::Bytes` represents logical field bytes **after COPY text escape decoding**, not the escaped archive spelling.

### FR-016 — Metadata-only inspection

Callers must be able to enumerate header and TOC information without touching entry payloads.

### FR-017 — COPY column layout

For supported pg_dump-generated table-data entries, pgdumpx must derive the COPY column layout from the entry's recorded COPY statement when the required metadata is available.

The row reader must expose column names as bytes and provide efficient name-to-index lookup.

Column-aware APIs must distinguish:

```text
metadata valid + column found      -> Ok(Some(index))
metadata valid + column not found  -> Ok(None)
metadata unavailable/malformed     -> Err(...)
```

Representative direction:

```rust
pub fn columns(&self) -> Result<&[Column], PgDumpError>;

pub fn column_index(
    &self,
    name: &[u8],
) -> Result<Option<usize>, PgDumpError>;
```

If rows are positionally readable but the supported column layout cannot be derived, positional iteration may remain available while column-aware lookup/filtering fails explicitly rather than guessing names.

### FR-018 — Lending row API

Normal row iteration must return a borrowed row backed by a reusable reader buffer or an equivalent allocation-conscious representation.

A method equivalent in purpose to:

```rust
pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError>;
```

is acceptable and intentionally need not implement standard `Iterator` when the yielded row borrows from the same object that must be mutably advanced.

The lifetime boundary must be documented: a borrowed row remains valid only until the next mutable row-reader operation.

### FR-019 — First-match predicate scan

The row reader must provide an operation equivalent in purpose to:

```rust
pub fn find_first<F>(
    &mut self,
    predicate: F,
) -> Result<Option<OwnedRow>, PgDumpError>
where
    F: FnMut(&Row<'_>) -> bool;
```

Semantics:

- rows are evaluated in archive/COPY order;
- the first row whose predicate returns `true` is returned;
- scanning stops immediately after the first match is fully parsed;
- if no row matches, the result is `Ok(None)` after the selected stream is exhausted;
- the operation uses the same streaming, decompression, parsing, and resource-limit path as normal row iteration;
- the operation does not allocate or buffer the complete table.

### FR-020 — Owned first-match result

A successful first-match operation must return an owned row whose fields remain valid after the streaming reader advances or is dropped.

The owned result remains byte-oriented and is bounded by the same row-size and field-count limits as the borrowed row.

### FR-021 — Explicit row-search performance semantics

Public documentation must state that custom archives provide table-data entry offsets, not row-level indexes.

Therefore first-match lookup is a sequential scan within the selected table:

- selecting the table-data entry uses parsed TOC/index metadata;
- row matching requires streaming decompression and parsing from the beginning of that entry;
- a match may terminate early;
- a missing or late match may read the complete selected entry;
- worst-case unrestricted work is proportional to selected table-data size.

The project must not describe this API as database-index lookup or imply constant-time row access.

### FR-022 — Explicit unsupported table-data representation

Before row-aware parsing, pgdumpx must determine whether the selected entry is supported as COPY text from available TOC/table-data metadata.

INSERT-based dump modes such as `--inserts`, `--column-inserts`, and INSERT output produced through `--rows-per-insert` must not be sent through the COPY text parser.

Row-aware access to such data returns a typed error equivalent to `UnsupportedTableDataRepresentation`. Raw entry extraction may remain available if the archive entry itself is readable.

### FR-023 — Scan work budgets

Long-running row operations must support configurable work budgets equivalent in purpose to:

```rust
pub struct ScanLimits {
    // private
}
```

with maximum rows and maximum decompressed bytes.

Required accounting semantics:

- row counters use checked arithmetic;
- `max_rows = N` permits at most `N` complete rows to be yielded or evaluated;
- a row that would cross a row or byte budget is not yielded or passed to the predicate;
- the matching row counts toward both budgets;
- decompressed-byte accounting applies to bytes consumed by the row parser, including separators and terminators;
- unread decoder/reader lookahead does not count merely because it was buffered;
- exceeding a budget returns a typed resource-limit error;
- budget enforcement does not require buffering the complete entry;
- the library exposes this mechanism directly rather than requiring callers to recreate the scan loop.

### FR-024 — Encapsulated public metadata

Public archive metadata types such as `ArchiveHeader`, `ArchiveVersion`, `DumpId`, `TocEntry`, and `Column` should use private fields with accessors unless direct construction is a deliberate requirement.

Public enums expected to grow with compatibility should be `#[non_exhaustive]` before v1.0.

### FR-025 — Standalone read path

The production archive/COPY path must be implemented within pgdumpx rather than delegating mandatory behavior to another dump library.

Other libraries may be used as research references or differential-test comparators. This requirement exists so byte preservation, limits, integrity checks, and error semantics remain under one coherent contract.

## 5. CLI requirements

### CLI-001 — Inspect

```text
pgdumpx inspect <FILE>
```

Print archive-level metadata and a concise summary without decompressing all data entries.

### CLI-002 — List

```text
pgdumpx list <FILE>
```

List TOC entries with stable, understandable identifiers and schema/name context.

### CLI-003 — Extract

```text
pgdumpx extract <FILE> <SCHEMA.TABLE>
```

Write the selected entry's **decompressed table-data body** to stdout as binary-safe bytes.

The command must not imply that output is a complete restorable SQL script. It does not add schema DDL, a `COPY` statement wrapper, or unrelated entries.

The command must use the library's bounded raw extraction path. A configured limit is an error boundary; output must not be silently reported as successful truncation.

### CLI-004 — Find first matching field value

```text
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

The command resolves the column through the same COPY metadata path as the library and returns the first row whose selected logical field bytes equal the UTF-8 bytes of `<VALUE>`.

v0.1 command-line schema, table, column, and value arguments are UTF-8. The Rust API remains byte-oriented. Arbitrary byte-literal CLI input is deferred.

The command uses the core streaming `find_first` path rather than implementing a separate parser.

The command is intentionally narrow. A SQL `WHERE` parser or general condition DSL is not required.

The CLI should expose practical scan-budget options, including maximum rows and/or maximum decompressed bytes.

### CLI-005 — Exit behavior

Stable behavior:

```text
0  find matched a row; other commands completed successfully
1  find completed successfully but no row matched
2+ usage, I/O, format, integrity, decompression, COPY, encoding, unknown-column, unsupported-representation, or resource error
```

Exact subdivision of error codes `2+` may evolve, but no-match must remain distinct from parser/runtime failure.

Diagnostics go to stderr. Binary `extract` output goes to stdout without diagnostic contamination.

### CLI-006 — Shared implementation

All archive parsing, entry streaming, COPY parsing, column lookup, limit accounting, and first-match behavior must delegate to the public or intended-public library path. The CLI must not maintain a second parser.

## 6. Error requirements

The public error type must be typed, implement `std::error::Error`, and be `#[non_exhaustive]` before v1.0.

Expected categories include:

- I/O;
- unexpected EOF;
- invalid magic;
- unsupported archive version;
- unexpected archive format;
- invalid integer or offset encoding;
- invalid metadata length;
- malformed TOC entry;
- invalid data offset;
- invalid block type;
- dump-id mismatch;
- unsupported compression;
- decompression failure;
- malformed COPY row/escape;
- malformed or unavailable COPY column metadata;
- unsupported table-data representation;
- unknown requested column where a convenience API requires one;
- resource limit exceeded, including structural, scan, and raw extraction budgets;
- arithmetic overflow;
- UTF-8 conversion failure for explicit string accessors or CLI input handling.

Errors should include byte offsets, dump IDs, object context, row numbers, consumed work, or limit context where meaningful.

A limit-aware `Read` adapter must preserve the typed pgdumpx limit error as an error source so high-level APIs can map it back to `PgDumpError`.

## 7. Resource and safety requirements

### SAFE-001 — No whole-archive allocation

Normal archive opening must not allocate a buffer proportional to the complete archive file size.

### SAFE-002 — No whole-table allocation for streaming reads

`EntryDataReader`, row iteration, first-match filtering, and CLI extraction must not require a complete data entry in memory.

### SAFE-003 — Checked arithmetic

All offsets, encoded lengths, counters, and conversions influenced by input must use checked operations.

### SAFE-004 — Bounded strings

Archive string decoding must enforce `max_string_bytes` or an equivalent configurable budget before allocation.

### SAFE-005 — Bounded TOC metadata

The parser must enforce configurable limits for TOC entry count and dependency count.

### SAFE-006 — Bounded COPY rows

Row buffering must enforce configurable maximum row bytes and field count.

### SAFE-007 — Malformed input does not panic

Structurally malformed input must return `Ok` or `Err`; it must not cause a parser panic.

### SAFE-008 — `unsafe` policy

The initial implementation uses no project-authored `unsafe`. Introducing it requires an accepted ADR explaining invariants, alternatives, and verification.

### SAFE-009 — Decompression resource awareness

The API and documentation must distinguish individual allocation limits from total row-scan work and raw decompressed output.

### SAFE-010 — First-match result allocation

Creating an `OwnedRow` for a match copies only the matched row and remains bounded by configured row and field limits.

### SAFE-011 — Scan budget enforcement

Configured scan budgets are checked on the normal streaming path and terminate with a typed error when exceeded. A scan budget is not permission to pre-decompress or pre-count the complete entry.

### SAFE-012 — Raw extraction budget enforcement

The library and CLI provide a bounded raw extraction path. Crossing the configured decompressed-byte limit returns an error and is distinguishable from normal EOF.

### SAFE-013 — Dependency boundary disclosure

The default build requires no PostgreSQL runtime component. Compression or optional dependencies that introduce material native build/runtime constraints must be documented and, where practical, feature-gated.

## 8. Compatibility requirements

Primary compatibility source is PostgreSQL upstream behavior for supported archive versions.

Relevant upstream files include:

- `src/bin/pg_dump/pg_backup_archiver.h`;
- `src/bin/pg_dump/pg_backup_archiver.c`;
- `src/bin/pg_dump/pg_backup_custom.c`;
- `src/bin/pg_dump/compress_io.c`.

Reference fixtures must be generated by official PostgreSQL `pg_dump`, not invented solely from reverse-engineered assumptions.

Known version conditions that affect supported parsing must be recorded in `docs/PG-DUMP-CUSTOM-FORMAT.md` and tests.

`docs/COMPATIBILITY.md` is the public matrix separating intended support from fixture-verified support. A combination must not be described as verified until production-path tests support that claim.

Every valid-format fixture must record provenance equivalent in purpose to:

- stable fixture name/path;
- archive format version;
- exact `pg_dump` generator version;
- exact generation command;
- checksum;
- fixture purpose;
- expected schemas/tables or entries.

Malformed hand-built fixtures are allowed for invalid states that official tools cannot generate, but they must not be the sole evidence for valid behavior.

## 9. Testing requirements

The test suite should cover at least:

- invalid/truncated `PGDMP` magic;
- versions 1.14, 1.15, and 1.16;
- unsupported older/newer versions;
- varying integer and offset sizes supported by upstream behavior;
- truncated archive metadata;
- zero and multiple TOC entries;
- large/invalid string lengths;
- dependency-list limits;
- no-data, unset-offset, and set-offset states;
- invalid/overflowing data offsets;
- wrong block type at a stored offset;
- dump-id mismatch;
- uncompressed entry streaming;
- gzip/LZ4/Zstandard entry streaming;
- short reads across framing and decoder boundaries;
- bounded raw entry extraction at below/exactly/above-limit sizes;
- raw limit exhaustion distinguishable from EOF and not reported as successful truncation;
- table/table-data lookup;
- COPY NULL, empty string, tabs, escapes, embedded escaped control characters, and terminator;
- non-UTF-8 field bytes;
- row-size and field-count limits;
- malformed COPY escapes;
- supported COPY column-list parsing;
- byte-oriented column-name lookup;
- valid metadata + column present;
- valid metadata + column absent;
- unavailable/malformed column metadata;
- INSERT-based representation rejected explicitly by row APIs;
- borrowed row invalidation/lifetime behavior documented through compile-time/API tests where practical;
- first-row first-match;
- middle/late first-match;
- no-match result;
- multiple matching rows returning the first only;
- early stop after the first match using an instrumented reader;
- owned matched row surviving reader drop;
- non-UTF-8 predicate values;
- first-match behavior under row/field limits;
- max-row budget below/exactly/above the boundary;
- max-decompressed-byte budget below/exactly/above the boundary;
- parser-consumed byte accounting independent of decoder read-ahead buffer size;
- CLI `extract` binary stdout and stderr separation;
- CLI UTF-8 input boundary;
- CLI exit 0/1/2+ behavior;
- arbitrary-input no-panic fuzzing for metadata, framing, limits, and COPY parsing;
- fixture comparison with `pg_restore` where practical;
- fixture manifest entries matching checksums and generator metadata.

## 10. Performance requirements

v0.1 must include a benchmark harness capable of measuring:

- archive open / TOC parsing;
- peak memory for archive open;
- selected-entry extraction throughput;
- peak memory for extraction;
- COPY parsing throughput;
- first-match scan with matches near the beginning, middle, end, and absent;
- none/gzip/LZ4/Zstandard throughput;
- raw extraction limit-accounting overhead;
- row-scan budget-accounting overhead.

Benchmark reports used for public claims must record:

- hardware and operating system;
- exact commit/release;
- fixture or reproducible dataset generator;
- archive version and compression;
- command/API path;
- match position when applicable;
- measurement tool and repetition/warm-up method.

Comparisons with `pg_restore` or another library must use operations that are meaningfully equivalent and state material semantic differences.

Performance optimizations must preserve tests and safety invariants.

## 11. Definition of done for v0.1

v0.1 is ready when:

- official fixtures for supported archive versions open through the public parser path;
- `docs/COMPATIBILITY.md` marks only tested combinations as verified;
- fixture provenance and checksums are recorded;
- metadata inspection does not read all payloads;
- a selected table-data entry can be validated, streamed, and decompressed;
- bounded raw extraction is available to library and CLI callers;
- supported COPY text data can be iterated as rows/fields;
- `FieldRef::Bytes` matches the documented post-escape-decoding contract;
- borrowed row lifetime semantics are documented and do not require per-row ownership;
- supported COPY column metadata can be resolved;
- missing columns are distinguishable from unavailable/malformed metadata;
- unsupported INSERT-based row representations fail explicitly;
- a caller can return the first row matching a Rust predicate without buffering the table;
- the returned first-match row remains valid after reader teardown;
- row scans can be bounded by configurable total-work budgets with exact documented accounting;
- raw output can be bounded by decompressed bytes without successful silent truncation;
- first-match documentation states sequential-scan complexity;
- resource budgets are enforced with typed errors;
- malformed parser input has broad boundary/fuzz coverage;
- CLI `inspect`, `list`, `extract`, and `find` consume the same library path;
- `extract` output and `find` UTF-8/exit contracts match the documentation;
- the default build requires no PostgreSQL server, `libpq`, or `pg_restore` runtime;
- `docs/COPY-TEXT.md`, API documentation, roadmap, and compatibility documentation match tested behavior;
- CI passes on supported platforms;
- public APIs have rustdoc documentation;
- benchmark methodology is documented;
- README claims match measured and fixture-backed behavior;
- licensing metadata and files use `MIT OR Apache-2.0`.
