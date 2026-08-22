# pgdumpx Roadmap

Status: **v0.1 implementation and release-readiness complete; publication remains separate work.**

This roadmap records the delivered v0.1 vertical slices and possible later directions. The normative v0.1 contract remains in `docs/REQUIREMENTS.md`, final evidence is mapped in `docs/V0.1-RELEASE-AUDIT.md`, and GitHub Tracking Issue #30 records the implementation issue sequence.

## v0.1 — Bounded row scanning for Custom Format archives

Delivered goal: a Rust library and CLI that can select one table from a PostgreSQL custom-format (`-Fc`) archive, stream its COPY text rows, and stop when a requested row is found—without restoring PostgreSQL or buffering the complete table.

The implemented v0.1 scope covers archive versions 1.14–1.16 and none/gzip/LZ4/Zstandard. Delivery was intentionally vertical: first prove the complete row-search user story on a narrow compatibility slice, then complete resource semantics, broaden compatibility, harden untrusted-input handling, add benchmark/CI/rustdoc/packaging evidence, and finish the Definition of Done audit.

## Alpha 1 — First end-to-end `find` slice — completed

Historical target compatibility:

```text
archive version: 1.16
compression:     none and gzip
source:          seekable Read + Seek
row format:      normal pg_dump COPY text
```

Delivered:

- Cargo workspace with `pgdumpx` and `pgdumpx-cli` crates;
- CI for formatting, linting, tests, and rustdoc;
- official PostgreSQL-generated 1.16 none/gzip fixtures with provenance and checksums;
- checked primitive reader and foundational typed errors;
- finite metadata/row/field bounds on every production parser path;
- archive 1.16 header/TOC parsing and public `Archive::open`;
- dump-ID and table/table-data lookup indexes with relationship validation;
- validated seek to selected data entries;
- custom chunk framing and streaming none/gzip decompression;
- standalone COPY text parsing with NULL handling, escape decoding, borrowed rows, and byte-oriented fields;
- parser-consumed byte-accounting seam independent of decoder/read-ahead behavior;
- supported COPY column-list parsing from TOC metadata;
- explicit rejection of unsupported INSERT/Binary row representations;
- streaming `find_first` with an owned matched row;
- the first narrow `pgdumpx find` CLI path.

The v0.1 CLI selector grammar established here remains exact: one ASCII `.` separator with non-empty schema/table components, no SQL identifier quoting/escaping, and a byte-oriented Rust API behind the UTF-8 CLI boundary.

The slice's exit criteria were completed before later compatibility work: official 1.16 fixtures open through the public path, table lookup is unambiguous, none/gzip row streaming and early/middle/late/absent search pass, `OwnedRow` survives reader teardown, no complete table-data entry is buffered, and match/no-match/failure are distinct.

## Alpha 2 — Complete resource, error, and CLI semantics — completed

Delivered:

- public structural `Limits` with finite compatibility-oriented defaults for TOC entries, archive strings, dependencies, row bytes, and fields per row;
- contextual typed-error taxonomy and `std::error::Error::source` behavior;
- exact library `ScanLimits` accounting for complete rows and physical decompressed COPY bytes consumed by the parser;
- `pgdumpx find` scan options:

```text
--max-rows <N>
--max-decompressed-bytes <N>
```

- bounded raw entry extraction with `EntryReadLimits`;
- a finite **1,073,741,824-byte (1 GiB)** default for `pgdumpx extract` and an explicit positive-`u64` override;
- stable CLI commands:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

- binary-safe `extract` output of the selected decompressed table-data body;
- documented streaming partial-output behavior: bytes already written before a later limit/input/writer failure cannot be rolled back, but the command still reports non-success;
- documented UTF-8 CLI query boundary while the Rust archive/row API remains byte-oriented;
- typed, location-aware errors for malformed metadata, block identity, decompression, COPY data, representation, encoding, and resource limits.

The completed exit criteria include exact scan/raw boundary tests, non-success resource exhaustion, borrowed/owned lifetime coverage, stdout/stderr separation, and all four commands delegating to the library.

## Alpha 3 — Compatibility expansion — completed

Delivered:

- archive version 1.15 parsing with explicit compression metadata;
- archive version 1.14 parsing with its pre-1.15 compression representation;
- streaming LZ4 and Zstandard decoders;
- the complete v0.1 TOC metadata/accessor surface for archive versions 1.14–1.16;
- version-conditional absence rather than invented metadata defaults;
- official fixture provenance containing generator version, exact command, checksum, purpose, and expected objects;
- differential checks against `pg_restore` where operations are semantically equivalent;
- compatibility matrix updates backed only by production-path fixture evidence.

