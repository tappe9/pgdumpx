# pgdumpx Architecture

Status: **Accepted for initial implementation**

This document defines the initial architecture for pgdumpx. The first implementation is intentionally narrow: a safe, read-only, file-backed row scanner for modern PostgreSQL custom-format archives.

## 1. Architectural goals

The architecture prioritizes, in order:

1. correctness against PostgreSQL archive behavior;
2. safety on untrusted archive bytes;
3. bounded memory use for large dump files;
4. bounded total row-scan and raw-decompression work when callers request it;
5. efficient selective access to individual data entries;
6. byte-oriented row extraction and first-match filtering without restore;
7. clear separation between archive framing, decompression, COPY parsing, and presentation;
8. a reusable Rust API independent of CLI, Python, Arrow, PostgreSQL connections, `libpq`, and `pg_restore`;
9. public API encapsulation that leaves room for pre-1.0 compatibility growth;
10. measurable performance without sacrificing correctness or safety.

## 2. Product and system boundary

pgdumpx **reads** PostgreSQL custom-format dump archives. It does not create, modify, restore, or execute their SQL.

The project's specialization is the composition of selective custom-archive access with bounded, byte-oriented, row-aware PostgreSQL `COPY` processing. TOC lookup, seeking, and streaming decompression are foundations for that higher-level workflow rather than standalone product claims.

```text
seekable pg_dump -Fc archive
          │
          ▼
┌────────────────────────────────┐
│          pgdumpx core          │
│                                │
│  checked primitive reader      │
│          │                     │
│  header + TOC parser           │
│          │                     │
│      ArchiveIndex              │
│          │                     │
│  on-demand entry seek          │
│          │                     │
│  framed data reader            │
│          │                     │
│  decompressor                  │
│          │                     │
│  raw-output accounting         │
│          │                     │
│  representation validation     │
│          │                     │
│  COPY text row parser          │
│          │                     │
│  row filter / first match      │
│          │                     │
│  row-scan accounting           │
└──────────┬─────────────────────┘
           │
     public Rust API
           │
     ┌─────┼────────┐
     ▼     ▼        ▼
    CLI   PyO3    Arrow/other
   v0.1   future     future
```

The core does not own terminal formatting, command-line parsing, Python objects, Arrow arrays, network database connections, SQL execution, or a SQL query language.

## 3. Runtime and dependency boundary

The default build must not require:

- a running PostgreSQL server;
- `libpq`;
- `pg_restore` or another PostgreSQL executable at runtime;
- project-authored C code;
- project-authored `unsafe` without a separately accepted ADR.

The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency. Compression backends are selected through correctness, maintenance, portability, build, and benchmark evidence. A backend that introduces a material native build/runtime constraint must be documented and, where practical, feature-gated.

pgdumpx implements its own narrow archive read path rather than using another dump library as a mandatory backend. This keeps byte preservation, integrity validation, resource accounting, and typed errors under one coherent contract. Adjacent libraries may be used for research and differential testing.

See `docs/adr/0007-standalone-row-scanner-and-vertical-slices.md`.

## 4. Initial workspace direction

The first code milestone uses a Cargo workspace with separate library and CLI crates:

```text
pgdumpx/
├── Cargo.toml
├── crates/
│   ├── pgdumpx/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── archive.rs
│   │       ├── error.rs
│   │       ├── limits.rs
│   │       ├── model.rs
│   │       ├── io/
│   │       ├── custom/
│   │       ├── compression/
│   │       └── copy/
│   └── pgdumpx-cli/
│       └── src/main.rs
├── docs/
└── tests/
```

Exact private module names are not public API commitments.

The CLI crate exists from the first vertical slice so end-to-end behavior is exercised early. Parser logic remains in the library.

## 5. Core responsibilities

The `pgdumpx` library owns:

