# pgdumpx Roadmap

This roadmap is directional. Delivery slices may change when upstream format research, official fixtures, fuzzing, and benchmark results reveal better boundaries.

## v0.1 — Bounded row scanning for Custom Format archives

Goal: deliver a useful Rust library and CLI that can select one table from a modern PostgreSQL custom-format (`-Fc`) archive, stream its COPY text rows, and stop when a requested row is found—without restoring PostgreSQL or buffering the complete table.

The final v0.1 scope still covers archive versions 1.14–1.16 and none/gzip/LZ4/Zstandard. The implementation order is vertical: prove the complete row-search user story on a narrow compatibility slice first, then broaden compatibility and hardening.

## Alpha 1 — First end-to-end `find` slice

Target compatibility:

```text
archive version: 1.16
compression:     none and gzip
source:          seekable Read + Seek
row format:      normal pg_dump COPY text
```

Deliver:

- Cargo workspace with `pgdumpx` and `pgdumpx-cli` crates;
- CI for formatting, linting, tests, and rustdoc;
- checked primitive reader and foundational typed errors;
- `PGDMP` magic and archive 1.16 header parsing;
- minimum TOC metadata required for table/table-data lookup;
- validated seek to one selected data entry;
- custom chunk framing and streaming none/gzip decompression;
- COPY text row framing, NULL handling, and escape decoding;
- supported COPY column-list parsing from TOC metadata;
- byte-oriented row and field access;
- streaming `find_first` with an owned matched row;
- a narrow CLI path:

```text
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Exit criteria:

- an official PostgreSQL-generated archive 1.16 fixture is opened through the public library path;
- `public.orders` or an equivalent fixture table is located through TOC metadata;
- an early, middle, late, and absent value can be tested through the same `find_first` path;
- no complete table-data entry is buffered;
- `find` distinguishes match, no match, and parser/runtime failure;
- documentation does not claim compatibility beyond the fixture evidence.

This alpha is intentionally narrow. Its purpose is to demonstrate the product value before implementing every archive-version and compression branch.

## Alpha 2 — Complete row API and resource semantics

Deliver:

- complete structural `Limits` behavior for TOC entries, strings, dependencies, row bytes, and field count;
- explicit distinction between valid missing columns and unavailable/malformed column metadata;
- positional row iteration when row bytes are readable but column metadata cannot be derived;
- explicit rejection of INSERT-based and Binary COPY representations from row APIs;
- exact `ScanLimits` accounting semantics;
- raw entry extraction with a library-provided decompressed-byte limit;
- stable CLI contracts for:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract <FILE> <SCHEMA.TABLE>
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

- binary-safe `extract` output of the selected decompressed table-data body;
- documented UTF-8 CLI argument boundary while the Rust row API remains byte-oriented;
- typed, location-aware errors for malformed metadata, block identity, decompression, COPY data, representation, and resource limits.

Exit criteria:

- configured row and decompressed-byte budgets stop on the production streaming path;
- the row that would cross a configured budget is not yielded;
- raw extraction fails on limit exhaustion rather than silently truncating output;
- borrowed rows remain valid only until the next mutable reader operation;
- matched `OwnedRow` values survive reader teardown;
- CLI output and exit-code behavior is covered by integration tests.

## Alpha 3 — Compatibility expansion

Deliver:

- archive version 1.14 parsing, including its compression representation;
- archive version 1.15 parsing and explicit compression metadata;
- LZ4 and Zstandard streaming decoders;
- additional TOC fields required by the supported versions;
- fixture provenance manifest containing generator version, exact command, checksum, purpose, and expected objects;
- differential checks against `pg_restore` output where practical;
- compatibility matrix updates backed only by fixtures that pass through production code paths.

Exit criteria:

- each target archive-version/compression combination is either verified with recorded evidence or remains clearly marked unverified;
- short reads across archive chunks and decoder boundaries are tested;
- unsupported older/newer archive versions fail explicitly;
- no target is advertised as “PostgreSQL X supported” solely from server-version naming.

## Beta — Hardening and performance evidence

Deliver:

- malformed-input regression corpus;
- fuzz targets for archive metadata, chunk framing, COPY escapes, column metadata, and limit accounting;
- benchmark harness and reproducible dataset generation;
- peak-memory and throughput measurements;
- first-match benchmarks at early/middle/late/absent positions;
- comparison methodology for `pg_restore`, adjacent libraries, and pgdumpx paths when the operations are meaningfully equivalent;
- cross-platform CI and release packaging;
- public rustdoc coverage;
- dependency/license verification for `MIT OR Apache-2.0`.

Exit criteria:

- no known malformed-input parser panic within tested/fuzzed boundaries;
- benchmark reports state exact hardware, fixture, command, compression, and measurement method;
- README performance statements are based on reproducible evidence;
- all v0.1 Definition of Done items in `docs/REQUIREMENTS.md` are satisfied.

## v0.1 release scope summary

v0.1 includes:

- archive versions 1.14, 1.15, and 1.16;
- metadata/TOC inspection;
- selective table-data lookup and validated seeking;
- streaming none/gzip/LZ4/Zstandard decompression;
- bounded raw entry extraction;
- normal pg_dump COPY text row parsing;
- byte-oriented borrowed rows and fields;
- COPY column metadata with explicit error semantics;
- streaming first-match filtering with an owned result;
- structural, row-scan, and raw-extraction limits;
- `inspect`, `list`, `extract`, and `find` CLI commands;
- fixture provenance, fuzzing, benchmarks, CI, and compatibility documentation.

Explicitly not included:

- archive writing;
- restoring into PostgreSQL;
- Directory or Tar archive formats;
- SQL `WHERE` parsing or a condition DSL;
- persistent/sidecar row indexes;
- constant-time or logarithmic row lookup guarantees;
- Binary COPY decoding;
- INSERT statement row parsing;
- Arrow/Parquet/DataFrame integrations;
- Python bindings;
- guaranteed parallel extraction.

## CLI contract for v0.1

### `extract`

```text
pgdumpx extract <FILE> <SCHEMA.TABLE>
```

Writes the selected entry's **decompressed table-data body** to stdout as binary-safe bytes. It does not add schema DDL, a `COPY` statement wrapper, or a complete restorable SQL script.

### `find`

```text
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Uses UTF-8 command-line arguments in v0.1, resolves the column through recorded COPY metadata, and compares the supplied value with logical post-unescape field bytes. A future byte-literal input mode requires a separate CLI design.

