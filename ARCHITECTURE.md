# pgdumpx Architecture

Status: **Accepted for initial implementation**

This document defines the initial architecture for pgdumpx. The first implementation is intentionally narrow: a safe, read-only, file-backed reader for modern PostgreSQL custom-format archives.

## 1. Architectural goals

The architecture prioritizes, in order:

1. correctness against PostgreSQL archive behavior;
2. safety on untrusted archive bytes;
3. bounded memory use for large dump files;
4. efficient selective access to individual data entries;
5. clear separation between archive framing, decompression, COPY parsing, and presentation;
6. a reusable Pure Rust API independent of CLI, Python, Arrow, or database connections;
7. measurable performance without sacrificing correctness or safety.

## 2. System boundary

pgdumpx **reads** PostgreSQL dump archives. It does not create, modify, restore, or execute their SQL.

```text
seekable pg_dump -Fc archive
          │
          ▼
┌──────────────────────────────┐
│          pgdumpx core        │
│                              │
│  checked primitive reader    │
│          │                   │
│  header + TOC parser         │
│          │                   │
│      ArchiveIndex            │
│          │                   │
│  on-demand entry seek        │
│          │                   │
│  framed data reader          │
│          │                   │
│  decompressor                │
│          │                   │
│  COPY text row parser        │
└──────────┬───────────────────┘
           │
     public Rust API
           │
     ┌─────┼────────┐
     ▼     ▼        ▼
    CLI   PyO3    Arrow/other
  future  future     future
```

The core does not own terminal formatting, command-line argument parsing, Python objects, Arrow arrays, network database connections, or SQL execution.

## 3. Initial workspace direction

The first code milestone should use a Cargo workspace with separate library and CLI crates:

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

## 4. Core responsibilities

The `pgdumpx` library owns:

- custom archive magic and version parsing;
- version-aware header decoding;
- TOC decoding and validation;
- custom-format extra TOC data, including entry data offsets;
- lookup indexes for entries and table/table-data relationships;
- checked seeking to data blocks;
- custom data block framing validation;
- streaming decompression for supported compression algorithms;
- PostgreSQL `COPY` text row framing and field unescaping;
- typed errors;
- configurable resource limits;
- public metadata and row-access APIs.

The library does **not** own:

- archive writing;
- `pg_restore` behavior;
- SQL execution;
- PostgreSQL client connections;
- filesystem path discovery beyond caller-provided readers/convenience APIs;
- CLI output formatting;
- DataFrame or Arrow representations;
- Python bindings.

## 5. Archive open pipeline

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

## 6. Source abstraction

The initial high-performance API requires a seekable source:

```rust
R: std::io::Read + std::io::Seek
```

This matches the primary use case: local files and other seekable byte sources.

Why `Read + Seek` first:

- custom archives record data positions when generated on seekable output;
- selective extraction is the core differentiator;
- arbitrary non-seekable streaming requires sequential block discovery and different state management;
- adding a separate sequential-reader API later is safer than weakening the file-backed API now.

A convenience `Archive::open_path()` may be added outside the parser's lowest-level primitives.

## 7. Archive index

Metadata is eagerly decoded into a compact index. Payloads are not.

Conceptual representation:

```rust
pub struct ArchiveIndex {
    header: ArchiveHeader,
    entries: Vec<TocEntry>,
    by_dump_id: HashMap<DumpId, usize>,
    tables: HashMap<TableKey, TableRef>,
}
```

The exact collection types are implementation details. The design requirement is that common entry/table lookup not require repeatedly scanning all TOC entries.

## 8. Entry data access

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
Read-compatible EntryDataReader
```

An entry reader must never require `Vec<u8>` proportional to the complete entry unless the caller explicitly requests a convenience `read_to_end` operation.

## 9. COPY text parsing

v0.1 row-aware extraction targets table data emitted through PostgreSQL `COPY ... FROM stdin` text form.

The parser is layered on top of decompressed entry bytes:

```text
EntryDataReader
      │
      ▼
line / record framing
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

The COPY parser should preserve raw bytes where possible. It must not assume UTF-8 for arbitrary database text data unless an API explicitly requests UTF-8 conversion.

`COPY` text semantics must be documented and tested separately from custom archive framing.

## 10. Ownership and allocation strategy

Normal operation should scale approximately with:

```text
TOC metadata
+ lookup indexes
+ compressed input buffer
+ decompression buffer
+ current COPY row
```

