# PostgreSQL COPY Text Contract

Status: **Accepted baseline for v0.1 implementation**

This document defines the COPY-text behavior that pgdumpx v0.1 relies on after a selected PostgreSQL Custom Format table-data entry has been framed and decompressed.

It is a parser contract for pgdumpx, not an independent PostgreSQL specification. PostgreSQL upstream behavior remains authoritative.

## 1. Scope

v0.1 row-aware access targets normal pg_dump-generated table-data entries restored through a `COPY ... FROM stdin` statement using PostgreSQL text representation.

The row-aware API does not execute SQL. It parses the table-data byte stream associated with the recorded COPY metadata.

The following pg_dump data representations are intentionally **not** row-aware v0.1 inputs:

- `--inserts`;
- `--column-inserts`;
- `--rows-per-insert` when it causes INSERT-based table data;
- Binary COPY data.

If a table-data entry uses an unsupported representation, pgdumpx must return a typed unsupported-representation error for row-aware access rather than attempting to parse it as COPY text.

Raw entry extraction may still be available where the archive entry itself is otherwise readable.

## 2. Layer boundary

COPY parsing begins only after archive-level processing has produced decompressed entry bytes:

```text
Custom Format entry
        │
        ▼
block/chunk framing
        │
        ▼
streaming decompression
        │
        ▼
COPY text byte stream
        │
        ├── record framing
        ├── field splitting
        ├── NULL recognition
        └── escape decoding
                │
                ▼
          Row / FieldRef
```

Archive chunks, decompressor output chunks, COPY rows, and COPY fields are independent boundaries. The implementation must handle arbitrary short reads across all of them.

## 3. Byte-oriented field contract

The core COPY API does not require UTF-8.

```rust
pub enum FieldRef<'a> {
    Null,
    Bytes(&'a [u8]),
}
```

`FieldRef::Bytes` represents the **logical field bytes after PostgreSQL COPY text escape decoding**. It does not expose the escaped on-wire spelling.

Examples conceptually include:

```text
input field bytes   logical FieldRef
-----------------   ----------------
\N                  Null
                    Bytes(b"")
hello               Bytes(b"hello")
hello\tworld        Bytes(b"hello\tworld" with the escape decoded to a tab byte)
```

A caller that explicitly wants UTF-8 may request a fallible string conversion. Invalid UTF-8 is not a structural COPY parse failure at the byte-oriented layer.

## 4. Row and field framing

For the normal pg_dump COPY text representation targeted by v0.1:

- fields are separated according to the recorded/supported COPY text layout used by pg_dump;
- rows terminate at physical record boundaries;
- embedded control characters represented through COPY escapes must not be confused with physical row delimiters;
- an empty field is a non-NULL zero-length byte string;
- `\N` is the NULL marker when it appears as the complete unescaped field representation;
- `\.` is recognized as the COPY end-of-data marker when present as a standalone terminator record in the stream representation.

The implementation must not assume one `Read::read` call, one archive chunk, or one decompressor output buffer contains a complete row.

## 5. Escape decoding

pgdumpx must implement the PostgreSQL COPY text escape rules required for pg_dump-generated data, including the standard backslash escapes and numeric byte escapes used by PostgreSQL text COPY.

The exact accepted spellings and edge cases must follow PostgreSQL upstream behavior and be covered by tests. The parser must not silently invent a more permissive escape language.

Malformed or truncated escapes return a typed `MalformedCopy`-class error with row/byte context where practical.

## 6. Column layout metadata

For supported pg_dump-generated table-data entries, the ordered column list is derived from the TOC entry's recorded COPY statement.

Column metadata has three distinct outcomes:

```text
metadata available + column found      -> Ok(Some(index))
metadata available + column not found  -> Ok(None)
metadata unavailable/malformed         -> Err(...)
```

Representative API:

```rust
pub fn columns(&self) -> Result<&[Column], PgDumpError>;

pub fn column_index(
    &self,
    name: &[u8],
) -> Result<Option<usize>, PgDumpError>;
```

Positional row iteration may remain available when the COPY byte stream is readable but the supported column layout cannot be derived. Column-aware helpers must never guess field names.

## 7. Row ownership

Normal iteration returns a borrowed `Row` backed by a reusable buffer. The row remains valid only until the next mutable operation on the row reader.

`find_first` copies only the matched row into `OwnedRow`, allowing that result to outlive the reader without turning normal iteration into an allocating API.

## 8. Resource limits

COPY parsing is bounded by configured per-row limits such as:

- maximum row bytes;
- maximum fields per row.

Long-running scans also accept operation-level work budgets such as:

- maximum rows scanned;
- maximum decompressed bytes processed for the selected entry.

These budgets protect different resources: row limits bound individual allocations, while scan limits bound total work for an operation such as `find_first`.

Exceeding either class of limit returns a typed resource-limit error.

## 9. Required tests

At minimum, COPY parser tests must cover:

- NULL versus empty field;
- field separators and record boundaries;
- backslash escapes;
- escaped control characters;
- numeric byte escapes required by PostgreSQL COPY text;
- terminator handling;
- short reads across row and escape boundaries;
- non-UTF-8 logical field bytes;
- malformed/truncated escapes;
- row-size and field-count limits;
- supported COPY column-list extraction;
- metadata available / column missing;
- metadata unavailable or malformed;
- unsupported INSERT-based table-data representation;
- scan row/decompressed-byte budgets when COPY parsing is driven by a search operation.

## 10. Upstream governance

When COPY-related behavior is uncertain, implementation decisions should be checked against PostgreSQL source and official pg_dump-generated fixtures. A hand-built fixture is appropriate for malformed-input tests but must not be the sole evidence for valid-format semantics.
