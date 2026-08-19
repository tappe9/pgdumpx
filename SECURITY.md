# Security Policy

pgdumpx parses archive files that may be attacker controlled. Parser safety and resource behavior are core requirements.

## Supported versions

pgdumpx is currently in the pre-release implementation stage and has no supported published version yet. This section will be updated when releases begin.

## Reporting a vulnerability

Please do not include exploit details or proof-of-concept payloads in a public issue when the problem could affect users.

Preferred path:

1. use GitHub private vulnerability reporting / Security Advisories for this repository when available;
2. if private reporting is unavailable, open a minimal public issue asking for a private contact path without including vulnerability details.

Useful information includes:

- affected commit or version;
- archive version and compression algorithm when relevant;
- input conditions required to trigger the issue;
- impact such as panic, excessive allocation, CPU exhaustion, invalid seek, decompression misuse, or incorrect row parsing;
- minimized reproduction data if safely shareable;
- whether the issue appears exploitable beyond denial of service.

## Security assumptions

pgdumpx assumes every archive byte is untrusted.

Security-sensitive properties include:

- no out-of-bounds reads;
- checked offset, length, and counter arithmetic;
- no parser panic caused by malformed archive structure;
- no allocation proportional to an unvalidated declared size;
- configurable metadata and per-row budgets;
- configurable total row-scan/decompression work budgets for applications that process hostile input;
- a library-provided bounded path for raw decompressed entry extraction;
- validation of block type and dump ID after seeking to a stored offset;
- validation of supported table-data representation before invoking the COPY row parser;
- bounded streaming buffers;
- explicit errors for unsupported archive versions, compression modes, and logical table-data representations.

## Runtime and dependency boundary

The default pgdumpx build is intended to inspect archives without a running PostgreSQL server, `libpq`, `pg_restore`, or other PostgreSQL executables at runtime.

The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency. Compression backend choices must be documented when they introduce material native build, distribution, or sandboxing implications. Project-authored `unsafe` remains prohibited unless a separately accepted ADR documents invariants and verification.

See [ADR 0007](docs/adr/0007-standalone-row-scanner-and-vertical-slices.md).

## Resource exhaustion and decompression bombs

Memory-bounded streaming alone does not make an operation computationally bounded. An input can contain a very large number of small rows or decompress to a large byte stream while staying below every individual row-size limit.

pgdumpx therefore distinguishes three classes of limits:

```text
structural / per-item limits
  - TOC entries
  - metadata string bytes
  - dependencies per entry
  - row bytes
  - fields per row

row-scan work limits
  - rows fully parsed/evaluated
  - decompressed bytes consumed by the row parser

raw extraction limits
  - decompressed bytes returned or copied from one selected entry
```

Configured limits are enforced incrementally on the normal streaming path and terminate with a typed resource-limit error when exceeded. They do not require pre-reading or buffering the complete entry.

Applications processing trusted local backups may choose generous limits. Applications processing untrusted or customer-supplied archives should configure limits appropriate to their service-level and resource constraints.

The low-level unlimited `EntryDataReader` may remain available for trusted callers, but the library and CLI must also provide a bounded raw-extraction path. `pgdumpx extract` must use the bounded path rather than relying on each CLI caller to wrap stdout copying correctly.

## Accounting semantics

Row-scan decompressed-byte budgets count bytes consumed by the row parser from the decompressed COPY stream, including field separators, row terminators, and the COPY terminator when consumed. Decoder read-ahead that has not been consumed by the parser must not make the result depend on buffer size.

A maximum-row budget of `N` permits at most `N` complete rows to be yielded or evaluated. If the next row would exceed the configured row or byte budget, that row is not yielded and the operation returns a typed limit error.

Raw extraction byte limits count decompressed bytes exposed to the caller or copied to the destination. Crossing the limit must fail rather than silently truncate output, unless a separately named truncating API is introduced in the future.

## COPY representation boundary

The v0.1 row API parses supported pg_dump-generated COPY text table data only. INSERT-based dump modes and Binary COPY are not treated as valid COPY text merely because their containing Custom Format entry is readable.

Unsupported representations must fail explicitly before row parsing where the necessary metadata is available.

See `docs/COPY-TEXT.md` for the byte-level row contract.

## Fuzzing

The planned baseline invariants are:

```text
arbitrary archive bytes -> successful parse/extraction or typed error, never parser panic
arbitrary COPY bytes    -> rows or typed error, never parser panic
```

Fuzzing should include structural limits, row limits, raw extraction limits, malformed COPY escape boundaries, unsupported-representation transitions, and checked scan counters.

Security-relevant regression inputs should remain in the permanent test corpus after fixes.
