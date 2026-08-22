# pgdumpx Requirements

Status: **Accepted v0.1 contract; implementation audited against the Definition of Done**

This document defines the normative functional, safety, compatibility, and quality contract for pgdumpx v0.1. Requirement language remains authoritative even after implementation; concrete completion evidence is mapped separately in `V0.1-RELEASE-AUDIT.md` rather than weakening requirements after the fact.

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

The v0.1 delivery sequence was required to prioritize a narrow archive 1.16 + none/gzip path that reached COPY rows, column lookup, `find_first`, and `pgdumpx find` before broad compatibility expansion.

This was a delivery-order requirement, not a reduction of the final v0.1 compatibility target.

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

### FR-001 — Validate custom-format magic

The parser must validate the `PGDMP` magic before interpreting archive fields.

### FR-002 — Parse supported archive versions explicitly

The parser must recognize archive-format versions 1.14, 1.15, and 1.16 with version-specific branches where PostgreSQL behavior differs.

Unsupported older or newer versions must return a typed error.

### FR-003 — Decode checked archive primitives

Integers, offsets, strings, timestamps, and counts must be decoded with checked arithmetic and explicit handling of representation sizes recorded by the archive.

### FR-004 — Parse TOC metadata eagerly

`Archive::open` must decode TOC metadata and build lookup structures without reading all table-data payloads.

### FR-005 — Preserve byte-oriented metadata

Archive strings must be preservable without assuming UTF-8. UTF-8 conversion must be explicit and fallible.

### FR-006 — Model data-location states

The public or stable internal model must distinguish at least:

- no data;
- data position not available;
- valid stored data offset.

### FR-007 — Validate selected-entry identity after seek

Before reading payload data after a stored-offset seek, pgdumpx must validate the expected block type and dump ID.

### FR-008 — Stream selected entry data

Selected entry bodies must be exposed incrementally without buffering the complete decompressed entry.

### FR-009 — Support v0.1 compression algorithms

The final v0.1 implementation must support:

- none;
- gzip;
- LZ4;
- Zstandard.

The exact backend implementation is private.

### FR-010 — Parse supported COPY text rows

For supported table-data entries, pgdumpx must parse PostgreSQL COPY text record and field boundaries across arbitrary reader segmentation.

### FR-011 — Decode COPY logical field bytes

`FieldRef::Bytes` must expose logical field bytes after COPY text escape decoding.

`\N` must be represented distinctly as NULL. An empty non-NULL field must remain an empty byte string.

### FR-012 — Extract supported column metadata

For normal pg_dump-generated COPY table data, pgdumpx must derive ordered column metadata from the TOC's recorded COPY statement when it is in the supported form.

### FR-013 — Distinguish missing columns from metadata failures

Column lookup must distinguish:

- requested name found;
- valid metadata but requested name absent;
- column metadata unavailable or malformed.

### FR-014 — Provide lending row iteration

The primary row iteration path must be allowed to reuse an internal row buffer. Borrowed rows/fields must not be documented as surviving the next mutable row-reader operation.

### FR-015 — Provide first-match filtering

The library must provide a streaming first-match operation that:

- evaluates rows in COPY order;
- stops on the first matching row;
- returns an owned row that survives reader teardown;
- does not buffer all prior rows or the complete table.

### FR-016 — Make first-match complexity explicit

Documentation must state that first-match filtering is a sequential scan inside the selected table-data entry. It must not imply database-index semantics.

### FR-017 — Provide structural resource limits

Callers must be able to bound at least:

- TOC entry count;
- archive metadata string bytes;
- dependencies per TOC entry;
- row bytes;
- fields per row.

Default structural limits must be finite.

### FR-018 — Provide total scan-work limits

Callers must be able to bound total operation work by at least:

- complete rows scanned/evaluated;
- decompressed COPY bytes consumed by the row parser.

These limits are distinct from per-row structural limits.

### FR-019 — Define exact scan accounting

For a configured scan budget:

