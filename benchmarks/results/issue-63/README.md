# Issue #63 buffer-tuning evidence

This directory records the reproducible evidence used to decide whether pgdumpx v0.2 should change its internal streaming buffer sizes.

## Decision

**No production buffer change.** Both relevant extraction-path defaults remain **8 KiB**:

- `crates/pgdumpx/src/raw_entry.rs::COPY_BUFFER_BYTES`
- `crates/pgdumpx/src/entry.rs::COMPRESSED_INPUT_BUFFER_BYTES` (gzip decoder input)

The bounded candidates were 4, 8, 16, and 32 KiB. A non-baseline candidate was eligible only if all predeclared conditions held:

1. geometric-mean throughput improvement of at least **5%** across its measured cases;
2. no measured case worse than **-3%** versus the 8 KiB baseline;
3. maximum median peak-RSS increase no greater than **2048 KiB**.

No candidate qualified. For `COPY_BUFFER_BYTES`, the 4/16/32 KiB candidates had aggregate throughput deltas of **-1.661% / -0.877% / -0.847%**. For the gzip input buffer, the corresponding aggregate deltas were **+0.096% / -0.009% / +0.160%**, far below the meaningful-improvement threshold.

## Measured production paths

- `single`: `Archive::table` followed by `Archive::copy_entry_to` for `bench.rows`.
- `multi`: Issue #60's sequential `ExtractionPlan::execute` path for `bench.rows` and `bench.rows_secondary`.

The `copy` experiment changes only the private `COPY_BUFFER_BYTES` constant and measures `none`, `gzip`, `lz4`, and `zstd`, each in single- and multi-table scenarios. The `gzip-input` experiment changes only the private gzip `COMPRESSED_INPUT_BUFFER_BYTES` constant and therefore measures gzip single- and multi-table scenarios only. Each candidate is rebuilt from the same production source path in a detached worktree; no benchmark-only extractor, decompressor, or public runtime tuning knob is used.

Before candidate measurements, the run executes the fixed library regression suite with `cargo test --package pgdumpx --all-features`, preserving the existing `ScanLimits`, `EntryReadLimits`, decompression-error, and partial-output tests.

## Dataset and repetitions

The recorded run used:

- measured source commit: `39ba66895438fea2a77a53e83af6505aa92a2f58`;
- GitHub Actions workflow run: `32713544781` (`Issue 63 buffer evidence`);
- evidence artifact digest: `sha256:20098c3e683628cb6653928ea396bba2f8d49ff8b8bdf685f36e00eb7487dfbe`;
- PostgreSQL `pg_dump` 18.4 from the pinned generator image;
- archive version 1.16.0;
- 100,000 deterministic rows in each of `bench.rows` and `bench.rows_secondary`;
- compression archives: none, gzip:6, lz4:1, zstd:3;
- 1 warm-up iteration and 5 throughput repetitions per case;
- 3 separate peak-RSS process repetitions per case using GNU `time` `%M` (KiB).

The two-table dataset was independently generated twice at a smaller size before measurement, and both tables' logical decompressed COPY bytes matched across regenerations and compression modes.

## Files

- `metadata.tsv`: environment, source commit, dataset provenance, commands, and repetition counts.
- `case-summary.tsv`: per-case median throughput and peak-RSS deltas versus 8 KiB.
- `candidate-summary.tsv`: aggregate candidate decision metrics.
- `decision.tsv`: selected size and thresholds for each buffer kind.
- `raw-evidence.zip`: exact `throughput.tsv`, `peak-rss.tsv`, and the summary/metadata files uploaded by the successful evidence workflow.

The raw repetitions are retained so the decision can be audited without treating a best run as representative. These measurements support only the buffer-size decision above; they are not a general product speedup claim.
