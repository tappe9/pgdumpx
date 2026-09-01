# Continuous integration responsibilities

The repository separates platform coverage from library feature coverage so each concern is exercised completely without expanding to the full operating-system × feature Cartesian product.

## Baseline quality gates

The `quality` job runs on Linux and verifies formatting, documentation links, the advisory-policy contract, the scheduled-fuzz orchestration contract, workspace-wide warning-denying Clippy, all-feature workspace tests, and warning-denying rustdoc generation.

The `msrv` job checks every workspace target with Rust 1.85.0 and all features enabled.

## Platform coverage

The `platform-stable` matrix runs the all-feature workspace test suite on Linux, macOS, and Windows. It also verifies that the default CLI build supports every compression backend included by default.

Platform jobs intentionally use the all-feature configuration. They are not multiplied by every reduced-feature configuration.

## Library feature coverage

The Linux `feature-matrix` job runs the complete applicable `pgdumpx` library test suite and warning-denying Clippy for each supported configuration:

```text
default
--no-default-features
--no-default-features --features lz4
--no-default-features --features zstd
```

Backend-specific test imports, helpers, fixtures, and enabled-backend tests must use precise `#[cfg(...)]` gates. Tests that verify an archive remains inspectable while a disabled backend reports `UnsupportedEntryCompression` remain active in configurations where that backend is unavailable.

A feature-specific test must not be skipped by excluding its whole test target from a matrix job. Add a precise compile-time gate to only the code that requires the feature.

## Dependency advisory coverage

`.github/workflows/dependency-advisories.yml` owns Rust dependency advisory checks. It runs on dependency-policy or lockfile changes, daily at `03:17 UTC`, and through `workflow_dispatch`.

The workflow has read-only contents permission, a 20-minute job timeout, and concurrency cancellation for duplicate runs. It validates `deny.toml` against `advisory-exceptions.toml`, installs cargo-deny `0.20.2` with `--locked`, and runs:

```bash
cargo deny --locked check advisories
```

The committed `Cargo.lock` is checked against the current RustSec database. Normal cargo-deny diagnostics remain visible so failures identify the advisory ID and affected dependency. The policy intentionally does not add license, source allowlist, or duplicate-version checks.

## Fuzz coverage

The `Fuzz target smoke` job in `.github/workflows/ci.yml` remains the pull-request latency gate. It builds the six maintained targets and runs each for exactly 64 iterations with `-max_len=65536` and `-timeout=2`.

`.github/workflows/scheduled-fuzz.yml` owns longer coverage-guided campaigns. It runs all six targets every Sunday at `03:29 UTC`, supports `workflow_dispatch`, and performs a short path-filtered push validation when the workflow, runner, contract tests, or `fuzz/**` changes.

| Trigger | Per-target campaign time | Purpose |
|---|---:|---|
| Scheduled | 300 seconds | Weekly coverage-guided campaign for every target |
| Manual dispatch | 300 seconds | Maintainer-requested full campaign |
| Relevant branch push | 10 seconds | Live workflow/script validation before or after merge |

Every target uses `-max_len=65536`, a 10-second individual-input timeout, and a 15-minute job timeout. The matrix has `fail-fast: false` so one finding does not prevent other targets from producing evidence. Workflow concurrency cancels redundant overlapping runs for the same schedule/ref group.

`scripts/run-fuzz-campaign.py` validates target-owned paths, records the fuzz command's combined log, exit status, and non-sensitive metadata, and returns temporarily so the `if: always()` artifact step can run. A final `propagate` step re-emits the saved status; crashes, hangs, sanitizer findings, and other non-zero results therefore fail the job after evidence upload rather than being swallowed.

Each artifact name contains the target, commit SHA, and workflow run ID. The workflow retains `fuzz/artifacts/<target>/` and `fuzz/campaign-results/<target>/` for 7 days, but does not upload or cache the evolving corpus. The workflow is restricted to repository-reviewed, non-sensitive seeds; `fuzz/README.md` owns triage, minimization, regression-first, provenance, and sensitive-artifact procedures.

## Specialized evidence jobs

The remaining jobs separately verify fixture provenance and compatibility, differential behavior against PostgreSQL tooling, benchmark target buildability, and release packaging.

GitHub Actions used by repository-owned workflows must be pinned to immutable commit SHAs. Rust tools installed by CI use exact versions and `--locked` where supported. Action or tool updates should be reviewed as normal dependency changes rather than accepted through floating tags.
