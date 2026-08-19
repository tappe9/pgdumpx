# ADR 0003: Eager metadata index, lazy streaming payload readers

- Status: Accepted
- Date: 2026-08-19

## Context

The target workload includes multi-gigabyte archives where loading every table into memory is unacceptable. At the same time, the TOC is small enough relative to payloads to make metadata indexing useful for repeated lookups.

Custom archives record entry data positions when available, allowing seekable readers to access selected entries directly.

## Decision

`Archive::open` will eagerly parse the header and TOC into a compact `ArchiveIndex` but will not read/decompress all payloads.

Payload access is lazy:

1. resolve a TOC entry;
2. validate that it has a usable data offset;
3. seek to that offset;
4. validate block type and dump ID;
5. expose a streaming decompressed `Read` implementation;
6. optionally compose it with the COPY row parser.

The primary v0.1 source bound is `Read + Seek`.

## Consequences

### Positive

- archive open time and memory do not scale with total payload bytes;
- metadata queries are cheap after open;
- selected table extraction does not require reading unrelated entries;
- the same streaming reader can feed CLI output or row parsing.

### Negative

- one `Archive<R>` cannot safely expose multiple simultaneous readers that independently seek the same `R`;
- non-seekable inputs need a different future API/fallback;
- TOC metadata still scales with number of archive objects.

## Future parallelism

Parallel file extraction should use independently opened or cloned seekable sources rather than mutex-sharing one cursor.
