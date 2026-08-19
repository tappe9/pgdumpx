# pgdumpx Roadmap

This roadmap is directional. Milestones may change when format research, compatibility fixtures, fuzzing, and benchmark results reveal better boundaries.

## v0.1 — Safe custom-archive reader

Goal: deliver a useful Pure Rust reader for modern PostgreSQL custom-format (`-Fc`) archives.

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
- configurable resource limits;
- typed errors;
- `pgdumpx inspect`, `list`, and `extract` CLI commands;
- reference-generated fixtures;
- malformed-input tests;
- fuzz targets;
- benchmark harness;
- CI for formatting, linting, tests, docs, and supported platforms;
- `MIT OR Apache-2.0` licensing.

Explicitly not included:

- archive writing;
- restoring into PostgreSQL;
- Directory or Tar archive formats;
- Binary COPY decoding;
- Arrow/Parquet/DataFrame integrations;
- Python bindings;
- guaranteed parallel extraction.

## v0.2 — Extraction performance and ergonomics

Candidate scope:

- file-oriented convenience API;
- reusable extraction plans / selectors;
- efficient multi-table extraction;
- optional parallel extraction using independently seekable file handles;
- buffer-size tuning from benchmark evidence;
- richer filtering by schema/object type/name;
- stable benchmark corpus and published performance methodology.

## v0.3 — Data ecosystem integrations

Candidate companion crates/features:

- CSV output;
- JSON Lines output;
- Apache Arrow integration;
- Polars integration;
- Parquet export.

These integrations should consume the same core row stream and must not move DataFrame dependencies into the mandatory parser core.

## v0.4 — Directory format

Add PostgreSQL Directory Format (`pg_dump -Fd`) behind the same conceptual archive/entry API where semantics genuinely align.

Do not force a misleading common abstraction solely to make formats look uniform.

## v0.5 — Language bindings

Candidate scope:

- PyO3-based Python package;
- Python iteration over archive metadata and table rows;
- wheels for common platforms;
- optional Arrow handoff for analytical workloads.

## v0.6 — Broader archive compatibility

Candidate scope:

- archive versions older than 1.14;
- Tar Format (`-Ft`) if there is demonstrated demand;
- additional COPY/data representations discovered in real-world archives.

## v1.0 — Stable read API

Potential criteria:

- documented stable Rust API;
- explicit archive-version compatibility matrix;
- extensive reference fixture corpus;
- robust fuzz coverage;
- no known malformed-input parser panic;
- documented resource-limit behavior;
- reproducible benchmark methodology;
- mature diagnostics;
- published crate and CLI release artifacts.

## Guiding rule

Prefer a small, excellent read/extract engine over becoming a second implementation of all `pg_dump`/`pg_restore` behavior.
