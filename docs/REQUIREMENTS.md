# pgdumpx Requirements

Status: **Accepted for v0.1 implementation**

This document defines the functional, safety, compatibility, and quality contract for pgdumpx v0.1. It describes intended behavior, not existing implementation.

## 1. Product definition

pgdumpx is a read-only Pure Rust library for inspecting and selectively extracting data from PostgreSQL custom-format (`pg_dump -Fc`) archives.

The primary product is the reusable Rust library. The CLI is a separate consumer of that library.

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

### G-006 — Reusable core

Archive behavior must not depend on terminal output, Python, Arrow, a PostgreSQL connection, SQL execution, or a SQL query parser.

### G-007 — Measurable performance

The project must include repeatable benchmarks before making comparative performance claims.

### G-008 — Bounded scan work

The library must provide a way for applications to bound total row-scan/decompression work in addition to bounding individual metadata and row allocations.

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

v0.1 must support archive versions 1.14, 1.15, and 1.16.

A newer or older unsupported version must fail explicitly with `UnsupportedArchiveVersion` or equivalent. The parser must not guess a newer format's layout.

### FR-005 — Parse archive metadata

The parser must decode version-appropriate archive metadata required for inspection, including creation timestamp fields, database name, originating PostgreSQL server version string, and `pg_dump` version string where present in the supported versions.

### FR-006 — Parse TOC entries

The parser must parse the complete TOC metadata required to identify and relate objects, including at least:

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

### FR-007 — Build entry indexes

The archive must support efficient lookup by dump ID and practical table/table-data lookup by `(schema, name)` without rescanning payload data.

These indexes are entry-level. v0.1 does not require an index mapping field values to individual rows.

### FR-008 — Preserve raw metadata bytes where text encoding is not guaranteed

The lowest-level parser must not require arbitrary archive strings or table data to be valid UTF-8 merely for the archive to be structurally readable.

Ergonomic UTF-8 accessors may return conversion errors.

### FR-009 — Seek to entry data

For an entry whose stored offset state indicates a valid position, the library must seek with checked conversion/arithmetic and validate the block header found there.

### FR-010 — Validate data block identity

After seeking, the parser must verify the custom block type and encoded dump ID before exposing payload bytes. A mismatch is a typed integrity error.

### FR-011 — Stream entry data

The library must provide a `Read`-compatible streaming view of decompressed table-data bytes without buffering the complete entry.

### FR-012 — Compression support

v0.1 must support and test:

- none;
- gzip;
- LZ4;
- Zstandard.

The implementation must honor archive-version-specific compression representation rather than treating all versions identically.

### FR-013 — Table/table-data relationship

The public model must make it convenient to locate the table-data entry associated with a table identity where the archive contains that relationship.

### FR-014 — COPY text rows

The library must provide a row iterator/reader for supported table-data entries encoded in PostgreSQL COPY text form.

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

If table rows are positionally readable but the supported column layout cannot be derived, positional row iteration may remain available, while column-aware lookup/filtering must fail explicitly with a typed error rather than guessing field names.

### FR-018 — First-match predicate scan

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
- if no row matches, the result is `Ok(None)` after the selected table-data stream is exhausted;
- the operation must use the same streaming, decompression, parsing, and resource-limit path as normal row iteration;
- the operation must not allocate or buffer the complete table.

### FR-019 — Owned first-match result

A successful first-match operation must return an owned row representation whose fields remain valid after the streaming row reader advances or is dropped.

The owned result remains byte-oriented and is bounded by the same `max_row_bytes` and `max_fields_per_row` limits as the borrowed row representation.

### FR-020 — Explicit row-search performance semantics

The public documentation must state that custom archives provide table-data entry offsets, not row-level indexes.

Therefore first-match lookup is a sequential scan within the selected table:

- selecting the table-data entry uses parsed TOC/index metadata;
- row matching requires streaming decompression and row parsing from the beginning of that entry;
- a match may terminate early;
- a missing match or late match may require reading the complete selected table-data entry;
- worst-case work is proportional to the selected table's data size.

The project must not describe this API as database-index lookup or imply constant-time row access.

### FR-021 — Explicit unsupported table-data representation

Before row-aware parsing, pgdumpx must determine whether the selected entry is supported as COPY text from the available TOC/table-data metadata.

INSERT-based dump modes such as `--inserts`, `--column-inserts`, and INSERT output produced through `--rows-per-insert` must not be sent through the COPY text parser.

Row-aware access to such data must return a typed error equivalent to `UnsupportedTableDataRepresentation`. Raw entry extraction may remain available if the archive entry itself is readable.

### FR-022 — Scan work budgets

Long-running row operations must support configurable work budgets equivalent in purpose to:

```rust
pub struct ScanLimits {
    pub max_rows: Option<u64>,
    pub max_decompressed_bytes: Option<u64>,
}
```

Exact naming and defaults may evolve during TDD.

Requirements:

- row count is checked using overflow-safe accounting;
- decompressed-byte accounting applies to bytes consumed for the selected row-aware operation;
- exceeding a configured budget returns a typed resource-limit error;
- budget enforcement must not require buffering the complete entry;
- the library must expose this mechanism directly rather than requiring every caller to reimplement the scan loop.

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

Stream the selected table's decompressed entry data to stdout by default. Structured conversion options are deferred unless they naturally fit v0.1.

### CLI-004 — Find first matching field value

