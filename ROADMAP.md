# pgdumpx Roadmap

Status: **v0.1 foundation complete; v0.2 implementation complete; the 0.2.0 publication process is tracked separately from source delivery.**

This roadmap records completed v0.1/v0.2 work and explicitly deferred candidates. The normative v0.1 contract remains in `docs/REQUIREMENTS.md`, final v0.1 evidence is mapped in `docs/V0.1-RELEASE-AUDIT.md`, GitHub Tracking Issue #30 records the v0.1 sequence, and [Tracking Issue #56](https://github.com/tappe9/pgdumpx/issues/56) records the completed v0.2 sequence.

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
- `pgdumpx find` scan options and finite CLI defaults:

```text
--max-rows <N>               default: 100000 complete rows
--max-decompressed-bytes <N> default: 67108864 parser-consumed bytes
--unlimited                  explicit trusted-input opt-in
```

- bounded raw entry extraction with `EntryReadLimits`;
- a finite **1,073,741,824-byte (1 GiB)** default for `pgdumpx extract` and an explicit positive-`u64` override;
- stable CLI commands:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
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

Beta exit criteria were satisfied by the merged Issue #36 audit: supported platform/toolchain/feature jobs passed, default CLI packaging included all v0.1 compression backends, warning-free rustdoc succeeded, package/license/runtime constraints passed the dedicated preflight, English/Japanese README claims matched tested behavior, and `docs/V0.1-RELEASE-AUDIT.md` recorded no unresolved production blocker.

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
pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] \
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Uses UTF-8 command-line arguments, resolves the column through recorded COPY metadata, and compares the supplied value bytes with logical post-unescape field bytes. The CLI applies finite defaults of 100,000 complete rows and 64 MiB of parser-consumed bytes; one finite override preserves the other default, while `--unlimited` explicitly disables both. Scan-budget options delegate to the same library `ScanLimits` accounting path used by Rust callers. A future byte-literal input mode requires a separate CLI design.

Stable exit behavior:

```text
0  match found
1  no matching row
2  usage, I/O, format, integrity, decompression, COPY, encoding,
   unsupported representation, unknown column, or resource error
```

No-match remains distinct from failure.

## v0.2 — Completed

The v0.2 implementation builds on the v0.1 `Read + Seek`, validated-entry, decompression, row-parser, and raw-output paths. It preserves the single mutable seekable-source invariant and does not add parallel or indexed alternatives.

### Delivered

- #57–#63 ([tracking issue #56](https://github.com/tappe9/pgdumpx/issues/56)) delivered file-oriented archive opening, owned exact-byte `TableSelector` values, reusable `ExtractionPlan` preflight, bounded deterministic sequential multi-table execution, metadata-only `MetadataFilter`, exact named-column equality helpers, and benchmark-driven evaluation through production paths.
- #70–#78 ([first follow-up](https://github.com/tappe9/pgdumpx/issues/70)) delivered correctness, security, and maintenance follow-ups: destination flush-error propagation, aggregate metadata budgets, terminal COPY reader failures, row field-count validation, linear plan duplicate detection, finite CLI scan defaults, warning-clean feature coverage, scheduled fuzzing, and dependency advisory policy.
- [#89](https://github.com/tappe9/pgdumpx/issues/89) hardened GitHub Actions with pinned dependencies, bounded execution, least-privilege permissions, and cancellation policy.
- [#92](https://github.com/tappe9/pgdumpx/issues/92) prepared reproducible `pgdumpx 0.2.0` and `pgdumpx-cli 0.2.0` package metadata, changelog, release notes, packaging verification, and staged release instructions.

The `0.2.0` source and package contract is complete. Registry publication, the annotated tag, and the GitHub Release follow the independently verified process in `docs/RELEASING.md`; this roadmap does not infer their live state.

## v0.3+ — Deferred candidates

The following are candidates, not active commitments:

- parallel extraction, concurrent seeking, or source-factory/reopen designs;
- persistent/sidecar row indexes or decompression restart-point schemes;
- data-only archive identity synthesis for standalone `TABLE DATA`;
- SQL `WHERE`, a predicate DSL, SQL coercion/collation, or a broad typed-value system;
- CSV, JSON Lines, Arrow, Polars, or Parquet integrations;
- Directory/Tar archive formats;
- Python or other language bindings;
- archive versions older than 1.14 or newly discovered row representations.

Any candidate needs independent requirements, compatibility/resource analysis, and evidence before it becomes scheduled work.

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
