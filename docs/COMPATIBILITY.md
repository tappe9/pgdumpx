# pgdumpx Compatibility Matrix

Status: **Alpha 3 archive 1.14–1.16 metadata and compatibility matrix is fixture- and differential-verified**

This document separates intended v0.1 compatibility from compatibility that has actually been demonstrated by official fixtures and production-path tests.

The public production path verifies archive 1.14, 1.15, and 1.16 header/TOC parsing, version-aware public TOC metadata, metadata indexes, validated selected-entry seeking, custom chunk framing, and selected-entry streaming for the compression algorithms covered below. The committed compatibility corpus is also compared against official PostgreSQL `pg_restore` output in CI.

## Archive format versions

| Archive version | v0.1 target | Fixture-verified | Notes |
|---|---:|---:|---|
| 1.14 | Yes | Metadata + none/gzip selected entry + differential | Official PostgreSQL 15.19 fixtures verify the legacy compression-level header representation and the 1.14 TOC layout. |
| 1.15 | Yes | Metadata + none/gzip selected entry + differential | Official PostgreSQL 16.15 fixtures verify explicit compression-algorithm metadata and the 1.15 TOC layout. |
| 1.16 | Yes | Metadata + none/gzip/LZ4/Zstandard selected entry + COPY/INSERT differential | Official PostgreSQL 18.4 fixtures verify all four v0.1 compression algorithms, the established COPY row/search path, and INSERT row-API rejection. |
| < 1.14 | No | N/A | Rejected explicitly; deferred until real demand justifies compatibility work. |
| > 1.16 | No | N/A | Rejected explicitly until upstream format changes are reviewed and fixtures are added. |

Archive format version and PostgreSQL server release are related but are not interchangeable identifiers. Compatibility claims are made against observed archive-format behavior and reference-generated fixtures.

The implemented version gates mirror the supported upstream layouts: archive 1.14 stores a legacy compression level, archive 1.15 and newer store an explicit compression algorithm, table access method metadata is consumed for archive 1.14 and newer, and relkind is consumed only for archive 1.16 in the current target range. Public `TocEntry` accessors preserve raw archive-string bytes and distinguish version absence or encoded NULL from an encoded empty value. In particular, relkind is `None` before archive 1.16 and an encoded zero remains `Some(0)` in 1.16.

## Compression

| Compression | v0.1 target | Fixture-verified | Streaming requirement | Delivery order |
|---|---:|---:|---:|---|
| none | Yes | Selected-entry streaming + differential | Yes | First vertical slice / version compatibility |
| gzip | Yes | Selected-entry streaming + differential | Yes | First vertical slice / version compatibility |
| LZ4 | Yes | Selected-entry streaming + COPY rows/search + differential | Yes | Compatibility expansion |
| Zstandard | Yes | Selected-entry streaming + COPY rows/search + differential | Yes | Compatibility expansion |

Committed official PostgreSQL fixtures exercise all four v0.1 compression algorithms through the public selected-entry path. none/gzip additionally cover archive 1.14 and 1.15, while PostgreSQL 18.4 archive 1.16 fixtures cover none, gzip, LZ4, and Zstandard. The LZ4/Zstandard acceptance tests also verify one-byte source reads, one-byte custom-chunk segmentation, malformed/truncated compressed streams, exact raw-output limits, row iteration, and first-match early termination.

The compatibility differential job builds the CLI with all compression backends and compares selected table-data output from every committed fixture with PostgreSQL 18.4 `pg_restore`. COPY logical rows are compared byte-for-byte; the INSERT fixture's complete selected INSERT statement region is compared byte-for-byte.

A compression algorithm is only marked verified after a reference-generated fixture is opened and its selected entry is streamed through the same production decoder path used by the library. Version-specific compression representation is tested independently from the decompressor implementation.

### Compression backend and feature policy

Compression backend types and decoder settings remain private implementation details. The library exposes `lz4` and `zstd` Cargo features; both are enabled by default, and the CLI default feature set forwards both features to the library. Disabling a backend feature does not prevent metadata parsing: `Compression::Lz4` and `Compression::Zstd` remain recognizable, while attempting to read a selected entry that requires a disabled backend returns the typed unsupported-compression error.

The LZ4 backend uses `lz4_flex` 0.14 with its frame and safe-decode support. The Zstandard backend uses `ruzstd` 0.8.1, pinned to preserve the workspace Rust 1.85 MSRV. These choices do not add a PostgreSQL runtime dependency or a native compression-library build requirement. Backend-specific dependencies remain optional at the library boundary.