```text
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

The command must resolve the column through the same COPY column metadata path as the library and return the first row whose selected field equals the provided byte/string value according to the CLI's documented encoding rules.

It must use the core streaming `find_first` path rather than implementing a separate parser.

The command is intentionally narrow. A SQL `WHERE` parser or general condition DSL is not required for v0.1.

The CLI should expose practical scan-budget options once exact names are established, for example maximum rows and/or maximum decompressed bytes.

### CLI-005 — Exit behavior

I/O, format, decompression, COPY, integrity, unsupported-representation, unknown-column, or resource-limit errors must result in a non-zero exit code and diagnostics on stderr.

A no-match result for `find` must have documented, stable behavior that is distinguishable from parser failure.

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
- resource limit exceeded, including scan budgets;
- arithmetic overflow;
- UTF-8 conversion failure for explicit string accessors.

Errors should include byte offsets, dump IDs, object context, row numbers, or resource-limit context where meaningful.

## 7. Resource and safety requirements

### SAFE-001 — No whole-archive allocation

Normal archive opening must not allocate a buffer proportional to the complete archive file size.

### SAFE-002 — No whole-table allocation for streaming reads

`EntryDataReader`, row iteration, and first-match filtering must not require a complete data entry in memory.

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

The initial implementation uses no project-authored `unsafe`. Introducing it requires an accepted ADR explaining invariants and testing.

### SAFE-009 — Decompression resource awareness

The API and documentation must distinguish individual allocation limits from total scan/decompression work.

Callers processing untrusted input must have a supported way to bound total rows and/or decompressed bytes consumed by a row scan.

### SAFE-010 — First-match result allocation

Creating an `OwnedRow` for a match must copy only the matched row and must remain bounded by the configured row and field limits.

### SAFE-011 — Scan budget enforcement

Configured scan budgets must be checked on the normal streaming path and terminate processing with a typed error when exceeded. A scan budget is not permission to pre-decompress or pre-count the complete entry.

## 8. Compatibility requirements

Primary compatibility source is PostgreSQL upstream behavior for the supported archive versions.

Relevant upstream files include:

- `src/bin/pg_dump/pg_backup_archiver.h`;
- `src/bin/pg_dump/pg_backup_archiver.c`;
- `src/bin/pg_dump/pg_backup_custom.c`;
- `src/bin/pg_dump/compress_io.c`.

Reference fixtures should be generated by official PostgreSQL `pg_dump`, not invented solely from reverse-engineered assumptions.

Known version conditions that affect supported parsing must be recorded in `docs/PG-DUMP-CUSTOM-FORMAT.md` and tests.

`docs/COMPATIBILITY.md` is the public matrix separating intended support from fixture-verified support. A combination must not be described as verified until reference fixtures and tests support that claim.

## 9. Testing requirements

The test suite should cover at least:

- invalid/truncated `PGDMP` magic;
- versions 1.14, 1.15, and 1.16;
- unsupported older/newer versions;
- varying integer and offset sizes supported by the upstream format;
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
- short reads across framing boundaries;
- table/table-data lookup;
- COPY NULL, empty string, tabs, escapes, embedded escaped control characters, and terminator;
- non-UTF-8 field bytes;
- row-size and field-count limits;
- malformed COPY escapes;
- supported COPY column-list parsing;
- byte-oriented column-name lookup;
- metadata valid + column present;
- metadata valid + column absent;
- column metadata unavailable/malformed behavior;
- INSERT-based table-data representation rejected explicitly by row APIs;
- first-row first-match;
- middle/late first-match;
- no-match result;
- multiple matching rows returning the first only;
- early stop after the first match using an instrumented reader;
- owned matched row surviving reader drop;
- non-UTF-8 predicate values;
- first-match behavior under row/field resource limits;
- max-row scan budget exceeded;
- max-decompressed-byte scan budget exceeded;
- arbitrary-input no-panic fuzzing for metadata and COPY parsing;
- fixture comparison with `pg_restore` where practical.

## 10. Performance requirements

v0.1 must include a benchmark harness capable of measuring:

- archive open / TOC parsing;
- peak memory for archive open;
- selected-entry extraction throughput;
- peak memory for extraction;
- COPY parsing throughput;
- first-match scan with matches near the beginning, middle, end, and absent;
- compression-specific throughput;
- overhead of enabled scan-budget accounting.

Performance optimizations must preserve tests and safety invariants.

## 11. Definition of done for v0.1

v0.1 is ready when:

- the supported archive versions open from reference-generated fixtures;
- `docs/COMPATIBILITY.md` marks only actually tested combinations as verified;
- metadata inspection does not read all payloads;
- a selected table-data entry can be streamed and decompressed;
- supported COPY text data can be iterated as rows/fields;
- `FieldRef::Bytes` behavior matches the documented post-escape-decoding contract;
- supported COPY column metadata can be resolved for column-aware access;
- missing columns are distinguishable from unavailable/malformed column metadata;
- INSERT-based unsupported row representations fail explicitly rather than being misparsed as COPY;
- a caller can return the first row matching a Rust predicate without buffering the table;
- the returned first-match row is owned and remains valid after reader teardown;
- row scans can be bounded by configurable total-work budgets;
- first-match documentation explicitly states sequential-scan performance semantics;
- resource budgets are enforced with typed errors;
- malformed parser input has broad boundary/fuzz coverage;
- CLI `inspect`, `list`, `extract`, and `find` consume the same public library API;
- `docs/COPY-TEXT.md` and compatibility documentation match tested behavior;
- CI passes on supported platforms;
- public APIs have rustdoc documentation;
- benchmark methodology is documented;
- README claims match measured behavior;
- licensing metadata and files use `MIT OR Apache-2.0`.