- custom archive magic and version parsing;
- version-aware header decoding;
- TOC decoding and validation;
- custom-format extra TOC data, including entry data offsets;
- lookup indexes for entries and table/table-data relationships;
- checked seeking to data blocks;
- custom data block framing validation;
- streaming decompression for supported algorithms;
- bounded raw decompressed entry extraction;
- validation that a selected row-aware table-data entry uses a supported COPY representation;
- PostgreSQL `COPY` text row framing and field unescaping;
- supported COPY column-layout extraction from TOC `copyStmt` metadata;
- byte-oriented column lookup with explicit metadata-error semantics;
- streaming predicate evaluation and first-match retrieval;
- structural, row-scan, and raw-output resource limits;
- typed, contextual errors;
- public metadata and row-access APIs.

The library does **not** own:

- archive writing;
- `pg_restore` behavior;
- SQL execution;
- SQL `WHERE` parsing or general expression evaluation;
- PostgreSQL client connections;
- INSERT-statement row parsing in v0.1;
- filesystem discovery beyond caller-provided sources and explicit convenience APIs;
- CLI presentation formatting;
- DataFrame or Arrow representations;
- Python bindings;
- persistent row indexes in v0.1.

## 6. Archive open pipeline

Opening an archive performs metadata work only:

```text
Archive::open(reader)
        │
        ▼
validate PGDMP magic
        │
        ▼
parse version + sizes + format + compression
        │
        ▼
parse archive metadata
        │
        ▼
parse TOC entries
        │
        ▼
validate stored offsets and relationships
        │
        ▼
build ArchiveIndex
        │
        ▼
Archive<R>
```

Opening must not read or decompress every table-data block.

The first vertical slice may parse only the archive 1.16 TOC fields required for table lookup and row access. The final v0.1 parser must implement the complete supported-version requirements recorded in `docs/REQUIREMENTS.md`.

## 7. Source abstraction

The initial high-performance API requires a seekable source:

```rust
R: std::io::Read + std::io::Seek
```

This matches the primary use case: local files and other seekable byte sources.

Why `Read + Seek` first:

- custom archives record data positions when generated on seekable output;
- selective entry access is foundational to the row-aware workflow;
- arbitrary non-seekable streaming requires sequential block discovery and different state management;
- adding a separate sequential-reader API later is safer than weakening the file-backed API now.

A convenience `Archive::open_path()` may be added outside the parser's lowest-level primitives.

## 8. Archive index

Metadata is eagerly decoded into a compact index. Payloads are not.

Conceptual internal representation:

```rust
struct ArchiveIndex {
    header: ArchiveHeader,
    entries: Vec<TocEntry>,
    by_dump_id: HashMap<DumpId, usize>,
    tables: HashMap<TableKey, TableIndexEntry>,
}
```

Exact collection types are implementation details. Common dump-ID and `(schema, table)` lookup must not repeatedly scan all TOC entries.

The index is **entry-level**, not row-level. It identifies a selected table-data entry and archive offset; it does not provide offsets for individual COPY rows.

## 9. Public metadata encapsulation

Archive metadata types such as `ArchiveHeader`, `ArchiveVersion`, `DumpId`, `TocEntry`, and `Column` use private fields with accessors unless direct construction is an intentional requirement.

Reasons:

- upstream version expansion can add metadata without breaking struct literals;
- internal byte-storage choices remain changeable;
- invalid combinations cannot be constructed accidentally;
- CLI/serialization DTOs do not freeze the parser model.

Public enums expected to gain variants are `#[non_exhaustive]` before v1.0.

## 10. Entry data access

Entry reads are lazy:

```text
entry lookup
    │
    ▼
validated stored offset
    │
    ▼
seek
    │
    ▼
block type + dump-id validation
    │
    ▼
chunk framing reader
    │
    ▼
stream decompressor
    │
    ▼
raw-output accounting
    │
    ▼
Read-compatible EntryDataReader
```

An entry reader never requires `Vec<u8>` proportional to the complete entry unless a caller explicitly requests a convenience `read_to_end` operation.

Raw entry reading and row-aware parsing are separate capabilities. An archive entry may be structurally readable even when its logical representation is unsupported by the v0.1 COPY parser.

### Raw output limits

A low-level unlimited `EntryDataReader` may be available for trusted callers. The library also provides a bounded reader and a high-level copy operation with an `EntryReadLimits`-style decompressed-byte budget.

Required behavior:

