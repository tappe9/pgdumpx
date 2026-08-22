# Packaging and dependency constraints

Status: **v0.1 package, license, dependency, and runtime boundary verified**

This document records the v0.1 publish-time package and dependency boundary verified by Issue #35 / PR #54. The final cross-document Definition of Done mapping is recorded separately in `V0.1-RELEASE-AUDIT.md`.

## Intended package set

The workspace contains two intended crates.io packages; neither is published by this audit:

- `pgdumpx` — the reusable library;
- `pgdumpx-cli` — the `pgdumpx` command-line binary, depending on `pgdumpx = "0.1.0"` with library default features disabled and CLI features forwarded explicitly.

Both packages inherit the workspace version, edition, Rust version, license, repository, and README metadata. For v0.1 these are:

- version: `0.1.0`;
- edition: `2024`;
- MSRV: Rust `1.85.0`;
- license expression: `MIT OR Apache-2.0`;
- repository: `https://github.com/tappe9/pgdumpx`;
- README: the workspace `README.md`.

The package file list contains `README.md`, `LICENSE-MIT`, and `LICENSE-APACHE`. The package-local license copies exactly match the repository-root license texts. Repository fixtures, generated dump archives, and benchmark datasets are not distribution inputs and do not appear in either `.crate` package.

## Compression and feature boundary

The v0.1 default CLI supports all four archive compression modes required by the compatibility contract:

| Archive compression | Implementation | Feature behavior | Native/runtime constraint |
| --- | --- | --- | --- |
| none | pgdumpx framing/streaming path | always available | none |
| gzip | `flate2` with `default-features = false`, `rust_backend` | always available | no system zlib requirement |
| LZ4 | `lz4_flex` with default features disabled | CLI default feature `lz4`; library optional feature | no Cargo native `links` dependency in the accepted default graph |
| Zstandard | `ruzstd = 0.8.1` | CLI default feature `zstd`; library optional feature | no Cargo native `links` dependency in the accepted default graph |

The project does not use “Pure Rust” as a blanket promise for all future transitive dependencies. Instead, the release audit rejects a new default-runtime Cargo package with a non-empty `links` field until the build/distribution constraint is explicitly reviewed and documented.

The library remains coherent with:

```text
default features
--no-default-features
--no-default-features --features lz4
--no-default-features --features zstd
```

`flate2`/gzip remains part of the base library dependency set; the feature switches control the optional LZ4 and Zstandard decoders.

## PostgreSQL runtime boundary

The production library and CLI read the custom archive format directly. The default runtime dependency graph does not contain PostgreSQL client/runtime packages such as `libpq`, `pq-sys`, `postgres`, or `tokio-postgres`.

A running PostgreSQL server, `libpq`, `pg_restore`, `pg_dump`, `psql`, or another PostgreSQL executable is not required to run the v0.1 library or CLI. PostgreSQL tools may still be used by repository-only fixture generation or differential-test scripts; those development/test tools are not runtime dependencies of the packaged crates.

## Dependency license policy

Release packaging audits the normal runtime dependency closure of the default `pgdumpx-cli` build using `cargo metadata --locked`. Every runtime package must provide license metadata and its SPDX expression must have a distributable branch composed only of the accepted permissive identifiers enforced by `scripts/verify-release-packaging.py`.

The initial allow-set covers the permissive licenses used by the accepted dependency closure, including MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, 0BSD/MIT-0, and Unlicense forms. An SPDX `WITH` exception or a dependency outside that set fails the audit and requires an explicit review rather than being silently accepted.

This is a distribution-compatibility gate, not legal advice. `Cargo.lock` is the version/checksum record for the exact audited dependency resolution.

## Project-authored unsafe policy

The workspace sets:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
```

Both release crates inherit workspace lints. The packaging audit verifies that this remains true; normal all-target/all-feature CI compilation then enforces the policy across project-authored Rust code.

## Publish-preflight verification

Run from a clean checkout:

```bash
python3 scripts/verify-release-packaging.py
```

The audit performs all of the following without publishing:

1. loads locked Cargo metadata and verifies package metadata;
2. verifies both package-local license files are byte-for-byte copies of the repository-root licenses;
3. traverses the default CLI normal-runtime dependency graph;
4. checks dependency license expressions, PostgreSQL runtime exclusions, and native `links` constraints;
5. checks `cargo package --list` for both intended packages;
6. verifies required README/license files and rejects fixture/benchmark data in package contents;
7. checks the default and reduced-feature library builds plus the default CLI compression contract;
8. runs `cargo package --workspace --locked` so current stable Cargo assembles and verifies both interdependent release packages together before `pgdumpx 0.1.0` exists on crates.io;
9. verifies that the source tree remains unchanged by the preflight.

Release packaging CI intentionally follows the repository's stable toolchain so it can use current Cargo's multi-package packaging support. The v0.1 MSRV remains Rust 1.85.0 and is enforced separately by the normal workspace CI.

Publishing crates, creating tags, and creating release artifacts remain outside this preflight and outside the Issue #36 audit.
