# Fuzzing pgdumpx

pgdumpx uses `cargo-fuzz`/libFuzzer to exercise untrusted archive, framing, COPY, and resource-accounting boundaries through the same production APIs used by callers. The fuzz package is intentionally outside the stable workspace so normal builds and the Rust 1.85.0 MSRV do not acquire a nightly or libFuzzer dependency.

## Prerequisites

Fuzzing requires a nightly Rust toolchain and `cargo-fuzz`. CI pins the command-line tool version used by the smoke job:

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

## Build and smoke verification

From the repository root:

```bash
cargo +nightly fuzz build --fuzz-dir fuzz

for target in archive_open toc_metadata entry_framing copy_rows copy_metadata limit_accounting; do
  cargo +nightly fuzz run --fuzz-dir fuzz "$target" -- \
    -runs=64 -max_len=65536 -timeout=2
done
```

The pull-request CI job runs this bounded build/smoke sequence. It is a regression gate, not a substitute for longer coverage-guided campaigns.

## Longer local campaigns

Run one target without `-runs` to continue fuzzing until interrupted or until libFuzzer finds a failure. Keep the 64 KiB input cap unless a deliberate harness change justifies a larger bound:

```bash
cargo +nightly fuzz run --fuzz-dir fuzz archive_open -- \
  -max_len=65536 -timeout=2
```

Committed target-specific seeds live under `fuzz/corpus/<target>/` and include both valid minimal inputs and malformed/boundary inputs.

## Crash or hang workflow

When fuzzing finds a panic, hang, or boundary-accounting defect:

1. minimize the input, for example with `cargo +nightly fuzz tmin --fuzz-dir fuzz <target> <artifact>`;
2. preserve the minimized input in `fuzz/corpus/<target>/` when it is safe to commit;
3. add a deterministic regression test that reaches the same public production path and verify that the test fails before changing production code;
4. make the smallest production fix needed to return success or a typed error without panic/hang;
5. rerun the deterministic test, workspace quality gates, and the full fuzz build/smoke sequence.

Do not weaken a production validation rule only to make a fuzz target pass. Fuzz-discovered security-sensitive inputs remain permanent regression evidence.
