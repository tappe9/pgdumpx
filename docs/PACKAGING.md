# Packaging and dependency constraints

Status: **0.2.0 publication preflight; registry publication pending credentials**

This document records the package, dependency, and staged-verification boundary for the first public release.

## Package set and versioning

The workspace publishes two crates in dependency order:

- `pgdumpx 0.2.0` — the reusable library;
- `pgdumpx-cli 0.2.0` — the `pgdumpx` command-line binary.

Both packages inherit edition 2024, Rust 1.85.0, the `MIT OR Apache-2.0` license expression, repository, homepage, and root README metadata. Each package declares crates.io discovery metadata and is restricted to the `crates-io` registry. The CLI dependency is:

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

1. validates version, package metadata, the CLI path-plus-version dependency, licenses, and workspace lint inheritance;
2. audits the locked default runtime dependency closure and accepted license expressions;
3. checks package file lists for required and forbidden content;
4. exercises default, no-optional, LZ4-only, and Zstandard-only library builds;
5. runs the complete `pgdumpx-cli` test suite, including command, exit-code, limit, and compression behavior;
6. installs the CLI into an isolated temporary root and verifies `--version`, `--help`, and command discovery;
7. runs `cargo package --workspace --locked` and inspects both `.crate` sizes;
8. runs `cargo publish --dry-run -p pgdumpx --locked`;
9. verifies that the source tree remains unchanged.

The CLI package is assembled and build-verified through workspace packaging before the library exists in crates.io. Its final publication dry-run is intentionally staged after `pgdumpx 0.2.0` is visible in the registry:

```bash
cargo publish --dry-run -p pgdumpx-cli --locked
```

See `RELEASING.md` for the complete publication order, registry verification, tag creation, and recovery procedure. The preflight never uploads packages, creates tags, or creates a GitHub Release.