## Table-data representations

| Representation | Raw entry access | Row-aware v0.1 access | Evidence |
|---|---:|---:|---|
| pg_dump COPY text | Implemented for supported none/gzip/LZ4/Zstandard entry paths | Implemented | Official fixtures and production-path tests; selected output differential-checked against `pg_restore`. |
| Binary COPY | Depends on readable entry | No | Explicitly deferred. |
| `--inserts` | Implemented when the selected entry itself is readable | No | Official PostgreSQL 18.4 fixture proves raw extraction remains available and row APIs return `UnsupportedTableDataRepresentation::Insert` before COPY parsing. |
| `--column-inserts` | Depends on readable entry | No | Same explicit unsupported-representation policy; no dedicated v0.1 compatibility cell is claimed. |
| INSERT output produced with `--rows-per-insert` | Depends on readable entry | No | Same explicit unsupported-representation policy; no dedicated v0.1 compatibility cell is claimed. |

See [COPY-TEXT.md](COPY-TEXT.md) for the row parser contract.

## Source compatibility

The initial high-performance archive API requires:

```rust
Read + Seek
```

The primary supported use case is a seekable PostgreSQL Custom Format archive with usable recorded entry positions. A general non-seekable sequential archive API is deferred.

The default build must not require a running PostgreSQL server, `libpq`, or `pg_restore` at runtime. PostgreSQL executables are used only by fixture-generation and differential-test tooling.

## Verification policy

A compatibility cell moves to a concrete verified state only when all relevant evidence exists:

1. the fixture was generated by an official PostgreSQL `pg_dump`;
2. the generator version, fixed image provenance, and exact command are recorded;
3. the fixture checksum is recorded;
4. the archive opens through the public parser path;
5. the selected entry is validated and streamed;
6. decompressed bytes/rows are compared with `pg_restore` output where an equivalent representation exists;
7. the case runs in CI or in a reproducible compatibility job.

All nine committed official fixtures satisfy these requirements for the claims recorded in this document. Focused malformed/non-UTF-8 tests complement the official corpus for cases that are not appropriate valid-format fixture requirements.

The repository should avoid broad claims such as “supports PostgreSQL X–Y” when the actual evidence is narrower than the archive-version and feature cells above.

## Fixture provenance manifest

Every valid-format fixture has a machine-readable provenance record. `tests/fixtures/manifest.toml` records the generator, fixed image digest/platform, exact command, archive version, compression details, checksum, purposes, expected table, row count, and column layout for each committed official fixture.

Rules:

- valid-format behavior must not rely only on hand-built bytes;
- malformed fixtures may be hand-built when their purpose is an impossible or dangerous boundary condition;
- generated fixture binaries should be kept small and deterministic where practical;
- large benchmark datasets should be generated reproducibly rather than committed by default;
- fixture updates that change a checksum must explain why the generator input or command changed.

## Current fixture inventory

The committed official compatibility corpus is:

```text
archive version 1.14 — PostgreSQL 15.19
  - none / COPY text (legacy compression level 0)
  - gzip / COPY text (legacy compression level 6)

archive version 1.15 — PostgreSQL 16.15
  - none / COPY text
  - gzip level 6 / COPY text

archive version 1.16 — PostgreSQL 18.4
  - none / COPY text
  - gzip level 6 / COPY text
  - LZ4 level 1 / COPY text
  - Zstandard level 3 / COPY text
  - none / INSERT
```

All nine archives use the same deterministic `public.orders` source table. The 1.14/1.15 fixtures exercise metadata open, table/table-data lookup, validated seek, custom framing, none/gzip selected-entry streaming, and differential comparison. The 1.16 corpus covers all four v0.1 compression paths; COPY fixtures exercise the established row/search behavior, and the INSERT fixture proves raw-entry availability plus explicit row-aware rejection.

Exact PostgreSQL generator releases and image digests are recorded in the fixture manifest rather than inferred from server-version labels.

## Updating this document

When PostgreSQL introduces a new archive version:

1. review the upstream archive version constants and comments;
2. diff header, TOC, custom-data framing, offset, and compression behavior;
3. update `PG-DUMP-CUSTOM-FORMAT.md`;
4. generate official fixtures and provenance records;
5. add compatibility and differential tests;
6. only then expand this matrix and the public supported-version range.