- count decompressed bytes returned or copied;
- use checked counters;
- fail when the next bytes would cross the configured limit;
- distinguish limit exhaustion from normal EOF;
- do not report a truncated stream as successful completion;
- preserve a typed pgdumpx resource error through the `std::io::Read` error source;
- make the CLI `extract` command use the bounded high-level path.

Large-object entries with OID framing may use a separate API rather than being forced into a flat byte stream.

## 11. COPY text parsing

v0.1 row-aware extraction targets table data emitted through PostgreSQL `COPY ... FROM stdin` text form.

The parser is layered on decompressed entry bytes:

```text
EntryDataReader
      │
      ▼
representation validation
      │
      ▼
record framing
      │
      ▼
COPY terminator handling
      │
      ▼
field splitting
      │
      ▼
escape / NULL decoding
      │
      ▼
Row / FieldRef
```

The COPY parser preserves logical bytes and does not assume UTF-8 unless an API explicitly requests conversion.

`FieldRef::Bytes` represents bytes after PostgreSQL COPY text escape decoding. The escaped spelling is an input representation detail, not the exposed value.

The detailed contract is maintained in `docs/COPY-TEXT.md` and tested independently from archive framing.

### Supported representation boundary

Row-aware v0.1 APIs support normal pg_dump-generated COPY text table data.

INSERT-based dump modes such as `--inserts`, `--column-inserts`, and INSERT output produced through `--rows-per-insert` are detected from available metadata and rejected explicitly. They are not guessed as COPY text.

Binary COPY decoding is deferred.

### Column metadata

For normal pg_dump table-data entries, the TOC carries the COPY statement used to restore that entry. pgdumpx parses the supported pg_dump-generated column list and exposes a byte-oriented layout.

Column lookup is resolved once against that layout and then uses positional field access while scanning rows.

```text
metadata valid + column found      -> Ok(Some(index))
metadata valid + column not found  -> Ok(None)
metadata unavailable/malformed     -> Err(...)
```

If row bytes are positionally readable but column metadata cannot be derived, positional iteration may remain available while column-aware helpers return a typed metadata error.

### Lending row model

Normal iteration returns a borrowed `Row` backed by a reusable buffer. It remains valid only until the next mutable row-reader operation.

This relationship is intentionally exposed through `next_row(&mut self)` or an equivalent lending method rather than forcing owned allocation to implement standard `Iterator`.

### First-match filtering

v0.1 includes first-match filtering as a core use case:

```text
select table via TOC
      │
      ▼
seek directly to table-data entry
      │
      ▼
stream decompress + parse COPY rows
      │
      ├── row would exceed budget ──► typed error, row not yielded
      │
      ├── predicate false ──────────► account row ──► next row
      │
      └── predicate true ───────────► copy current row into OwnedRow and stop
```

This is not indexed database lookup. PostgreSQL Custom Format provides an offset for the table-data entry, not a map from column values to row positions.

Therefore:

- table selection is efficient after TOC parsing;
- search within that table is sequential;
- a match near the beginning can stop early;
- a missing or late match may process the complete selected entry unless a budget stops it;
- worst-case unrestricted work is `O(selected table-data size)`;
- memory remains bounded by metadata/indexes, streaming buffers, the current row, and one owned matched row.

A SQL-like `WHERE` parser is not required. Callers use a Rust predicate and column-index helpers.

## 12. Ownership and allocation strategy

Normal operation scales approximately with:

```text
TOC metadata
+ lookup indexes
+ compressed input buffer
+ decompression buffer
+ current COPY row
+ one OwnedRow when first-match succeeds
```

The design rejects:

- loading the whole archive into memory;
- loading every data entry during `Archive::open`;
- loading a complete table to evaluate a predicate;
- allocating from an untrusted declared length without a configured or hard check;
- copying decompressed entry bytes merely to expose a streaming API;
- allocating every row solely to implement standard `Iterator`.

## 13. Compression boundary

Compression is an internal streaming boundary. Archive framing identifies the algorithm; a small abstraction returns a `Read` implementation for decompressed bytes.

Final v0.1 algorithms:

- none;
- gzip;
- LZ4;
- Zstandard.

