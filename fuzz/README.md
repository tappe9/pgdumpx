# Fuzzing pgdumpx

pgdumpx uses `cargo-fuzz`/libFuzzer to exercise untrusted archive, framing, COPY, and resource-accounting boundaries through the same production APIs used by callers. The fuzz package is intentionally outside the stable workspace so normal builds and the Rust 1.85.0 MSRV do not acquire a nightly or libFuzzer dependency.

## Prerequisites

Fuzzing requires a nightly Rust toolchain and `cargo-fuzz`. CI pins the command-line tool version used by both smoke and coverage-guided campaigns:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --version 0.13.2 --locked
```

## Targets

| Target | Production boundary exercised |
|---|---|
| `archive_open` | Arbitrary archive bytes through `Archive::open_with_limits` |
| `toc_metadata` | Valid 1.16 header plus arbitrary TOC bytes, including structural limits |
| `entry_framing` | Valid metadata plus arbitrary selected-entry block/chunk framing through `entry_reader` |
| `copy_rows` | Arbitrary COPY text bytes through `CopyRowReader`, including row/field and scan limits |
| `copy_metadata` | Arbitrary COPY statement metadata embedded in a valid table-data TOC entry |
| `limit_accounting` | Boundary-adjacent scan-byte/row and raw decompressed-byte accounting |

Harnesses cap individual fuzz inputs at 64 KiB and use finite parser limits. They assemble minimal archive wrappers only to reach deeper production paths; no fuzz-only parser semantics are implemented in `pgdumpx`.

## Pull-request smoke gate

From the repository root:

```bash
cargo +nightly fuzz build --fuzz-dir fuzz

for target in archive_open toc_metadata entry_framing copy_rows copy_metadata limit_accounting; do
  cargo +nightly fuzz run --fuzz-dir fuzz "$target" -- \
    -runs=64 -max_len=65536 -timeout=2
done
```

The pull-request `Fuzz target smoke` job keeps this exact bounded 64-run sequence. It is a fast regression/build gate and is not lengthened by the scheduled campaign.

## Scheduled coverage-guided campaigns

`.github/workflows/scheduled-fuzz.yml` runs every maintained target each Sunday at `03:29 UTC`. A manual `workflow_dispatch` run uses the same campaign budget. A path-filtered branch push that changes the workflow, runner, contract tests, or `fuzz/**` performs a 10-second-per-target live validation; scheduled and manual runs use 300 seconds per target.

| Target | Frequency | `-max_total_time` |
|---|---|---:|
| `archive_open` | Weekly and manual | 300 seconds |
| `toc_metadata` | Weekly and manual | 300 seconds |
| `entry_framing` | Weekly and manual | 300 seconds |
| `copy_rows` | Weekly and manual | 300 seconds |
| `copy_metadata` | Weekly and manual | 300 seconds |
| `limit_accounting` | Weekly and manual | 300 seconds |

Every campaign also sets:

```text
-max_len=65536
-timeout=10
job timeout-minutes=15
```

The six targets run as separate matrix jobs. Workflow-level concurrency cancels an older overlapping run in the same schedule/ref group, and every job has a hard timeout, so neither an individual target nor the overall matrix can run indefinitely.

### Manual dispatch

1. Open the repository's **Actions** tab.
2. Select **Scheduled fuzz campaigns**.
3. Choose **Run workflow**, select `main`, and start the run.
4. Confirm that all six `Fuzz campaign (<target>)` jobs complete successfully and inspect the per-target artifacts described below.

Manual dispatch intentionally uses the 300-second target budget. For a faster validation, update the runner/workflow on a branch and let the path-filtered push validation use 10 seconds per target.

## Failure preservation and job status

`scripts/run-fuzz-campaign.py` separates command execution from final status propagation:

1. `run` validates the target/path, creates `fuzz/artifacts/<target>` and `fuzz/campaign-results/<target>`, streams combined stdout/stderr to `campaign.log`, and records `status.txt` plus non-sensitive metadata;
2. the workflow uploads both directories with `if: always()`;
3. `propagate` reads the saved status and exits with the original fuzz command status.

A crash, sanitizer finding, input timeout/hang, or any other non-zero libFuzzer result is therefore not hidden to make artifact upload succeed. The upload runs first, then the job fails with the original status. A missing or malformed status file is itself an error rather than an implicit success.

The Actions artifact name is:

```text
scheduled-fuzz-<target>-<commit SHA>-<run ID>
```

Artifacts are retained for 7 days and contain only:

- `fuzz/artifacts/<target>/` for libFuzzer crash/timeout artifacts;
- `fuzz/campaign-results/<target>/campaign.log`;
- `fuzz/campaign-results/<target>/status.txt`;
- `fuzz/campaign-results/<target>/metadata.json`.

The evolving `fuzz/corpus/<target>/` directory is not uploaded or cached by this workflow.

## Local coverage-guided campaign

Use the same bounds locally when reproducing CI behavior:

```bash
TARGET=archive_open
mkdir -p "fuzz/artifacts/$TARGET"

cargo +nightly fuzz run --fuzz-dir fuzz "$TARGET" -- \
  -max_total_time=300 \
  -max_len=65536 \
  -timeout=10 \
  "-artifact_prefix=fuzz/artifacts/$TARGET/"
```

Committed target-specific seeds live under `fuzz/corpus/<target>/` and include valid minimal inputs plus malformed/boundary inputs.

## Crash, hang, and sanitizer triage

For a failed Actions campaign:

1. record the workflow run ID, target, commit SHA, saved exit status, and tool version;
2. download the target-specific artifact through an authorized GitHub session;
3. inspect `campaign.log` and reproduce the artifact locally with the same target and bounds;
4. minimize it before changing production code, for example:

   ```bash
   cargo +nightly fuzz tmin --fuzz-dir fuzz <target> <artifact> -- \
     -max_len=65536 -timeout=10
   ```

5. add a deterministic regression test through the same public production path and verify that it fails for the expected panic, hang, sanitizer finding, or boundary defect;
6. make the smallest production fix, then rerun the deterministic test, workspace quality gates, PR smoke sequence, and the affected bounded campaign.

Do not fix production code first and add the regression afterward. The minimized deterministic test is the RED evidence and permanent behavioral contract.

## Sensitive artifact handling

GitHub Actions artifacts are temporary diagnostic storage, not a confidential disclosure channel. The repository workflow may use only committed, reviewed, non-sensitive corpus seeds and generated mutations. Never seed this public-repository campaign with production dumps, customer data, credentials, personal data, proprietary inputs, or embargoed vulnerability material.

When a finding may be security-sensitive:

- do not paste the crashing bytes or exploit details into a public issue or job summary;
- follow [SECURITY.md](../SECURITY.md) and move triage to private vulnerability reporting;
- keep sensitive local/private inputs out of the scheduled workflow and its artifact paths;
- share a minimized artifact only with authorized reviewers and only after confirming that it contains no protected data.

## Corpus provenance and review

A discovered input may be committed to `fuzz/corpus/<target>/` only when it is minimized, deterministic, necessary for regression coverage, and safe for public distribution. The PR that adds or changes corpus data must record:

- target name;
- source workflow run ID or local campaign description;
- source commit SHA;
- minimization command;
- checksum of the committed input;
- the production invariant it protects;
- reviewer confirmation that no secrets, personal data, customer data, or proprietary archive content is present.

Corpus evolution is never copied automatically from CI. Review provenance and content exactly like source code. Do not weaken a production validation rule only to make a fuzz target pass.