- `max_rows = N` permits at most `N` complete rows to be exposed/evaluated;
- the matching row counts;
- a crossing row is not exposed to the caller/predicate;
- decompressed-byte accounting measures parser-consumed physical COPY bytes;
- separators, row terminators, escape spellings, and the COPY terminator count when consumed;
- unread decompressor/`BufRead` lookahead does not count;
- logical decoded field length does not replace physical-byte accounting;
- counters use checked arithmetic;
- exhaustion produces a typed error.

### FR-020 — Provide bounded raw extraction

The library must provide a high-level path to copy or read one selected decompressed entry under a caller-supplied decompressed-byte budget.

Crossing that budget must fail rather than appear as successful truncation. The byte count must cover bytes exposed to the caller or written to the destination.

### FR-021 — Detect unsupported table-data representations

Row-aware APIs must reject unsupported logical representations explicitly, including INSERT-based pg_dump output and Binary COPY where detectable from available metadata.

Readable raw archive data must not automatically be treated as COPY text.

### FR-022 — Provide metadata-only CLI inspection

`pgdumpx inspect` and `pgdumpx list` must use metadata parsed by the library and must not require table-data decompression.

### FR-023 — Provide bounded CLI extraction

`pgdumpx extract` must:

- select one table-data entry through library metadata;
- write only its decompressed entry body to stdout as binary-safe bytes;
- use a finite default raw-output limit;
- accept an explicit positive `u64` byte-limit override;
- return failure on limit exhaustion;
- document that already-written bytes cannot be rolled back after a later limit or writer failure;
- keep diagnostics on stderr.

### FR-024 — Provide first-match CLI search

`pgdumpx find` must:

- accept `<FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>` plus optional scan limits;
- use the library column metadata and row-search path;
- compare UTF-8 CLI value bytes against logical post-unescape field bytes;
- emit one deterministic normalized COPY-text row when matched;
- emit no stdout for no match;
- return exit 0 for match, exit 1 for completed no-match, and exit 2+ for usage/runtime/resource failure;
- keep diagnostics on stderr.

### FR-025 — Enforce exact CLI table-selector grammar

For v0.1, `<SCHEMA.TABLE>` must contain exactly one ASCII `.` separator and both components must be non-empty. SQL identifier quoting/escaping and identifiers containing `.` are not supported by the CLI.

## 5. Safety requirements

### SAFE-001 — Treat all archive bytes as untrusted

Every length, count, offset, version gate, identifier, and compressed stream is attacker-controlled input.

### SAFE-002 — No project-authored unsafe by default

Project-authored Rust code must not use `unsafe` without a separately accepted ADR documenting the invariants and verification plan.

### SAFE-003 — Checked arithmetic

Parser arithmetic for offsets, lengths, counters, and allocation sizes must use checked operations/conversions.

### SAFE-004 — Bounded allocation

The parser must not allocate in direct proportion to an unvalidated attacker-controlled declared size without enforcing a configured or hard bound.

### SAFE-005 — No parser panic for malformed input

Within ordinary resource availability, structurally malformed input must return an error rather than panic.

This does not promise recovery from global allocator exhaustion or OS-level failures.

### SAFE-006 — Bound total work for hostile input

The public API must provide operation-level row/decompressed-byte budgets suitable for callers handling untrusted or customer-supplied archives.

### SAFE-007 — Bound raw decompression for hostile input

The library must provide a high-level raw extraction path with decompressed-byte accounting so callers handling untrusted archives do not need to reimplement the boundary around a low-level `Read` adapter.

## 6. Compatibility requirements

### COMP-001 — v0.1 archive-version range

The target archive-format versions are 1.14, 1.15, and 1.16.

### COMP-002 — Explicit version gates

Format behavior that differs by archive version must be represented by explicit branches and tests, not optimistic parsing.

### COMP-003 — Fixture evidence

Each verified valid-format compatibility cell must have official `pg_dump`-generated fixture evidence that passes through the public production path.

### COMP-004 — Fixture provenance

Each committed official fixture must record:

- `pg_dump` generator version;
- fixed generator image provenance/platform where containerized generation is used;
- exact generation command;
- archive-format version;
- compression configuration;
- checksum;
- purpose;
- expected objects/rows/layout used by tests.

### COMP-005 — Differential validation

Where operations are semantically equivalent, compatibility fixtures should be compared with `pg_restore` output in a reproducible check.

