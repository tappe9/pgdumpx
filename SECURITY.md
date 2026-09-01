# Security Policy

pgdumpx parses archive files that may be attacker controlled. Parser safety and resource behavior are core requirements.

## Supported versions

The v0.1 implementation and release-readiness audit are complete, but pgdumpx has no supported published release yet. This section will be updated when releases begin.

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

The default pgdumpx build inspects archives without a running PostgreSQL server, `libpq`, `pg_restore`, or another PostgreSQL executable at runtime. The release-packaging preflight verifies the default runtime dependency graph and native `links` constraints; see [Packaging and dependency constraints](docs/PACKAGING.md).

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

`pgdumpx find` applies finite defaults of 100,000 complete rows and 64 MiB of parser-consumed decompressed COPY bytes. A one-dimensional override preserves the other finite default. `--unlimited` is an explicit trusted-input opt-in that disables both total-work budgets and must not be used for unreviewed customer-supplied archives merely to bypass a resource error. See [`find` scan-budget policy](docs/FIND-SCAN-LIMITS.md).

A low-level unlimited `EntryDataReader` remains available for trusted callers. The library also provides `entry_reader_with_limits` and `copy_entry_to` bounded raw-extraction paths, and `pgdumpx extract` uses the bounded high-level path with a finite 1 GiB default rather than relying on CLI callers to wrap stdout copying themselves.

## Accounting semantics

Row-scan decompressed-byte budgets count bytes consumed by the row parser from the decompressed COPY stream, including field separators, row terminators, escape spellings, and the COPY terminator when consumed. Decoder read-ahead that has not been consumed by the parser does not affect accounting.

A maximum-row budget of `N` permits at most `N` complete rows to be yielded or evaluated. If the next row would exceed the configured row or byte budget, that row is not yielded and the operation returns a typed limit error.

Raw extraction byte limits count decompressed bytes exposed to the caller or copied to the destination. Crossing the limit fails rather than silently truncating output. Because copying is streaming, bytes written before a later limit or writer failure cannot be rolled back; the operation still returns an error and the CLI exits non-successfully.

## COPY representation boundary

The v0.1 row API parses supported pg_dump-generated COPY text table data only. INSERT-based dump modes and Binary COPY are not treated as valid COPY text merely because their containing Custom Format entry is readable.

Unsupported representations fail explicitly before row parsing where the necessary metadata is available.

See [COPY text contract](docs/COPY-TEXT.md) for the byte-level row contract.

## Dependency advisory policy

The committed `Cargo.lock` is checked with cargo-deny `0.20.2` and the repository-root `[advisories]` policy. Dependency-related pull requests and `main` pushes run the check, a daily `03:17 UTC` schedule refreshes the RustSec database without requiring a source change, and maintainers can use `workflow_dispatch` for manual validation.

When a check fails:

1. read the cargo-deny diagnostic for the advisory ID, affected crate/version, and dependency path;
2. identify whether the affected dependency is used by the library, CLI, development, benchmark, or fuzz scope;
3. prefer updating the lockfile, upgrading or replacing the dependency, or reducing the affected feature surface;
4. rerun the repository quality gates and `cargo deny --locked check advisories`;
5. do not place exploit details, sensitive crash inputs, credentials, or private archive data in public logs or issues.

An exception is a temporary last resort. `deny.toml` must use an object ignore with an advisory ID or yanked crate plus a non-empty reason. `advisory-exceptions.toml` must contain one matching record with the same reason, affected scope, removal condition, and either a review date or tracking Issue. `scripts/verify-advisory-policy.py` rejects bare IDs, incomplete or unmatched metadata, and metadata left behind after an ignore is removed. Delete both records when the removal condition is met.

For cargo-deny tool or schema updates, review the current official release and configuration documentation, update the exact version in the workflow, tests, and documentation together, install it with `--locked`, and manually dispatch the workflow before merging. License, source allowlist, and duplicate-version checks remain outside this policy.

## Fuzzing

The baseline invariants are:

```text
arbitrary archive bytes -> successful parse/extraction or typed error, never parser panic
arbitrary COPY bytes    -> rows or typed error, never parser panic
```

The committed `cargo-fuzz` harnesses cover raw archive opening, TOC/metadata parsing with structural limits, selected-entry block/chunk framing, COPY row/escape parsing, COPY column metadata, and structural/scan/raw-output limit accounting. Each harness uses bounded inputs and the normal production parser/limit paths.

Pull-request CI compiles all six targets and retains the short deterministic 64-run smoke gate. A dedicated workflow runs all six targets weekly and on manual dispatch with a finite five-minute budget per target, 64 KiB maximum inputs, a 10-second individual-input timeout, and a 15-minute job timeout. Relevant branch pushes use a 10-second target budget to validate the workflow without turning pull-request CI into a long campaign.

The campaign runner saves each command's exit status and execution log, allows the `if: always()` artifact step to collect target failure artifacts and logs, then re-emits the original status. Crash, hang, sanitizer, and other non-zero results therefore remain job failures. Artifacts are target/commit/run-specific, retained for 7 days, and exclude corpus evolution.

The repository campaign may use only committed, reviewed, non-sensitive seeds and generated mutations. Actions artifacts are not a confidential vulnerability-disclosure channel: never introduce production dumps, customer data, credentials, personal data, proprietary inputs, or embargoed vulnerability material into the scheduled workflow. Security-sensitive findings must move to private reporting before payloads or exploit details are shared.

When fuzzing finds a panic, hang, sanitizer finding, or boundary defect, minimize the input and add a deterministic failing regression test through the same production path before changing production code. A discovered corpus input may be committed only after provenance, checksum, minimization, public-distribution safety, and reviewer approval are recorded. Reproducible commands, target frequency, bounds, artifact handling, triage, and corpus policy are documented in [`fuzz/README.md`](fuzz/README.md).
