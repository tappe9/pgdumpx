# pgdumpx Roadmap

This roadmap is directional. Milestones may change when format research, compatibility fixtures, fuzzing, and benchmark results reveal better boundaries.

## v0.1 — Safe, row-aware custom-archive reader

Goal: deliver a useful Pure Rust reader for modern PostgreSQL custom-format (`-Fc`) archives, including streaming row access and first-match retrieval without restore.

Planned scope:

- Cargo workspace and crate boundaries;
- checked binary I/O primitives;
- `PGDMP` header parsing;
- archive versions 1.14, 1.15, and 1.16;
- archive metadata parsing;
- TOC entry parsing and typed object metadata;
- custom-format per-entry data offsets;
- lazy seek to individual data entries;
- data block type and dump-id validation;
- streaming entry readers;
- none/gzip/LZ4/Zstandard decompression;
- table ↔ table-data relationship lookup;
- PostgreSQL COPY text row parser;
- byte-oriented row/field API;
- supported COPY column-layout parsing from TOC metadata;
- byte-oriented column-name lookup;
- streaming `find_first` predicate scan;
- owned result row for first-match retrieval;
- explicit documentation that row lookup is a sequential scan within the selected table;
- configurable resource limits;
- typed errors;
- `pgdumpx inspect`, `list`, and `extract` CLI commands;
- reference-generated fixtures;
- malformed-input tests;
- fuzz targets;
- benchmark harness, including early/middle/late/no-match row scans;
- CI for formatting, linting, tests, docs, and supported platforms;
- `MIT OR Apache-2.0` licensing.

Explicitly not included:

- archive writing;
- restoring into PostgreSQL;
- Directory or Tar archive formats;
- SQL `WHERE` parsing or a condition DSL;
- persistent/sidecar row indexes;
- constant-time or logarithmic row lookup guarantees;
- Binary COPY decoding;
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
- documented resource-limit behavior;
- documented row-search complexity and first-match semantics;
- reproducible benchmark methodology;
- mature diagnostics;
- published crate and CLI release artifacts.

## Guiding rule

Prefer a small, excellent read/extract/query engine for PostgreSQL Custom Format over becoming a second implementation of all `pg_dump`/`pg_restore` behavior.