Stable exit behavior:

```text
0  match found
1  no matching row
2+ usage, I/O, format, integrity, decompression, COPY, encoding, or resource error
```

Exact non-zero error-code subdivision may evolve, but no-match remains distinct from failure.

## v0.2 — Extraction performance and ergonomics

Candidate scope:

- file-oriented convenience APIs;
- reusable extraction plans/selectors;
- additional equality or typed-value helpers if real usage justifies them;
- efficient multi-table extraction;
- optional parallel extraction using independently seekable file handles;
- buffer-size tuning from benchmark evidence;
- richer filtering by schema/object type/name;
- research into optional sidecar indexes or decompression restart points for repeated row queries, without assuming arbitrary row seek inside compressed entries.

## v0.3 — Data ecosystem integrations

Candidate companion crates or optional features:

- CSV output;
- JSON Lines output;
- Apache Arrow integration;
- Polars integration;
- Parquet export.

These integrations consume the core row stream and must not move DataFrame dependencies into the mandatory parser core.

## v0.4 — Optional format expansion

Only if demonstrated demand exists, evaluate PostgreSQL Directory Format (`pg_dump -Fd`) or other archive formats behind the same conceptual archive/entry API where semantics genuinely align.

Custom Format remains the primary specialization; broad format coverage is not a success criterion by itself.

## v0.5 — Language bindings

Candidate scope:

- PyO3-based Python package;
- Python iteration over archive metadata and table rows;
- first-match queries using Python callables or narrowly scoped filters;
- wheels for common platforms;
- optional Arrow handoff for analytical workloads.

## v0.6 — Broader archive compatibility

Candidate scope only if real-world demand exists:

- archive versions older than 1.14;
- additional COPY/data representations discovered in real archives.

## v1.0 — Stable read API

Potential criteria:

- documented stable Rust API;
- explicit fixture-backed archive-version compatibility matrix;
- extensive reference corpus;
- robust fuzz coverage;
- no known malformed-input parser panic;
- documented structural, scan-work, and raw-extraction limits;
- documented row-search complexity and first-match semantics;
- reproducible benchmark methodology;
- mature diagnostics;
- published crate and CLI release artifacts.

## Guiding rule

Prefer a small, excellent read/extract/query engine for PostgreSQL Custom Format over becoming a second implementation of all `pg_dump`/`pg_restore` behavior.