The first end-to-end slice implements none and gzip for archive 1.16. LZ4, Zstandard, and older version representations follow after the row-search path is working.

Compression dependencies are not part of the public API. Backend selection is evidence-based rather than governed by an ambiguous “Pure Rust” label.

## 14. Error architecture

Errors are typed and preserve useful context.

Representative categories:

- I/O failure;
- unexpected EOF;
- invalid magic;
- unsupported archive version;
- unexpected archive format;
- invalid integer/offset encoding;
- invalid or excessive string length;
- malformed TOC entry;
- invalid data offset;
- unexpected data block type;
- dump-ID mismatch after seek;
- unsupported compression;
- decompression failure;
- malformed COPY data;
- COPY column metadata unavailable or malformed;
- unsupported table-data representation;
- unknown requested column;
- structural, row-scan, or raw-output resource limit exceeded;
- arithmetic overflow;
- explicit UTF-8 conversion failure.

The public error enum is `#[non_exhaustive]` before v1.0.

Errors include offset, dump ID, object, row, consumed work, or limit context when meaningful. Callers do not parse `Display` strings to identify categories.

## 15. Resource limits and accounting

Untrusted metadata and decompressed content can create denial-of-service behavior. Limits are part of the architecture rather than an afterthought.

### Structural/per-item limits

Equivalent in purpose to:

```rust
pub struct Limits {
    // maximum TOC entries
    // maximum metadata string bytes
    // maximum dependencies per entry
    // maximum row bytes
    // maximum fields per row
}
```

### Row-scan limits

Equivalent in purpose to:

```rust
pub struct ScanLimits {
    // optional maximum complete rows
    // optional maximum decompressed bytes consumed by the row parser
}
```

Accounting rules:

- `max_rows = N` permits at most `N` complete rows to be yielded/evaluated;
- a row that crosses a budget is not yielded;
- the matched row counts;
- parser-consumed decompressed bytes include separators and terminators;
- decoder/`BufRead` read-ahead does not count until consumed;
- counters use checked arithmetic;
- early match does not consume later rows.

### Raw extraction limits

Equivalent in purpose to:

```rust
pub struct EntryReadLimits {
    // optional maximum decompressed bytes returned or copied
}
```

Raw limit exhaustion is an error, not successful truncation.

Exact defaults are decided from compatibility fixtures, usability, fuzzing, and CLI needs.

## 16. Version and compatibility policy

The final v0.1 target is archive versions 1.14, 1.15, and 1.16.

Version-specific fields are decoded deliberately. In particular, archive 1.15 introduced explicit compression-algorithm information and 1.16 introduced additional large-object metadata/relkind-related changes.

Unsupported versions return a typed error rather than being parsed optimistically.

Delivery begins with archive 1.16. Intended versus fixture-verified support is tracked in `docs/COMPATIBILITY.md`; the narrow alpha must not be described as complete v0.1 compatibility.

## 17. Safety policy

Every archive byte is untrusted.

Initial rules:

- no project-authored `unsafe` without a separately accepted ADR;
- checked integer conversions, offsets, and counters;
- no unchecked indexing into attacker-controlled buffers;
- no parser panic for structurally malformed input;
- validate a sought block's type and dump ID before consuming payload;
- limit metadata counts, string sizes, dependency counts, row sizes, and field counts;
- provide row-scan budgets for total rows/decompressed bytes;
- provide a bounded raw decompression path;
- keep first-match filtering on the same bounded path as iteration;
- reject unsupported representations before COPY parsing;
- fuzz parser and accounting boundaries before stable release.

The no-panic invariant does not claim recovery from global allocator exhaustion or OS-level failures.

## 18. Concurrency strategy

v0.1 does not require parallel extraction.

The architecture avoids global mutable state that would prevent future parallel extraction. A later file-oriented API may reopen or clone file handles so independent workers can seek to different entries.

Parallel extraction must be benchmarked; it is not automatically beneficial for compressed input or one storage device.

## 19. CLI as an early library consumer

The CLI must not duplicate parser logic. Planned v0.1 commands are:

```text
inspect
list
extract
find
```

`find` is included in the first vertical slice and delegates to table lookup, streaming decompression, COPY parsing, column lookup, limits, and `find_first`.

