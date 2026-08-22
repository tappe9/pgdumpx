# Bounded raw entry extraction

Status: **Implemented v0.1 contract**

`pgdumpx` exposes selected custom-archive entry bodies as decompressed byte streams. Raw extraction is deliberately separate from structural metadata limits and row-scan limits: it bounds the number of decompressed bytes exposed by one selected entry without changing COPY parsing semantics.

## Library API

`EntryReadLimits` configures the raw-entry decompressed-byte budget used by `Archive::entry_reader_with_limits` and `Archive::copy_entry_to`.

- The counter measures decompressed bytes returned or copied from the selected entry body.
- A limit of `N` permits exactly `N` bytes when the entry ends there.
- If byte `N + 1` exists, the operation fails with `PgDumpError::EntryDecompressedByteLimitExceeded`; crossing the limit is never reported as clean EOF or successful truncation.
- The bounded `Read` path preserves the typed `PgDumpError` as the source of its `std::io::Error`. The higher-level copy path maps that source back to the typed error taxonomy.
- Byte accounting uses checked arithmetic and reports `PgDumpError::EntryDecompressedByteCountOverflow` on overflow.
- `EntryReadLimits::unlimited()` remains available to trusted library callers that intentionally choose an unbounded raw-entry read.

`Archive::copy_entry_to` uses the same bounded reader path rather than maintaining a second limit implementation. It writes incrementally and is binary-safe. If the destination or input fails after some bytes have been written, those bytes cannot be rolled back; the operation still returns an error rather than success.

## CLI

```text
pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>
```

The selector uses the same exact `SCHEMA.TABLE` grammar as `pgdumpx find`: one ASCII `.` separator, two non-empty components, and no SQL identifier quoting.

`extract` resolves the requested table through the library metadata index, selects only its related `TABLE DATA` entry, and writes only that entry's decompressed body to stdout. It does not synthesize DDL, a COPY statement wrapper, or unrelated archive entries. Output bytes are not interpreted as UTF-8; diagnostics go to stderr.

### Default raw-output budget

When `--max-decompressed-bytes` is omitted, the CLI applies a finite default of **1,073,741,824 bytes (1 GiB)** per extraction.

The committed PostgreSQL 18 compatibility fixtures used by the repository produce a 270-byte selected `TABLE DATA` body for both none and gzip compression. A 1 GiB default therefore leaves substantial compatibility headroom over that validated fixture while still preventing an omitted option from becoming an implicit unbounded decompression request. Users who intentionally need a larger trusted extraction can provide a larger positive `u64` value explicitly.

The override must be a positive `u64`; zero, negative, malformed, overflowing, duplicate, or unknown limit options are usage errors.

### Failure and partial-output semantics

The CLI treats limit exhaustion, archive errors, decompression errors, and destination write failures as non-success outcomes. Because stdout is streamed, bytes already written before a later failure cannot be withdrawn. `pgdumpx` flushes already-produced stdout before reporting the extraction failure on stderr, so downstream consumers can observe partial output but must use the process exit status to decide whether extraction completed successfully.
