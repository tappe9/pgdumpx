# pgdumpx Roadmap

This roadmap is directional. Milestones may change when format research, compatibility fixtures, fuzzing, and benchmark results reveal better boundaries.

## v0.1 — Safe, row-aware custom-archive reader

Goal: deliver a useful Pure Rust reader for modern PostgreSQL custom-format (`-Fc`) archives, including streaming row access and first-match retrieval without restore.

The v0.1 feature set remains intentionally focused on Custom Format, but implementation is split into internal milestones so each stage can be developed and verified through TDD.

### M1 — Workspace and checked archive primitives

Deliver:

- Cargo workspace and crate boundaries;
- checked binary I/O primitives;
- foundational typed errors;
- structural `Limits` model;
- `PGDMP` magic validation;
- archive version, integer-size, offset-size, format, and compression metadata parsing;
- initial CI for formatting, linting, tests, and docs.

Exit criteria:

- malformed/truncated primitive inputs return typed errors rather than panics;
- supported header fixtures for archive versions 1.14–1.16 parse through public/archive-level code paths.

### M2 — TOC metadata, relationships, and lazy seek

Deliver:

- archive metadata parsing;
- TOC entry parsing and typed object metadata;
- custom-format per-entry data offsets;
- entry indexes by dump ID and practical `(schema, table)` lookup;
- table ↔ table-data relationship lookup;
- lazy seek to individual data entries;
- data block type and dump-ID validation.

Exit criteria:

- archive open touches metadata/TOC only;
- a selected table-data entry can be located without rescanning payload data;
- malformed offsets/block identities fail explicitly.

### M3 — Streaming entry data and compression

Deliver:

- custom chunk framing reader;
- `EntryDataReader` streaming decompressed bytes;
- none/gzip/LZ4/Zstandard decompression;
- short-read handling across archive chunks and decoder boundaries;
- compatibility fixtures for version-specific compression representation.

Exit criteria:

- selected entries stream without whole-entry buffering;
- decompressed output matches `pg_restore` or another upstream-grounded reference where practical;
- `docs/COMPATIBILITY.md` begins recording actually verified combinations.

### M4 — PostgreSQL COPY text row parser

Deliver:

- representation validation before row parsing;
- PostgreSQL COPY text row framing;
- NULL and empty-field handling;
- escape decoding;
- byte-oriented borrowed `Row` / `FieldRef` API;
- row-size and field-count limits;
- explicit `UnsupportedTableDataRepresentation` behavior for INSERT-based dump modes;
- tests matching `docs/COPY-TEXT.md`.

Exit criteria:

- `FieldRef::Bytes` exposes logical post-unescape bytes;
- non-UTF-8 field data is supported at the byte layer;
- `--inserts` / `--column-inserts` / INSERT output from `--rows-per-insert` is not misparsed as COPY.

### M5 — COPY column metadata and column lookup

Deliver:

- supported COPY column-layout parsing from TOC metadata;
- byte-oriented column-name lookup;
- clear distinction between missing column and unavailable/malformed column metadata.

Representative contract:

```text
Ok(Some(index))  -> valid layout, column found
Ok(None)         -> valid layout, column absent
Err(...)         -> layout unavailable/malformed
```

Exit criteria:

- column metadata is derived once per selected table-data entry;
- positional row iteration remains possible where valid even if column-aware helpers cannot be provided.

### M6 — First-match filtering and scan budgets

Deliver:

- streaming `find_first` predicate scan;
- owned `OwnedRow` result for a match;
- early termination at first match;
- operation-level `ScanLimits` equivalent to maximum rows and maximum decompressed bytes;
- checked scan-work accounting;
- first/middle/late/no-match benchmark cases.

Exit criteria:

- matched row survives reader teardown;
- nonmatching rows remain borrowed and allocation-conscious;
- configured scan budgets terminate work with typed errors;
- documentation clearly states sequential-scan complexity.

### M7 — CLI user story