`extract` writes the decompressed selected table-data body as binary-safe bytes. It does not add DDL or a COPY statement wrapper and uses the bounded raw-copy path.

v0.1 CLI identifiers and values are UTF-8, while the Rust API remains byte-oriented.

Exit behavior:

```text
0  success / find matched
1  find completed with no match
2+ failure
```

Diagnostics remain on stderr so binary stdout is not corrupted.

## 20. Vertical-slice delivery

The first usable alpha is deliberately narrow:

```text
workspace + CI
    -> archive 1.16 header
    -> minimum TOC/table lookup
    -> validated entry seek
    -> none/gzip streaming
    -> COPY rows + columns
    -> find_first
    -> pgdumpx find
```

After this path works with official fixtures, development expands to full limits, raw extraction semantics, archive 1.14/1.15, LZ4/Zstandard, fuzzing, and benchmarks.

The final v0.1 scope remains unchanged. This sequence exposes user value and integration risks earlier than a horizontal layer-by-layer roadmap.

## 21. Testing architecture

Tests are layered.

### Primitive unit tests

Integer, offset, string, version, timestamp, and bounds behavior.

### Official format fixtures

Custom archives are generated by supported official `pg_dump` versions.

Every valid fixture records:

- generator version;
- exact command;
- checksum;
- archive version/compression;
- fixture purpose;
- expected objects.

Verified combinations are reflected in `docs/COMPATIBILITY.md`; targets without passing production-path fixtures are not presented as verified.

### Hand-built malformed fixtures

Truncation, impossible lengths, overflow, wrong block type, dump-ID mismatch, unsupported version, malformed COPY data, and resource-budget exhaustion.

Hand-built bytes are not the sole evidence for valid-format behavior.

### Streaming and resource tests

Cover arbitrary short reads, none/gzip/LZ4/Zstandard boundaries, raw output below/exactly/above limits, row scan below/exactly/above limits, parser-consumed byte accounting independent of read-ahead buffer size, and no silent truncation.

### COPY parser tests

NULL, empty values, tabs, backslashes, numeric/control escapes, terminator, non-UTF-8 bytes, row/field limits, malformed escapes, supported column lists, missing/unavailable metadata, and explicit rejection of INSERT-based representations.

### First-match tests

Cover column lookup, first/middle/late/absent match, first of multiple matches, early termination, owned-row lifetime, non-UTF-8 values, and all resource boundaries.

### CLI integration tests

Cover binary `extract` stdout, diagnostics on stderr, UTF-8 argument behavior, and exit `0`/`1`/`2+` semantics.

### Differential checks

Where practical, compare selected decompressed bytes or logical rows with `pg_restore` output and adjacent libraries for equivalent operations.

### Fuzzing

Primary invariants:

```text
arbitrary metadata bytes -> Ok(...) or typed Err(...), never parser panic
arbitrary framed bytes   -> output or typed Err(...), never parser panic
arbitrary COPY bytes     -> rows or typed Err(...), never parser panic
```

## 22. Performance policy

Performance is a product requirement but not a correctness exception.

Benchmarks measure at least:

- archive open / TOC time;
- peak RSS during open;
- selected-entry extraction throughput and peak RSS;
- COPY row parsing throughput;
- first-match positions near beginning, middle, end, and absent;
- compression-specific throughput;
- raw-output and row-scan accounting overhead.

Published benchmark reports record hardware, OS, commit, fixture/generator, archive version, compression, command/API path, match position, warm-up/repetition method, and measurement tool.

Comparisons against PostgreSQL tools or libraries are included only when they answer a concrete question and semantic differences are stated.

## 23. Accepted ADRs

- ADR 0001 — Pure Rust read-only engine (**superseded by ADR 0007**)
- ADR 0002 — Custom format first
- ADR 0003 — Indexed metadata plus streaming entry readers
- ADR 0004 — v0.1 public API and compatibility policy
- ADR 0005 — Streaming first-match row filtering
- ADR 0006 — v0.1 API and scan-work refinements
- ADR 0007 — Standalone row scanner and vertical-slice delivery

Intentional architectural divergence must be recorded in a new ADR.