The design rejects:

- loading the whole archive into memory;
- loading every data entry during `Archive::open`;
- allocating based solely on an untrusted declared string/row length without a configured or hard safety check;
- copying decompressed entry bytes merely to expose a streaming API.

## 11. Compression boundary

Compression is an internal streaming boundary. Archive framing code identifies the configured algorithm; a small abstraction returns a `Read` implementation for decompressed bytes.

Planned v0.1 algorithms:

- none;
- gzip;
- LZ4;
- Zstandard.

The project prefers Pure Rust implementations when their correctness and maintenance quality are acceptable. Compression dependencies are not part of the public API.

## 12. Error architecture

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
- dump-id mismatch after seek;
- unsupported compression;
- decompression failure;
- malformed COPY data;
- resource limit exceeded;
- arithmetic overflow.

The public error enum is `#[non_exhaustive]` before v1.0.

## 13. Resource limits

Untrusted metadata and decompressed rows can otherwise create denial-of-service behavior. Limits are part of the initial architecture rather than an afterthought.

Conceptual configuration:

```rust
pub struct Limits {
    pub max_toc_entries: usize,
    pub max_string_bytes: usize,
    pub max_dependencies_per_entry: usize,
    pub max_row_bytes: usize,
    pub max_fields_per_row: usize,
}
```

Exact defaults are decided during implementation from compatibility fixtures and fuzzing evidence. Limits must be configurable without changing the archive model.

## 14. Version policy

v0.1 targets archive versions 1.14, 1.15, and 1.16.

Version-specific fields are decoded deliberately. In particular, archive 1.15 introduced explicit compression-algorithm information in the header and 1.16 introduced additional large-object metadata/relkind-related archive changes in PostgreSQL upstream.

Unsupported versions return a typed error rather than being parsed optimistically.

Support for older versions may be added after the modern-format implementation is stable.

## 15. Safety policy

Every archive byte is untrusted.

Initial rules:

- no project-authored `unsafe` without a separately accepted ADR;
- checked integer conversions and offset arithmetic;
- no unchecked indexing into attacker-controlled buffers;
- no panic for structurally malformed archive input;
- validate a sought data block's type and dump ID before consuming payload;
- limit metadata counts, string sizes, dependency counts, row sizes, and field counts;
- fuzz parser boundaries before a stable release.

The no-panic invariant does not claim recoverability from global allocator exhaustion or OS-level I/O failures.

## 16. Concurrency strategy

v0.1 does not require parallel extraction.

The architecture must, however, avoid global mutable state that would prevent future parallel extraction. A later file-oriented API may reopen or clone file handles so independent workers can seek to different data entries concurrently.

Parallel extraction must be benchmarked; it is not automatically beneficial for compressed input or a single storage device.

## 17. Testing architecture

Tests are layered:

### Primitive unit tests

Integer, offset, string, version, timestamp, and bounds behavior.

### Format fixtures

Custom archives generated by supported PostgreSQL `pg_dump` versions, with none/gzip/LZ4/Zstandard variants where PostgreSQL supports them.

### Hand-built malformed fixtures

Truncation, invalid lengths, offset overflow, wrong block type, dump-id mismatch, unsupported version, and resource-budget exhaustion.

### COPY parser tests

NULL markers, escapes, tabs, backslashes, newlines, empty fields, terminator handling, non-UTF-8 bytes, large-row limits, and malformed escapes.

### Differential/integration checks

Where practical, compare extracted bytes or rows with `pg_restore` output for the same entry.

### Fuzzing

Primary invariants:

```text
arbitrary metadata bytes -> Ok(...) or typed Err(...), never parser panic
arbitrary COPY bytes     -> rows or typed Err(...), never parser panic
```

## 18. Performance policy

Performance is a product requirement but not a correctness exception.

Benchmarks should measure at least:

- archive open / TOC parse time;
- peak RSS while opening large archives;
- single-entry extraction throughput;
- COPY row parsing throughput;
- compression-specific throughput;
- peak RSS during extraction.

Comparisons with `pg_restore` and `libpgdump` are encouraged, but README claims must be based on reproducible results.

## 19. Accepted ADRs

- ADR 0001 — Pure Rust read-only engine
- ADR 0002 — Custom format first
- ADR 0003 — Indexed metadata plus streaming entry readers
- ADR 0004 — v0.1 public API and compatibility policy

Intentional architectural divergence should be recorded in a new ADR.
