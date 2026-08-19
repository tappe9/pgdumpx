# pgdumpx Roadmap

This roadmap is directional. Delivery slices may change when upstream format research, official fixtures, fuzzing, and benchmark results reveal better boundaries.

The normative v0.1 contract remains in `docs/REQUIREMENTS.md`. GitHub Tracking Issue #30 records the executable issue order and distinguishes technical dependencies from preferred delivery sequence.

## v0.1 — Bounded row scanning for Custom Format archives

Goal: deliver a useful Rust library and CLI that can select one table from a modern PostgreSQL custom-format (`-Fc`) archive, stream its COPY text rows, and stop when a requested row is found—without restoring PostgreSQL or buffering the complete table.

The final v0.1 scope still covers archive versions 1.14–1.16 and none/gzip/LZ4/Zstandard. The implementation order is vertical: prove the complete row-search user story on a narrow compatibility slice first, then complete resource semantics, broaden compatibility, and finish hardening/release evidence.

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
- official PostgreSQL-generated 1.16 none/gzip fixtures with provenance and checksums;
- checked primitive reader and foundational typed errors;
- provisional finite metadata/row/field bounds on every Alpha 1 production parser path;
- an internal `PGDMP`/archive-1.16 header parser;
- the first valid public `Archive::open`, completed with minimum 1.16 TOC metadata;
- minimum TOC metadata required for dump-ID and table/table-data lookup;
- unambiguous table/table-data relationship validation rather than name-only guessing;
- validated seek to one selected data entry;
- custom chunk framing and streaming none/gzip decompression;
- a standalone COPY text parser with NULL handling, escape decoding, borrowed rows, and byte-oriented fields;
- a checked parser-consumed byte accounting seam that is independent of decoder/read-ahead behavior;
- supported COPY column-list parsing from TOC metadata;
- explicit rejection of unsupported INSERT/Binary table-data representations;
- streaming `find_first` with an owned matched row;
- a narrow CLI path:

```text
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

The v0.1 CLI grammar uses exactly one ASCII `.` separator with non-empty schema and table components. SQL identifier quoting/escaping and identifiers containing `.` are deferred at the CLI boundary; the Rust API remains byte-oriented.

Exit criteria:

- official PostgreSQL-generated archive 1.16 fixtures open through the public library path;
- `public.orders` or an equivalent fixture table is located through unambiguous TOC metadata;
- none/gzip row streaming works;
- early, middle, late, multiple-match, and absent cases pass through the same `find_first` path;
- a matching `OwnedRow` survives reader teardown;
- no complete table-data entry is buffered;
- Alpha 1 contains no implicit unbounded structural allocation path;
- parser-consumed byte accounting is stable across input/read-buffer segmentation and excludes unread lookahead;
- `find` distinguishes match, no match, and parser/runtime failure;
- `find` validates the documented `SCHEMA.TABLE` grammar;
- documentation does not claim compatibility beyond fixture evidence.

This alpha is intentionally narrow. Its purpose is to demonstrate the product value before implementing every archive-version and compression branch.

## Alpha 2 — Complete resource, error, and CLI semantics

Deliver:

- a public structural `Limits` contract with finite compatibility-oriented defaults for:
  - TOC entries;
  - archive string bytes;
  - dependencies per entry;
  - row bytes;
  - fields per row;
- replacement of the Alpha 1 provisional bounds with one coherent public configuration path;
- a complete contextual typed-error taxonomy and `std::error::Error::source` behavior;
- exact library `ScanLimits` accounting for:
  - complete rows yielded/evaluated;
  - physical decompressed COPY bytes consumed by the parser;
- `pgdumpx find` options equivalent in purpose to:

```text
--max-rows <N>
--max-decompressed-bytes <N>
```

- raw entry extraction with a library-provided decompressed-byte limit;
- a finite compatibility-oriented default raw-output limit for `pgdumpx extract`;
- an explicit `extract` byte-limit override;
- stable CLI contracts for:

```text
pgdumpx inspect <FILE>
pgdumpx list <FILE>
pgdumpx extract <FILE> <SCHEMA.TABLE>
pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

- binary-safe `extract` output of the selected decompressed table-data body;
- documented streaming failure behavior: bytes already written before a later limit/writer error cannot be rolled back, but partial output is never reported as successful completion;
- documented UTF-8 CLI argument boundary while the Rust archive/row API remains byte-oriented;
- typed, location-aware errors for malformed metadata, block identity, decompression, COPY data, representation, encoding, and resource limits.

`inspect`/`list`, structural-limit completion, and error-taxonomy completion are technically separable once the Alpha 1 path exists. Tracking Issue #30 records the preferred sequence without creating artificial technical dependencies.

Exit criteria:

- every structural default is finite and configurable;
- configured row/decompressed-byte budgets stop on the normal production streaming path;
- a row that would cross a configured row/byte budget is not yielded or passed to the predicate;
- scan byte accounting is independent of decoder/`BufRead` read-ahead;
- `find` resource exhaustion exits as failure (`2+`), not no match (`1`);
- raw extraction fails on limit exhaustion rather than silently truncating output;
- `extract` has a finite CLI default and a validated explicit override;
- partial bytes followed by a limit/writer error are documented and exit non-successfully;
- borrowed rows remain valid only until the next mutable reader operation;
- matched `OwnedRow` values survive reader teardown;
- CLI output, stderr separation, argument grammar, and exit behavior are covered by integration tests;
- `inspect`, `list`, `extract`, and `find` all delegate to the library.

## Alpha 3 — Compatibility expansion

Deliver:

- archive version 1.15 parsing and explicit compression metadata;
- archive version 1.14 parsing, including its pre-1.15 compression representation;
- LZ4 and Zstandard streaming decoders;
- the complete v0.1 TOC metadata/accessor surface for archive versions 1.14–1.16;
- version-conditional absence rather than invented metadata defaults;
- fixture provenance containing generator version, exact command, checksum, purpose, and expected objects;
- differential checks against `pg_restore` output where practical;
- compatibility matrix updates backed only by fixtures that pass through production code paths.

Version work proceeds 1.16 → 1.15 → 1.14 because 1.14 introduces the distinct pre-1.15 compression representation.

LZ4/Zstandard are technically independent from archive-1.15 parsing when valid official 1.16 fixtures exercise the backends. They may proceed in parallel with older-version work after the entry/row/raw paths are complete.

Compression packaging policy for v0.1:

- the default published CLI build enables none/gzip/LZ4/Zstandard;
- the library may provide reduced-feature builds where practical;
- disabling a backend must not prevent metadata parsing from recognizing the archive algorithm;
- attempting to read an entry whose backend is disabled returns a typed backend-unavailable/unsupported-compression error;
- backend-specific types/settings do not leak into the public archive API;
- formal performance claims wait for the Beta benchmark evidence.

Exit criteria:

- each target archive-version/compression combination is either verified with recorded evidence or remains clearly marked unverified;
- short reads across archive chunks and decoder boundaries are tested;
- unsupported older/newer archive versions fail explicitly;
- disabled compression backends fail selected-entry reads explicitly without corrupting metadata behavior;
- no target is advertised as “PostgreSQL X supported” solely from server-version naming;
- the complete public TOC surface does not create a duplicate parser path beside the minimum version parsers.

## Beta — Hardening, performance evidence, and release readiness

Deliver:

- malformed-input regression corpus;
- fuzz targets for archive metadata, TOC parsing, chunk framing, COPY escapes, column metadata, and limit accounting;
- benchmark harness and reproducible dataset generation;
- peak-memory and throughput measurements;
- first-match benchmarks at early/middle/late/absent positions;
- comparison methodology for `pg_restore`, adjacent libraries, and pgdumpx paths when the operations are meaningfully equivalent;
- cross-platform and compression-feature-matrix CI;
- public rustdoc coverage, including lending lifetime and sequential-scan semantics;
- packaging verification;
- dependency/license/native-build constraint verification for `MIT OR Apache-2.0`;
- a final evidence-based audit of every v0.1 Definition of Done item and all public documentation.

Focused parser fuzz harnesses or small backend-selection measurements may be introduced earlier when useful. The full fuzz campaign and reproducible public benchmark evidence remain Beta completion work.

Exit criteria:

- no known malformed-input parser panic within tested/fuzzed boundaries;
- benchmark reports state exact hardware, fixture, command, compression, and measurement method;
- README performance statements are based on reproducible evidence;
- the documented platform/toolchain/feature matrix passes;
- the default CLI package includes every v0.1 compression backend;
- public rustdoc builds with warnings denied and documents actual semantics;
- package contents, licensing, and dependency constraints are verified;
- English/Japanese README claims and all CLI contracts match tested reality;
- all v0.1 Definition of Done items in `docs/REQUIREMENTS.md` are satisfied or an explicit release blocker remains.

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
- fixture provenance, fuzzing, benchmarks, CI, rustdoc, packaging, dependency/license verification, and compatibility documentation.

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

The command uses a finite compatibility-oriented default byte limit and allows a validated explicit override. Limit exhaustion is a failure rather than successful truncation. Because output is streamed, bytes already written before a later limit or writer error cannot be rolled back; the command exits non-successfully and diagnostics remain on stderr.

### `find`

```text
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] \
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

Uses UTF-8 command-line arguments in v0.1, resolves the column through recorded COPY metadata, and compares the supplied value with logical post-unescape field bytes. Scan-budget options delegate to the same library `ScanLimits` accounting path used by Rust callers. A future byte-literal input mode requires a separate CLI design.

Stable exit behavior:

```text
0  match found
1  no matching row
2+ usage, I/O, format, integrity, decompression, COPY, encoding, unsupported representation, unknown column, or resource error
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
