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

### G-004 — Safe untrusted-input handling

Malformed archive or COPY data must produce a typed error rather than unchecked memory access, arithmetic wraparound, or parser panic.

### G-005 — Reusable core

Archive behavior must not depend on terminal output, Python, Arrow, a PostgreSQL connection, or CLI-specific types.

### G-006 — Measurable performance

The project must include repeatable benchmarks before making comparative performance claims.

## 3. Non-goals for v0.1

v0.1 will not:

- write PostgreSQL dump archives;
- replace `pg_dump` or `pg_restore`;
- restore SQL into a database;
- execute SQL stored in the archive;
- support Directory (`-Fd`) or Tar (`-Ft`) formats;
- support arbitrary historical archive versions older than 1.14;
- guarantee Binary COPY decoding;
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

- tab-separated fields;
- newline record boundaries;
- `\N` NULL marker;
- backslash escapes used by COPY text;
- empty non-NULL fields;
- `\.` end-of-data marker when present in the stream representation;
- rows larger than the configured row limit;
- non-UTF-8 field bytes at the byte-oriented API layer.

### FR-015 — Byte-oriented field access

The core row API must allow fields to be consumed as bytes. String conversion is an explicit convenience operation, not a parser prerequisite.

### FR-016 — Metadata-only inspection

Callers must be able to enumerate header and TOC information without touching entry payloads.

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

Stream the selected table's decompressed COPY text data to stdout by default. Structured conversion options are deferred unless they naturally fit v0.1.

### CLI-004 — Exit behavior

I/O, format, decompression, COPY, or integrity errors must result in a non-zero exit code and diagnostics on stderr.

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
- resource limit exceeded;
- arithmetic overflow;
- UTF-8 conversion failure for explicit string accessors.

Errors should include byte offsets, dump IDs, or object context where meaningful.

## 7. Resource and safety requirements

### SAFE-001 — No whole-archive allocation

Normal archive opening must not allocate a buffer proportional to the complete archive file size.

### SAFE-002 — No whole-table allocation for streaming reads

`EntryDataReader` and row iteration must not require a complete data entry in memory.

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

The API and documentation must make clear which limits bound individual rows/metadata and where callers must bound total extracted output to defend against decompression bombs.

## 8. Compatibility requirements

Primary compatibility source is PostgreSQL upstream behavior for the supported archive versions.

Relevant upstream files include:

- `src/bin/pg_dump/pg_backup_archiver.h`;
- `src/bin/pg_dump/pg_backup_archiver.c`;
- `src/bin/pg_dump/pg_backup_custom.c`;
- `src/bin/pg_dump/compress_io.c`.

Reference fixtures should be generated by official PostgreSQL `pg_dump`, not invented solely from reverse-engineered assumptions.

Known version conditions that affect supported parsing must be recorded in `docs/PG-DUMP-CUSTOM-FORMAT.md` and tests.

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
- COPY NULL, empty string, tabs, escapes, embedded escaped newline, and terminator;
- non-UTF-8 field bytes;
- row-size and field-count limits;
- malformed COPY escapes;
- arbitrary-input no-panic fuzzing for metadata and COPY parsing;
- fixture comparison with `pg_restore` where practical.

## 10. Performance requirements

v0.1 must include a benchmark harness capable of measuring:

- archive open / TOC parsing;
- peak memory for archive open;
- selected-entry extraction throughput;
- peak memory for extraction;
- COPY parsing throughput;
- compression-specific throughput.

Performance optimizations must preserve tests and safety invariants.

## 11. Definition of done for v0.1

v0.1 is ready when:

- the supported archive versions open from reference-generated fixtures;
- metadata inspection does not read all payloads;
- a selected table-data entry can be streamed and decompressed;
- supported COPY text data can be iterated as rows/fields;
- resource budgets are enforced with typed errors;
- malformed parser input has broad boundary/fuzz coverage;
- CLI inspect/list/extract consume the same public library API;
- CI passes on supported platforms;
- public APIs have rustdoc documentation;
- benchmark methodology is documented;
- README claims match measured behavior;
- licensing metadata and files use `MIT OR Apache-2.0`.