The default CLI enables none/gzip/LZ4/Zstandard. The library supports default, no-optional-compression, LZ4-only, and Zstandard-only configurations; a disabled backend remains recognizable in metadata and selected-entry reads fail with a typed backend-unavailable/unsupported-compression error. Backend-specific types do not leak into the public archive API.

The final evidence matrix remains intentionally narrower than “every backend on every version”; see `docs/COMPATIBILITY.md`.

## Beta — Hardening, performance evidence, and release readiness — completed

Delivered:

- malformed-input regression corpus;
- six bounded production-path fuzz targets for archive open/TOC, chunk framing, COPY rows, COPY metadata, and limit accounting;
- deterministic benchmark dataset generation and a production-path benchmark runner;
- peak-RSS and throughput methodology;
- first-match benchmark cases at early/middle/late/absent positions;
- comparison policy for `pg_restore` or adjacent libraries only when operations are meaningfully comparable;
- stable Linux/macOS/Windows and compression-feature-matrix CI;
- public rustdoc for lending lifetime, byte semantics, limits, sequential scan, typed errors, and raw partial-output behavior;
- package-content, metadata, license, native/runtime, and dependency verification;
- the final evidence-based audit of every v0.1 Definition of Done item and public documentation.

Ordinary CI compiles/smokes fuzz and benchmark targets but does not substitute short smoke runs for longer fuzz campaigns or performance measurements. The benchmark harness is reproducible separately, and the README publishes no quantitative throughput/latency/memory/speedup claim without a recorded result.

Beta exit criteria are satisfied when the final Issue #36 PR is green: no known malformed-input parser panic within the documented boundaries, supported platform/toolchain/feature jobs pass, default CLI packaging includes all v0.1 compression backends, warning-free rustdoc succeeds, package/license/runtime constraints pass the dedicated preflight, English/Japanese README claims agree with tested behavior, and `docs/V0.1-RELEASE-AUDIT.md` contains no unresolved production blocker.

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
- fixture provenance, fuzzing, benchmark methodology, CI, rustdoc, packaging, dependency/license verification, and compatibility documentation.

Explicitly not included:

- archive writing;
- restoring into PostgreSQL;
- Directory or Tar archive formats;
- SQL `WHERE` parsing or a condition DSL;
- SQL identifier quoting/escaping in v0.1 CLI table arguments;
- persistent/sidecar row indexes;
- constant-time or logarithmic row lookup guarantees;
- Binary COPY decoding;
- INSERT statement row parsing;
- synthesizing `TableRef` identities for standalone `TABLE DATA` entries without a corresponding normal `TABLE` entry;
- Arrow/Parquet/DataFrame integrations;
- Python bindings;
- guaranteed parallel extraction.

## CLI contract for v0.1

For all table-oriented commands, `<SCHEMA.TABLE>` contains exactly one ASCII `.` separator and non-empty UTF-8 schema/table components. SQL identifier quoting/escaping and identifiers containing `.` are not part of the v0.1 CLI grammar. The Rust library API remains byte-oriented.

### `extract`

```text
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
```

Writes the selected entry's **decompressed table-data body** to stdout as binary-safe bytes. It does not add schema DDL, a `COPY` statement wrapper, or a complete restorable SQL script.

The command uses a finite 1,073,741,824-byte (1 GiB) default and allows a positive decimal `u64` override before `<FILE>`. Limit exhaustion is a failure rather than successful truncation. Because output is streamed, bytes already written before a later limit/input/writer error cannot be rolled back; the command exits non-successfully and diagnostics remain on stderr.

### `find`

```text
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] \
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Uses UTF-8 command-line arguments, resolves the column through recorded COPY metadata, and compares the supplied value bytes with logical post-unescape field bytes. Scan-budget options delegate to the same library `ScanLimits` accounting path used by Rust callers. A future byte-literal input mode requires a separate CLI design.

Stable exit behavior:

```text
0  match found
1  no matching row
2+ usage, I/O, format, integrity, decompression, COPY, encoding,
   unsupported representation, unknown column, or resource error
```

No-match remains distinct from failure.

## v0.2 — Extraction performance and ergonomics

Candidate scope only; none of this is part of the v0.1 audit PR:

- file-oriented convenience APIs;
- reusable extraction plans/selectors;
- additional equality or typed-value helpers if real usage justifies them;
- efficient multi-table extraction;
- optional parallel extraction using independently seekable file handles;
- buffer-size tuning from benchmark evidence;
- richer filtering by schema/object type/name;
- explicit support for data-only archive lookup if real fixture/user demand justifies a model change;
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