Deliver:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract <FILE> <SCHEMA.TABLE>
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

The `find` command is a narrow equality-oriented consumer of the core column lookup and `find_first` API. It does not introduce SQL `WHERE` parsing or a general condition DSL.

Practical CLI scan-budget flags should map directly to the core scan-limit model.

Exit criteria:

- all CLI parser behavior delegates to the public Rust library;
- no-match behavior and parser/resource failures have documented distinct exit behavior.

### M8 — Hardening, compatibility evidence, and release quality

Deliver:

- reference-generated fixture corpus;
- malformed-input tests;
- fuzz targets;
- compatibility matrix backed by actual fixtures;
- benchmark harness and documented methodology;
- peak-memory and throughput measurements;
- public rustdoc coverage;
- `MIT OR Apache-2.0` packaging/license verification;
- release documentation whose claims match measured behavior.

Exit criteria:

- CI passes on supported platforms;
- no known parser panic for malformed input within tested/fuzzed boundaries;
- README compatibility/performance claims are evidence-based;
- all v0.1 Definition of Done items in `docs/REQUIREMENTS.md` are satisfied.

## v0.1 release scope summary

Across M1–M8, v0.1 includes:

- archive versions 1.14, 1.15, and 1.16;
- metadata/TOC inspection;
- selective entry seeking;
- streaming none/gzip/LZ4/Zstandard decompression;
- table/table-data lookup;
- COPY text row parsing;
- byte-oriented borrowed rows/fields;
- COPY column metadata with explicit error semantics;
- streaming first-match filtering with an owned result;
- structural resource limits and total scan-work budgets;
- `inspect`, `list`, `extract`, and `find` CLI commands;
- fixtures, fuzzing, benchmarks, CI, and compatibility documentation.

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

## v0.2 — Extraction performance and ergonomics

Candidate scope:

- file-oriented convenience API;
- reusable extraction plans / selectors;
- additional first-match/equality convenience APIs if real usage justifies them;
- efficient multi-table extraction;
- optional parallel extraction using independently seekable file handles;
- buffer-size tuning from benchmark evidence;
- richer filtering by schema/object type/name;
- stable benchmark corpus and published performance methodology;
- research into optional sidecar indexes or decompression restart points for repeated row queries, without assuming arbitrary row seek is possible inside compressed entries.

## v0.3 — Data ecosystem integrations

Candidate companion crates/features:

- CSV output;
- JSON Lines output;
- Apache Arrow integration;
- Polars integration;
- Parquet export.

These integrations should consume the same core row stream and must not move DataFrame dependencies into the mandatory parser core.

## v0.4 — Optional format expansion

Only if there is demonstrated demand, evaluate PostgreSQL Directory Format (`pg_dump -Fd`) or other archive formats behind the same conceptual archive/entry API where semantics genuinely align.

Custom Format remains the project's primary specialization; broad format coverage is not a success criterion by itself.

## v0.5 — Language bindings

Candidate scope:

- PyO3-based Python package;
- Python iteration over archive metadata and table rows;
- first-match row queries using Python callables or narrowly scoped convenience filters;
- wheels for common platforms;
- optional Arrow handoff for analytical workloads.

## v0.6 — Broader archive compatibility

Candidate scope only if real-world demand exists:

- archive versions older than 1.14;
- additional COPY/data representations discovered in real-world custom archives.

## v1.0 — Stable read API

Potential criteria:

- documented stable Rust API;
- explicit archive-version compatibility matrix;
- extensive reference fixture corpus;
- robust fuzz coverage;
- no known malformed-input parser panic;
- documented structural and scan-work resource-limit behavior;
- documented row-search complexity and first-match semantics;
- reproducible benchmark methodology;
- mature diagnostics;
- published crate and CLI release artifacts.

## Guiding rule

Prefer a small, excellent read/extract/query engine for PostgreSQL Custom Format over becoming a second implementation of all `pg_dump`/`pg_restore` behavior.
