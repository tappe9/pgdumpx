# Bounded raw entry extraction

Status: **Implemented v0.1 single-entry contract plus v0.2 sequential plan execution**

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

## Sequential extraction plans

`ExtractionPlan::execute` extends the same raw-copy path to an ordered set of selected tables without weakening the archive's single mutable seek invariant.

Execution has two distinct phases:

1. The complete logical plan is preflighted against archive metadata. Every selector must resolve to a table and related `TABLE DATA` entry before the first destination is requested or written.
2. Archive-specific target IDs are copied into owned execution targets, ending the metadata borrow. Targets are then processed strictly in plan order on the same mutable `Archive`, and each target delegates to `Archive::copy_entry_to` with the plan's existing `EntryReadLimits`.

The destination callback supplies a `Write` sink for each resolved target. It is intentionally not a filesystem policy layer: path layout, filename escaping, overwrite behavior, combined-output framing, and atomic replacement remain outside this API. The executor does not buffer a complete selected entry or an aggregate multi-table payload.

Each successful target returns its selector identity, resolved `TABLE`/`TABLE DATA` dump IDs, and copied decompressed-byte count. If execution fails after preflight, `ExtractionExecutionError` retains the fully completed earlier outcomes, identifies the current target, and preserves the existing `PgDumpError` as its source. Bytes already accepted by the failing target remain partial output, and no later destination is started. A destination-creation I/O failure is reported as an output error with zero bytes written for that target.

`EntryReadLimits` applies independently to each selected entry because every target uses a fresh bounded `copy_entry_to` operation. Limit exhaustion is therefore a target failure, never successful truncation or an aggregate-plan byte cap.

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
