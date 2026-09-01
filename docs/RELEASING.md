# Release process

This document defines the staged publication procedure for `pgdumpx 0.2.0`, the first public release of both workspace packages.

## Release identity

| Item | Value |
| --- | --- |
| Library package | `pgdumpx 0.2.0` |
| CLI package | `pgdumpx-cli 0.2.0` |
| Publication order | library, registry confirmation, CLI |
| Git tag | `v0.2.0` |
| GitHub Release name | `pgdumpx 0.2.0` |
| Release notes | `docs/release-notes/0.2.0.md` |
| Changelog range | repository inception through the release commit |

Both packages use the same version for the initial release train. The CLI retains a path dependency for workspace development and a matching `0.2.0` version requirement for published packages.

## Preconditions

Use a clean checkout of the exact release commit on `main`. Confirm that there are no release-blocking Issues or pull requests and that CI, dependency advisories, scheduled-fuzz policy, and Release Packaging are green for that commit.

Crates.io publication requires a verified crates.io account and a scoped API token. Store it with `cargo login`; never commit the token or place it in command history, logs, Issues, or pull requests.

Record the release commit before running the gates:

```bash
git switch main
git pull --ff-only
RELEASE_SHA="$(git rev-parse HEAD)"
git status --short --branch
python3 scripts/test-release-readiness.py
```

The status output must show a clean `main` synchronized with `origin/main`.

## Local release gates

Run every gate from the same clean commit:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo +1.85.0 check --workspace --all-targets --all-features
python3 scripts/verify-release-packaging.py
```

The packaging preflight verifies metadata, licenses, dependency boundaries, package contents and sizes, all CLI tests, an installed CLI smoke test, workspace packaging, and the library publication dry-run. Review the emitted package file lists before continuing.

## Stage 1: publish the library

Re-run the library dry-run immediately before upload:

```bash
cargo publish --dry-run -p pgdumpx --locked
cargo publish -p pgdumpx --locked
cargo info pgdumpx@0.2.0
```

If `cargo publish` times out while waiting for the index, do not retry the upload blindly. Run `cargo info pgdumpx@0.2.0` until the registry either confirms the version or clearly shows that it was not accepted.

## Stage 2: publish the CLI

Only continue after `cargo info pgdumpx@0.2.0` resolves the exact library version from crates.io:

```bash
cargo publish --dry-run -p pgdumpx-cli --locked
cargo publish -p pgdumpx-cli --locked
cargo info pgdumpx-cli@0.2.0
```

The pre-publication repository gate uses `cargo package --workspace --locked`, the complete CLI test suite, and `cargo install --path` because the published CLI dry-run cannot resolve its registry dependency until the library version exists in the crates.io index.

## Stage 3: create the source release

Do not create the tag or GitHub Release until both package versions are independently visible in crates.io.

Verify that the working tree and release commit have not changed, then create an annotated tag and a Release from the committed notes:

```bash
test "$(git rev-parse HEAD)" = "$RELEASE_SHA"
git status --porcelain
git tag -a v0.2.0 -m "pgdumpx 0.2.0" "$RELEASE_SHA"
git push origin v0.2.0
gh release create v0.2.0 \
  --verify-tag \
  --title "pgdumpx 0.2.0" \
  --notes-file docs/release-notes/0.2.0.md
```

Verify the final state:

```bash
git rev-list -n 1 v0.2.0
gh release view v0.2.0
cargo info pgdumpx@0.2.0
cargo info pgdumpx-cli@0.2.0
```

The tag must resolve to `RELEASE_SHA`. docs.rs builds are asynchronous; verify the `pgdumpx 0.2.0` documentation after the registry has accepted the package. The CLI package intentionally has no public Rust library target, so its primary usage documentation remains the repository README and CLI help.

## Partial publication recovery

### Library published, CLI not published

If the library publishes but the CLI fails, do not create `v0.2.0` or a GitHub Release and do not attempt to overwrite `pgdumpx 0.2.0`.

- If the CLI package itself is the only problem and `pgdumpx-cli 0.2.0` was never accepted, correct the CLI or release metadata, re-run every gate, and publish the same still-unpublished CLI version against `pgdumpx 0.2.0`.
- If the correction requires changing the published library API or package, prepare `0.2.1` for both packages and repeat the library-first sequence.
- Yank `pgdumpx 0.2.0` only for a severe correctness or security defect that justifies excluding it from new dependency resolution. Yanking does not delete the published archive.

### Packages published, tag or Release creation failed

Published crate versions are immutable. Verify both registry entries, restore the exact release commit locally, and retry only the failed Git tag or GitHub Release operation. Do not publish replacement crate versions merely because the source-release operation failed.
