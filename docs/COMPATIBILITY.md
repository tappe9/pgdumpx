# pgdumpx Compatibility Matrix

Status: **Archive 1.14–1.16 metadata opening and supported selected-entry compression paths are fixture-verified**

This document separates intended v0.1 compatibility from compatibility that has actually been demonstrated by official fixtures and production-path tests.

The public production path verifies archive 1.14, 1.15, and 1.16 header/TOC parsing, metadata indexes, validated selected-entry seeking, custom chunk framing, and selected-entry streaming for the compression algorithms covered below. The complete version-aware FR-006 public metadata-surface audit remains tracked separately, so this evidence does not imply that every version-conditional TOC field is already public.

## Archive format versions

| Archive version | v0.1 target | Fixture-verified | Notes |
|---|---:|---:|---|
| 1.14 | Yes | Metadata + none/gzip selected entry | Official PostgreSQL 15.19 fixtures verify the legacy compression-level header representation and the minimum version-aware TOC path. |
| 1.15 | Yes | Metadata + none/gzip selected entry | Official PostgreSQL 16.15 fixtures verify explicit compression-algorithm metadata and the minimum version-aware TOC path. |
| 1.16 | Yes | Metadata + none/gzip/LZ4/Zstandard selected entry | Official PostgreSQL 18.4 fixtures verify the 1.16 header/TOC path, all four v0.1 compression algorithms, and the established COPY row/search path. |
| < 1.14 | No | N/A | Rejected explicitly; deferred until real demand justifies compatibility work. |
| > 1.16 | No | N/A | Rejected explicitly until upstream format changes are reviewed and fixtures are added. |

Archive format version and PostgreSQL server release are related but are not interchangeable identifiers. Compatibility claims are made against observed archive-format behavior and reference-generated fixtures.

The implemented version gates mirror the supported upstream layouts: archive 1.14 stores a legacy compression level, archive 1.15 and newer store an explicit compression algorithm, table access method metadata is part of the supported 1.14+ TOC layout, and relkind is consumed only for archive 1.16 in the current target range. The broader public/model audit for version-conditional metadata remains separate work.

## Compression

| Compression | v0.1 target | Fixture-verified | Streaming requirement | Delivery order |
|---|---:|---:|---:|---|
| none | Yes | Selected-entry streaming | Yes | First vertical slice / version compatibility |
| gzip | Yes | Selected-entry streaming | Yes | First vertical slice / version compatibility |
| LZ4 | Yes | Selected-entry streaming + COPY rows/search | Yes | Compatibility expansion |
| Zstandard | Yes | Selected-entry streaming + COPY rows/search | Yes | Compatibility expansion |

Committed official PostgreSQL fixtures now exercise all four v0.1 compression algorithms through the public selected-entry reader. none/gzip additionally cover archive 1.14 and 1.15, while PostgreSQL 18.4 archive 1.16 fixtures cover none, gzip, LZ4, and Zstandard. The LZ4/Zstandard acceptance tests also verify one-byte source reads, one-byte custom-chunk segmentation, malformed/truncated compressed streams, exact raw-output limits, row iteration, and first-match early termination.

A compression algorithm is only marked fully verified after a reference-generated fixture is opened and its selected entry is streamed through the same production decoder path used by the library.

Version-specific compression representation is tested independently from the decompressor implementation.

### Compression backend and feature policy

Compression backend types and decoder settings remain private implementation details. The library exposes `lz4` and `zstd` Cargo features; both are enabled by default, and the CLI default feature set forwards both features to the library. Disabling a backend feature does not prevent metadata parsing: `Compression::Lz4` and `Compression::Zstd` remain recognizable, while attempting to read a selected entry that requires a disabled backend returns the typed unsupported-compression error.

The LZ4 backend uses `lz4_flex` 0.14 with its frame and safe-decode support. The Zstandard backend uses `ruzstd` 0.8.1, pinned to preserve the workspace Rust 1.85 MSRV. These choices do not add a PostgreSQL runtime dependency or a native compression-library build requirement. Backend-specific dependencies remain optional at the library boundary.