### COMP-006 — Do not overclaim support

README and compatibility documentation must not claim a PostgreSQL/archive/compression combination as verified without evidence.

## 7. CLI requirements

### CLI-001 — Stable commands

v0.1 includes:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

### CLI-002 — UTF-8 command-line boundary

Schema, table, column, and query value arguments supplied by the CLI must be valid UTF-8. The Rust library API remains byte-oriented.

### CLI-003 — Stdout/stderr discipline

Successful machine-consumable output must be written to stdout. Diagnostics must be written to stderr.

### CLI-004 — Extract output semantics

`extract` stdout is the decompressed body of the selected table-data entry, not a complete SQL restore script. Output is binary-safe.

### CLI-005 — Extract default and override

`extract` must use a finite default decompressed-byte budget and accept `--max-decompressed-bytes <N>` as a validated positive `u64` override before positional arguments.

### CLI-006 — Extract partial-output semantics

Because raw copying is streaming, bytes successfully written before a later limit, input, decompression, or destination failure cannot be rolled back. The process must still report failure and must not describe the partial bytes as a complete successful extraction.

### CLI-007 — Find output semantics

A successful match must emit exactly one deterministic normalized COPY-text record. No match must emit no stdout.

### CLI-008 — Find exit semantics

`find` must use:

```text
0  match found
1  completed scan with no matching row
2+ usage, I/O, archive, decompression, COPY, encoding,
   unsupported representation, unknown column, or resource failure
```

### CLI-009 — Find limit options

`--max-rows` and `--max-decompressed-bytes` must accept positive decimal `u64` values, appear before positional arguments, and be specified at most once each. Omitting a flag leaves that budget unlimited.

## 8. Public API requirements

### API-001 — Archive owns seek coordination

`Archive<R>` owns the `Read + Seek` source and coordinates mutable seeks. Public APIs must not allow independent simultaneous seekers over one underlying archive source by accident.

### API-002 — Byte-oriented metadata and row values

Metadata identities, COPY column names, and COPY field values must remain available as bytes without forcing UTF-8 conversion.

### API-003 — Public metadata encapsulation

Public metadata structs must keep fields private unless direct construction is a deliberate compatibility contract. Extensible public enums should be `#[non_exhaustive]` before v1.0.

### API-004 — Typed errors

Callers must be able to distinguish error categories without parsing `Display` strings.

### API-005 — Lending row semantics

Borrowed rows must be tied to the mutable row-reader lifetime. Public documentation must state that the next mutable reader operation invalidates the prior borrowed row.

### API-006 — Owned match semantics

First-match APIs must return an owned result that remains valid after reader advancement/teardown.

### API-007 — Independent limit types

Structural limits, row-scan work limits, and raw-output limits must be distinct concepts in the public API.

## 9. Testing requirements

### TEST-001 — Primitive and version tests

Checked archive primitive decoding and supported/unsupported version gates must have focused tests.

### TEST-002 — Official fixture tests

Every verified valid-format path must include official PostgreSQL-generated fixtures and production-path assertions.

### TEST-003 — Malformed-input regression coverage

Malformed input must have deterministic regression tests for important parser boundaries and every fuzz-discovered defect.

### TEST-004 — Streaming short-read coverage

Tests must exercise arbitrary short reads across archive chunk, decompressor, COPY record, and escape boundaries.

### TEST-005 — COPY semantics

Tests must cover NULL/empty distinction, escapes, control/numeric escapes, terminator behavior, non-UTF-8 bytes, row/field limits, and malformed escapes.

### TEST-006 — Column metadata

Tests must cover supported column layouts, absent requested names, unavailable/malformed metadata, and representation rejection.

### TEST-007 — First-match behavior

Tests must cover early/middle/late/absent, first-of-multiple, early termination, owned-row lifetime, non-UTF-8 values, and scan boundaries.

### TEST-008 — Raw extraction behavior

Tests must cover below/exactly/above decompressed-byte limits, no silent truncation, partial output on later failure where observable, and binary-safe data.

### TEST-009 — CLI integration

