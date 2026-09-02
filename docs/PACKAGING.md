# Packaging and dependency constraints

Status: **0.2.0 published on crates.io**

This document records the package, dependency, staged-verification, and publication boundary for the first public release.

## Published 0.2.0 record

The two workspace packages were published in dependency order and remain non-yanked:

- `pgdumpx 0.2.0` — published 2026-09-01 12:36:29 UTC;
- `pgdumpx-cli 0.2.0` — published 2026-09-01 12:36:56 UTC.

The crates.io package checksums recorded by the registry index are:

```text
pgdumpx      0.2.0  ef4d0cb73fdf21a87dd7e2515adf83cdf4415e13707dfed41bd9ad4576e9dd6b
pgdumpx-cli  0.2.0  9a6f0d8f690d4ab65bc9a2e5079397966b12d473629bdd21cfc939bcb56414fe
```

Published package pages:

- <https://crates.io/crates/pgdumpx/0.2.0>
- <https://crates.io/crates/pgdumpx-cli/0.2.0>

The matching source release is [`v0.2.0`](https://github.com/tappe9/pgdumpx/releases/tag/v0.2.0), created from commit `aaae67749e3f389bdde5c28f555436508e219fc8` after both packages were visible in the registry. Published crate archives and versions are immutable; later corrections require a new version unless yanking is justified by a severe correctness or security defect.

## Package set and versioning

The workspace publishes two crates in dependency order:

- `pgdumpx 0.2.0` — the reusable library;
- `pgdumpx-cli 0.2.0` — the package that installs one executable named `pgdumpx`.

Both packages inherit edition 2024, Rust 1.85.0, the `MIT OR Apache-2.0` license expression, repository, homepage, and root README metadata. Each package declares crates.io discovery metadata and is restricted to the `crates-io` registry. The CLI disables automatic binary discovery so `src/main.rs` remains an internal implementation module rather than a second published executable. Its library dependency is:

```toml
pgdumpx = { path = "../pgdumpx", version = "0.2.0", default-features = false }
```

Cargo uses the path while developing in the workspace and the matching version requirement in the published package.

## Package contents

The package audit checks each `cargo package --list` result. Every package must include `README.md`, `LICENSE-MIT`, and `LICENSE-APACHE`; package-local license copies must match the repository-root texts byte-for-byte.

Package archives must not include fixture directories, dump archives, benchmark datasets, workflow files, repository scripts, credentials, local environment files, or planning artifacts. The resulting `.crate` files are inspected after `cargo package --workspace --locked`, must be non-empty, and must remain below the registry package-size boundary enforced by the audit.

## Compression and feature boundary

The default CLI supports the four verified archive compression modes:

| Archive compression | Implementation | Feature behavior | Runtime constraint |
| --- | --- | --- | --- |
| none | archive framing and streaming path | always available | none |
| gzip | `flate2` with `rust_backend` | always available | no system zlib requirement |
| LZ4 | `lz4_flex` frame decoder | CLI default feature `lz4`; library optional feature | no accepted Cargo `links` dependency |
| Zstandard | `ruzstd 0.8.1` | CLI default feature `zstd`; library optional feature | no accepted Cargo `links` dependency |

The library remains coherent with default features, no optional compression, LZ4 only, and Zstandard only. Disabled optional backends remain recognizable in metadata and return the typed unsupported-compression error when entry decoding is requested.

## Runtime dependency boundary

The library and CLI read the archive format directly. The accepted runtime dependency graph contains no PostgreSQL client package, `libpq`, native compression library, running server, or PostgreSQL executable requirement. PostgreSQL tools are used only by repository fixture generation and compatibility differential checks.

The audit traverses the default CLI normal-runtime dependency graph through locked Cargo metadata. Every runtime dependency must provide an accepted distributable SPDX branch. A new license exception, native `links` package, or PostgreSQL runtime dependency fails the preflight until explicitly reviewed and documented.

The workspace forbids project-authored `unsafe`, and both release crates inherit the workspace lint policy.

## Publish-preflight verification

Run from a clean checkout:

```bash
python3 scripts/verify-release-packaging.py
```

The audit performs all of the following without uploading a package:

1. validates version, package metadata, the single CLI binary target, the CLI path-plus-version dependency, licenses, and workspace lint inheritance;
2. audits the locked default runtime dependency closure and accepted license expressions;
3. checks package file lists for required and forbidden content;
4. exercises default, no-optional, LZ4-only, and Zstandard-only library builds;
5. runs the complete `pgdumpx-cli` test suite, including command, exit-code, limit, and compression behavior;
6. installs the CLI into an isolated temporary root and verifies `--version`, `--help`, and command discovery;
7. runs `cargo package --workspace --locked` and inspects both `.crate` sizes;
8. runs `cargo publish --dry-run -p pgdumpx --locked`;
9. verifies that the source tree remains unchanged.

The CLI package is assembled and build-verified through workspace packaging before a new library version exists in crates.io. Its final publication dry-run is intentionally staged after the matching `pgdumpx` version is visible in the registry:

```bash
cargo publish --dry-run -p pgdumpx-cli --locked
```

See [RELEASING.md](RELEASING.md) for the complete publication order, registry verification, tag creation, and recovery procedure. The preflight never uploads packages, creates tags, or creates a GitHub Release.