## Table-data representations

| Representation | Raw entry access | Row-aware v0.1 access | Notes |
|---|---:|---:|---|
| pg_dump COPY text | Implemented for supported none/gzip/LZ4/Zstandard entry paths | Implemented on the established 1.16 row path | Version-expansion row-matrix evidence is completed separately. |
| Binary COPY | Depends on readable entry | No | Explicitly deferred. |
| `--inserts` | Depends on readable entry | No | Must not be misparsed as COPY text. |
| `--column-inserts` | Depends on readable entry | No | Must return an explicit unsupported-representation error for row APIs. |
| INSERT output produced with `--rows-per-insert` | Depends on readable entry | No | Same unsupported-representation policy. |

See [COPY-TEXT.md](COPY-TEXT.md) for the row parser contract.

## Source compatibility

The initial high-performance archive API requires:

```rust
Read + Seek
```

The primary supported use case is a seekable PostgreSQL Custom Format archive with usable recorded entry positions. A general non-seekable sequential archive API is deferred.

The default build must not require a running PostgreSQL server, `libpq`, or `pg_restore` at runtime. PostgreSQL executables may be used by fixture-generation and differential-test tooling.

## Verification policy

A compatibility cell should move from “Not yet” to a concrete verified state only when all relevant evidence exists:

1. the fixture was generated by an official PostgreSQL `pg_dump`;
2. the generator version and exact command are recorded;
3. the fixture checksum is recorded;
4. the archive opens through the public parser path;
5. the selected entry is validated and streamed;
6. decompressed bytes/rows are compared with `pg_restore` output where practical;
7. the case runs in CI or in a reproducible compatibility job.

A “Metadata + selected entry” cell records evidence for steps 1–5 plus CI coverage. It deliberately does not imply the full differential matrix in step 6 or completion of every public metadata accessor.

The repository should avoid broad claims such as “supports PostgreSQL X–Y” when the actual evidence is narrower than that statement.

## Fixture provenance manifest

Every valid-format fixture must have a machine-readable provenance record. The repository uses `tests/fixtures/manifest.toml` to record the generator, command, archive version, checksum, and expected contents for each committed official fixture.

Rules:

- valid-format behavior must not rely only on hand-built bytes;
- malformed fixtures may be hand-built when their purpose is an impossible or dangerous boundary condition;
- generated fixture binaries should be kept small and deterministic where practical;
- large benchmark datasets should be generated reproducibly rather than committed by default;
- fixture updates that change a checksum must explain why the generator input or command changed.

## Current fixture inventory

The committed official compatibility corpus now includes:

```text
archive version 1.14 — PostgreSQL 15.19
  - none (legacy compression level 0)
  - gzip (legacy compression level 6)

archive version 1.15 — PostgreSQL 16.15
  - none
  - gzip level 6

archive version 1.16 — PostgreSQL 18.4
  - none
  - gzip level 6
  - LZ4 level 1
  - Zstandard level 3
```

All eight archives contain the same deterministic `public.orders` table. The 1.14/1.15 fixtures are exercised through metadata open, table/table-data lookup, validated seek, custom framing, and selected-entry none/gzip streaming. The 1.16 corpus covers the existing COPY row/search paths, including LZ4 and Zstandard streaming.

Remaining Alpha 3 compatibility expansion includes:

```text
archive version 1.16
  - INSERT-based fixture proving row APIs reject unsupported representation

archive versions 1.14–1.16
  - complete public/version-conditional metadata audit
  - final differential compatibility matrix
```

Exact PostgreSQL generator releases and image digests are recorded in the fixture manifest rather than inferred from server-version labels.

## Updating this document

When PostgreSQL introduces a new archive version:

1. review the upstream archive version constants and comments;
2. diff header, TOC, custom-data framing, offset, and compression behavior;
3. update `PG-DUMP-CUSTOM-FORMAT.md`;
4. generate official fixtures and provenance records;
5. add compatibility tests;
6. only then expand this matrix and the public supported-version range.
