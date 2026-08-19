# Contributing to pgdumpx

Thank you for considering a contribution.

pgdumpx is a pre-1.0 parser and extraction project. Correctness, safety, bounded memory behavior, and evidence-backed performance matter more than feature count.

## Development principles

Contributions should preserve these principles:

- derive archive behavior from PostgreSQL upstream source and generated compatibility fixtures;
- keep the parser core independent from CLI, Python, Arrow, and presentation concerns;
- treat every archive byte as untrusted;
- use checked arithmetic for attacker-controlled sizes and offsets;
- avoid project-authored `unsafe` unless an accepted ADR justifies it;
- do not allocate the complete archive or complete table data in normal streaming paths;
- keep mandatory core dependencies minimal;
- keep archive framing, decompression, and COPY parsing as separate responsibilities;
- add tests for every format edge case and bug fix;
- do not claim performance wins without a reproducible benchmark.

## Authoritative references

The PostgreSQL custom archive format is primarily defined by PostgreSQL's implementation rather than by a standalone external format specification.

Relevant upstream files include:

- `src/bin/pg_dump/pg_backup_archiver.h`
- `src/bin/pg_dump/pg_backup_archiver.c`
- `src/bin/pg_dump/pg_backup_custom.c`
- `src/bin/pg_dump/compress_io.c`

See `docs/PG-DUMP-CUSTOM-FORMAT.md` for the project's format notes and source-governance rules.

## Format changes

For a change that affects archive interpretation:

1. identify the relevant PostgreSQL upstream behavior and archive-version condition;
2. add or regenerate a fixture with a documented `pg_dump` command when possible;
3. add a focused malformed or boundary test when relevant;
4. explain compatibility impact in the PR;
5. update requirements, format notes, API docs, or ADRs if the public contract changes.

## Pull requests

Keep PRs focused. A good parser PR explains:

- which archive structure or COPY rule is implemented;
- which archive versions are affected;
- malformed-input behavior;
- resource-allocation implications;
- tests and fixtures added;
- public API or error changes;
- benchmark impact when performance is part of the change.

Avoid mixing unrelated refactors with format-semantic changes.

## Validation

The exact CI commands will be finalized when the Cargo workspace is created. The intended baseline quality gates are:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Parser changes should additionally run relevant fixture tests and fuzz/regression tests as they become available.

## Public API changes

Before v1.0 the API may evolve, but breaking changes must still be intentional and explained.

Do not expose private parser types merely for implementation convenience. Public types should serve archive inspection and extraction consumers.

Accepted policy decisions live under `docs/adr/`. Change an accepted architectural policy with a new ADR rather than silently diverging from the documentation.

## Contribution licensing

pgdumpx is licensed under **MIT OR Apache-2.0**. Unless explicitly stated otherwise, contributions intentionally submitted for inclusion in pgdumpx are provided under the same dual-license terms.

See `LICENSE-MIT` and `LICENSE-APACHE`.

## Security issues

Do not publish exploit details for a vulnerability that could materially affect users. Follow [SECURITY.md](SECURITY.md).