Integration tests must cover grammar, UTF-8 argument handling, stdout/stderr, extract binary output/limits/partial-output behavior, find normalized output, and exit 0/1/2+ semantics.

### TEST-010 — Fuzzing

Fuzz targets must cover archive opening/metadata, TOC parsing, selected-entry framing, COPY rows/escapes, COPY metadata, and limit-accounting boundaries. CI may use bounded smoke runs; longer coverage-guided campaigns remain separate evidence.

## 10. Quality and performance requirements

### QUAL-001 — Workspace quality gates

CI must run formatting, Clippy with warnings denied, full workspace tests, and rustdoc with warnings denied.

### QUAL-002 — Platform/toolchain coverage

CI must exercise the supported platform families and the declared MSRV.

### QUAL-003 — Feature coverage

CI must exercise the default CLI feature set and reduced library compression-feature configurations.

### QUAL-004 — Benchmark methodology

The repository must include a reproducible benchmark harness that records dataset/generator, exact commit, hardware/OS, compression, command/API path, match position where relevant, warm-up/repetition method, and measurement tool.

Ordinary pull-request CI must not present short/noisy benchmark smoke runs as performance evidence.

### QUAL-005 — Packaging verification

The intended publish packages must pass package-content, metadata, license, dependency-license, native/runtime, and non-publishing `cargo package` preflight checks.

### QUAL-006 — License

Project metadata and files must use `MIT OR Apache-2.0` consistently.

### QUAL-007 — Runtime independence

The default runtime dependency graph must not require PostgreSQL server/runtime components or PostgreSQL executables.

### QUAL-008 — Project-authored unsafe

Project-authored Rust code must remain free of `unsafe` unless a separately accepted ADR changes this policy.

## 11. Definition of Done for v0.1

v0.1 is ready for a release decision only when all of the following are true:

- [ ] official PostgreSQL-generated fixtures for every supported archive version open through the public parser path;
- [ ] `docs/COMPATIBILITY.md` distinguishes target support from fixture-verified support and marks only passing combinations verified;
- [ ] every committed valid-format fixture records generator provenance and checksum;
- [ ] metadata-only archive inspection does not require reading every entry payload;
- [ ] one selected table-data entry can be validated, streamed, and decompressed through the public production path;
- [ ] bounded raw selected-entry extraction is available to library and CLI callers;
- [ ] supported COPY text data can be iterated as rows and fields;
- [ ] `FieldRef::Bytes` exposes the documented logical post-escape-decoding bytes;
- [ ] borrowed row lifetime semantics are documented and do not require per-row ownership;
- [ ] supported COPY column metadata can be resolved from pg_dump metadata;
- [ ] missing columns are distinguishable from unavailable/malformed column metadata;
- [ ] unsupported INSERT-based row representations fail explicitly before COPY parsing;
- [ ] a caller can return the first row matching a Rust predicate without buffering the table;
- [ ] the returned first-match row remains valid after reader teardown;
- [ ] row scans can be bounded by configurable total-work budgets with exact documented accounting;
- [ ] raw output can be bounded by decompressed bytes without successful silent truncation;
- [ ] first-match documentation states sequential-scan complexity and avoids row-index implications;
- [ ] resource budgets are enforced with typed errors;
- [ ] malformed parser input has broad boundary/fuzz coverage;
- [ ] CLI `inspect`, `list`, `extract`, and `find` consume the same library path rather than duplicate parser logic;
- [ ] CLI `extract` output and `find` UTF-8/exit behavior match documented contracts;
- [ ] the default build requires no PostgreSQL server, `libpq`, or `pg_restore` runtime;
- [ ] `COPY-TEXT.md`, `API-DESIGN.md`, `ROADMAP.md`, and `COMPATIBILITY.md` match tested behavior;
- [ ] CI passes on supported platforms;
- [ ] public APIs have rustdoc documentation;
- [ ] benchmark methodology is documented;
- [ ] README claims match measured and fixture-backed behavior;
- [ ] project licensing metadata and files use `MIT OR Apache-2.0` consistently.

Completion evidence is recorded in `V0.1-RELEASE-AUDIT.md`; this normative checklist is intentionally not rewritten into self-certified checked boxes, so requirements and evidence remain separate.
