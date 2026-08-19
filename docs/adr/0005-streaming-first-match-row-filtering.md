# ADR 0005: Streaming first-match row filtering

- Status: Accepted
- Date: 2026-08-19

## Context

A primary pgdumpx use case is to open a large PostgreSQL Custom Format dump, select one table, apply an application-defined condition, and return the first matching row without restoring the database.

The Custom Format TOC can identify a table-data entry and, when a usable position is recorded, let pgdumpx seek directly to that entry. It does not provide a required row-level index from column values to positions inside the decompressed COPY stream.

The existing v0.1 design already exposes streaming COPY rows backed by a reusable row buffer. A first-match API therefore needs an explicit policy for column lookup, predicate evaluation, result ownership, and performance expectations.

## Decision

### Custom Format remains the product focus

The first implementation remains specialized around `pg_dump -Fc`. Plain, Directory, and Tar formats are not added to v0.1 merely to broaden format coverage.

### Column layout comes from pg_dump COPY metadata

For supported pg_dump-generated table-data entries, pgdumpx parses the ordered column list from the TOC entry's recorded COPY statement.

The table row reader exposes byte-oriented column metadata and name-to-index lookup. Column names are resolved against metadata once; row scanning uses positional field access.

If row data is positionally readable but column metadata cannot be safely derived, positional iteration may remain available while column-aware helpers fail explicitly. pgdumpx does not guess column names.

### First-match is a streaming predicate scan

v0.1 exposes an operation equivalent in purpose to:

```rust
pub fn find_first<F>(
    &mut self,
    predicate: F,
) -> Result<Option<OwnedRow>, PgDumpError>
where
    F: FnMut(&Row<'_>) -> bool;
```

Rows are evaluated in COPY order. The scan stops immediately when the predicate first returns `true`.

The predicate API is intentionally Rust-native. pgdumpx v0.1 does not parse SQL `WHERE` strings or define a general condition DSL.

### The matched result is owned

Normal row iteration continues to use borrowed `Row` / `FieldRef` values backed by a reusable buffer.

When `find_first` finds a match, only that current row is copied into `OwnedRow` / `OwnedField` values. The result can then outlive the row reader safely.

The copy remains bounded by the same configured row-size and field-count limits as normal row parsing.

### No row-index performance claim

The public contract distinguishes two operations:

```text
archive/table selection  -> TOC/index metadata + direct seek
row matching             -> sequential decompression + COPY parsing
```

A match near the beginning can terminate quickly. A late or missing match can require scanning the complete selected table-data entry.

Worst-case row-search work is proportional to the selected table's data size. pgdumpx must not describe `find_first` as constant-time, logarithmic, or equivalent to a PostgreSQL index lookup.

### Persistent acceleration is deferred

A pgdumpx-specific sidecar index, decompression restart-point index, or other repeated-query acceleration structure is not part of v0.1.

Such a feature would require a separate design because a logical row position inside decompressed data is not automatically an independently seekable compressed-file position.

## Consequences

### Positive

- the common "table + condition -> one row" use case is supported without restore;
- memory remains bounded during scans;
- early matches stop I/O/decompression work as soon as possible;
- column-aware matching does not require UTF-8;
- the core stays independent from SQL parsing;
- returned matches have simple ownership semantics.

### Trade-offs

- repeated searches of a large table may repeatedly decompress and parse the same entry;
- absent or late matches can be expensive;
- callers wanting database-like indexed lookup need PostgreSQL itself or a future pgdumpx-specific indexing layer;
- COPY column metadata parsing becomes part of the v0.1 compatibility/test surface.

## Required tests

The implementation must cover at least:

- supported COPY column-list extraction;
- byte-oriented column lookup;
- first-row match;
- middle/late match;
- no match;
- multiple matching rows returning the first;
- early termination verified with an instrumented source/reader;
- matched `OwnedRow` surviving reader teardown;
- non-UTF-8 field matching;
- malformed/unavailable column metadata behavior;
- row-size and field-count limits during first-match scanning.
