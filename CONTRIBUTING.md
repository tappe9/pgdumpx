# Contributing to pgdumpx

Thank you for considering a contribution.

pgdumpx is a pre-1.0 parser and extraction project. Correctness, safety, bounded memory/work behavior, and evidence-backed performance matter more than feature count.

## Development principles

Contributions should preserve these principles:

- derive archive behavior from PostgreSQL upstream source and generated compatibility fixtures;
- keep the parser core independent from CLI, Python, Arrow, PostgreSQL server processes, `libpq`, and presentation concerns;
- treat every archive byte as untrusted;
- use checked arithmetic for attacker-controlled sizes, offsets, and counters;
- avoid project-authored `unsafe` unless an accepted ADR justifies it;
- do not allocate the complete archive or complete table data in normal streaming paths;
- provide bounded paths for both row scans and raw decompressed entry extraction;
- keep mandatory core dependencies minimal and document native build/runtime constraints;
- keep archive framing, decompression, COPY parsing, and presentation as separate responsibilities;
- add tests for every format edge case and bug fix;
- do not claim performance wins without a reproducible benchmark.

## Product boundary

pgdumpx is a bounded, byte-oriented row scanner for PostgreSQL Custom Format archives. TOC lookup, selective seeking, and decompression are foundations of that workflow; the primary user value is safe row/field inspection and search without restore.

The project implements its own narrow read path rather than using another dump library as a mandatory backend. Related projects remain valuable references and differential-test comparators. See [ADR 0007](docs/adr/0007-standalone-row-scanner-and-vertical-slices.md).

## Authoritative references

The PostgreSQL custom archive format is primarily defined by PostgreSQL's implementation rather than by a standalone external format specification.

Relevant upstream files include:

- `src/bin/pg_dump/pg_backup_archiver.h`
- `src/bin/pg_dump/pg_backup_archiver.c`
- `src/bin/pg_dump/pg_backup_custom.c`
- `src/bin/pg_dump/compress_io.c`

See `docs/PG-DUMP-CUSTOM-FORMAT.md` for the project's format notes and source-governance rules.

## Documentation ownership

Each document has one primary responsibility. Avoid copying normative text into several files when a link is sufficient.

| Document | Primary responsibility |
|---|---|
| `README.md` / `README.ja.md` | Product value, status, quick examples, high-level scope |
| `docs/REQUIREMENTS.md` | Normative v0.1 behavior and acceptance criteria |
| `ARCHITECTURE.md` | Internal boundaries, data flow, and safety architecture |
| `docs/API-DESIGN.md` | Intended public Rust API and exact API semantics |
| `docs/PG-DUMP-CUSTOM-FORMAT.md` | Upstream-derived archive-format notes |
| `docs/COPY-TEXT.md` | COPY text byte and row contract |
| `docs/COMPATIBILITY.md` | Target versus fixture-verified support and fixture provenance |
| `ROADMAP.md` | Delivery order and release sequencing |
| `docs/adr/` | Accepted design decisions and supersession history |

When a change affects more than one responsibility, update the smallest complete set and check terminology across those documents before opening a PR.

## Format changes

For a change that affects archive interpretation:

1. identify the relevant PostgreSQL upstream behavior and archive-version condition;
2. add or regenerate a fixture with a documented `pg_dump` command when possible;
3. record generator version, command, checksum, purpose, and expected objects in the fixture manifest;
4. add a focused malformed or boundary test when relevant;
5. explain compatibility impact in the PR;
6. update format notes, requirements, API docs, compatibility status, or ADRs when their owned contract changes.

## Fixture policy

Valid-format compatibility evidence should come from official `pg_dump` output. Hand-built fixtures are appropriate for malformed states that an official writer cannot produce, but they must not be the sole evidence for normal behavior.

The fixture manifest is expected to record fields equivalent in purpose to:

```toml
[[fixture]]
name = "pg18-gzip-copy-basic"
path = "tests/fixtures/archives/pg18-gzip-copy-basic.dump"
archive_version = "1.16.0"
generator = "pg_dump (PostgreSQL) 18.x"
command = "pg_dump -Fc --compress=gzip:6 --file=..."
sha256 = "<recorded checksum>"
purpose = ["header", "toc", "gzip", "copy-text"]
expected_tables = ["public.orders"]
```

The committed Alpha 1 inventory and exact regeneration process are documented in [tests/fixtures/README.md](tests/fixtures/README.md).

Large benchmark data should normally be generated reproducibly rather than committed.

## Pull requests

Keep PRs focused. A good parser PR explains:

- which archive structure or COPY rule is implemented;
- which archive versions and compression modes are affected;
- malformed-input behavior;
- allocation and total-work implications;
- tests and fixtures added;
- public API or error changes;
- benchmark impact when performance is part of the change;
- documentation files updated and why each owns part of the change.

Avoid mixing unrelated refactors with format-semantic changes.

## Validation

The workspace uses Rust 2024 edition. `rust-toolchain.toml` selects the current stable toolchain with `rustfmt` and `clippy`; the minimum supported Rust version is 1.85.0.

Run the same baseline quality gates used by CI before opening a pull request:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Verify the declared MSRV separately:

```bash
cargo +1.85.0 check --workspace --all-targets --all-features
```

Parser changes should additionally run relevant fixture, differential, fuzz/regression, and benchmark checks as they become available.

Documentation-only changes should verify at least:

- all relative links resolve;
- English and Japanese README product claims remain aligned;
- accepted/superseded ADR references are consistent;
- no compatibility cell is marked verified without fixture evidence;
- CLI command, output, encoding, and exit-code contracts do not conflict across documents.

## Public API changes

Before v1.0 the API may evolve, but breaking changes must still be intentional and explained.

Do not expose private parser types or public struct fields merely for implementation convenience. Public metadata types should be opaque with accessors where that preserves future compatibility. Public enums expected to grow should be `#[non_exhaustive]` before v1.0.

The streaming row API intentionally uses `next_row(&mut self)` rather than a standard `Iterator` when rows borrow a reusable internal buffer. Document that lifetime boundary instead of forcing per-row ownership to satisfy `Iterator`.

Accepted policy decisions live under `docs/adr/`. Change an accepted architectural policy with a new ADR rather than silently diverging from the documentation.

## Contribution licensing

pgdumpx is licensed under **MIT OR Apache-2.0**. Unless explicitly stated otherwise, contributions intentionally submitted for inclusion in pgdumpx are provided under the same dual-license terms.

See `LICENSE-MIT` and `LICENSE-APACHE`.

## Security issues

Do not publish exploit details for a vulnerability that could materially affect users. Follow [SECURITY.md](SECURITY.md).
