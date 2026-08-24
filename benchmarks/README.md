# pgdumpx benchmark harness

Status: **reproducible v0.1 performance-evidence harness implemented; v0.2 adds bounded sequential multi-table and internal buffer-tuning scenarios. Ordinary CI compiles/smokes benchmark targets without publishing performance results.**

This directory defines the reproducible performance-evidence harness for pgdumpx. It measures public production archive, decompression, COPY-row, limit, raw-extraction, and sequential multi-table paths. It does not contain a benchmark-only parser, extractor, or decompressor.

No benchmark result in this directory is a product performance claim by itself. Any published quantitative claim must retain the environment and dataset metadata emitted by this harness.

## What is measured

The original v0.1 runner covers:

| Operation | Production path | Primary units |
| --- | --- | --- |
| archive open | `File::open` then timed `Archive::open` | elapsed time, TOC entries/s, peak RSS |
| selected-entry extraction | `Archive::table` + `Archive::copy_entry_to` | decompressed bytes/s, peak RSS |
| COPY row parsing | `Archive::table_rows` + `TableRowReader::next_row` | rows/s |
| first match | `Archive::table_rows` + column lookup + `find_first` | evaluated rows/s |
| bounded first match | same path + `find_first_with_limits` | evaluated rows/s |

The v0.2 tuning runner adds:

| Scenario | Production path | Primary units |
| --- | --- | --- |
| single-table extraction | `Archive::table` + `Archive::copy_entry_to` | decompressed bytes/s, peak RSS |
| sequential multi-table extraction | `ExtractionPlan::execute` for two selectors | decompressed bytes/s, peak RSS |

`find` cases use deterministic values at the first row (`early`), middle row (`middle`), final row (`late`), and a value that does not exist (`absent`). The predicate counts rows actually evaluated, so early termination is visible in the output without adding a row-index shortcut.

Compression results are emitted separately for `none`, `gzip`, `lz4`, and `zstd` archives generated from the same logical rows. PostgreSQL documents these methods for `pg_dump --compress=method[:detail]`; the dataset generator pins explicit compression levels where a method uses one.

## Deterministic generated dataset

Large benchmark archives are not committed. Generate them with:

```bash
bash scripts/generate-benchmark-dataset.sh
```

The default logical dataset contains 250,000 rows **per table** in `bench.rows` and `bench.rows_secondary`. Override the size without changing either table's deterministic data formula:

```bash
PGDUMPX_BENCH_ROWS=1000000 \
  bash scripts/generate-benchmark-dataset.sh
```

The generator uses:

- the pinned `postgres:18.4-bookworm` image digest already used by the compatibility fixture tooling;
- `linux/amd64` as the generator platform;
- `benchmarks/dataset.sql` as the deterministic row definition;
- archive format 1.16.0;
- `none`, `gzip:6`, `lz4:1`, and `zstd:3` custom archives;
- `pg_restore --list` validation for the expected `TABLE DATA bench.rows` and `TABLE DATA bench.rows_secondary` entries.

Generated files live under `target/benchmark-data/` by default and therefore do not add large archives to the repository. `manifest.tsv` records the rows per table, generator version, pinned image, source hash, archive version, compression settings, generation command summary, and each generated archive checksum.

The logical dataset is deterministic; custom archive bytes are not required to be byte-for-byte identical between regenerations because PostgreSQL records archive metadata such as creation time. Reproducibility is therefore checked at the production-path decompressed table-data boundary rather than by requiring identical archive SHA-256 values.

To generate the dataset twice and verify that both tables in all four compression variants yield identical logical table-data bytes through `pgdumpx extract`:

```bash
PGDUMPX_BENCH_REPRO_ROWS=101 \
  bash scripts/verify-benchmark-reproducibility.sh
```

This check deliberately uses a small row count because it validates determinism, not throughput.

## Running the v0.1 benchmark matrix

Generate data first, then run:

```bash
bash scripts/run-benchmarks.sh
```

Defaults:

```text
warm-up iterations: 2
measured repetitions: 10
```

Override them explicitly when needed:

```bash
PGDUMPX_BENCH_WARMUP=1 \
PGDUMPX_BENCH_REPETITIONS=5 \
PGDUMPX_BENCH_RESULTS_DIR=target/benchmark-results/local \
  bash scripts/run-benchmarks.sh
```

The script builds `crates/pgdumpx/examples/benchmark_runner.rs` in release mode with all compression features, then executes the matrix against every generated archive.

Output files are:

```text
metadata.tsv
throughput.tsv
peak-rss.tsv
```

`metadata.tsv` records at least:

- exact Git commit and whether the working tree was dirty;
- OS/kernel information;
- CPU model when discoverable;
- `rustc` and Cargo versions;
- dataset generator metadata and archive checksums;
- warm-up and repetition counts;
- clock and peak-RSS measurement tools.

`throughput.tsv` records one row per measured repetition with:

- operation;
- archive compression and format version;
- match position where applicable;
- limit-accounting mode;
- elapsed nanoseconds;
- amount of work completed;
- work unit and derived units/second;
- match/success outcome.

The derived rate is intentionally simple: completed work divided by the operation's measured wall-clock interval. Keep raw repetitions; do not publish only a best run.

## Timed-region semantics

Each measured repetition opens a fresh file/archive instance so state is not reused across repetitions.

For `open`, `File::open` happens before the timer and `Archive::open` is the timed region. Peak RSS is process-level and therefore includes the complete benchmark-runner process.

For `extract`, `rows`, and `find`, `Archive::open` happens before the timer. The timed region begins before table/table-data selection and includes the corresponding public production operation. This separates metadata-open cost from selected-entry work while retaining normal table lookup, validated entry seek, decompression, COPY parsing, and limit behavior.

For the v0.2 tuning runner, `Archive::open` also happens before the timer. The `single` timed region starts before `Archive::table` and includes `copy_entry_to`. The `multi` timed region wraps Issue #60's `ExtractionPlan::execute`; construction of the fixed two-selector plan happens before timing so the measurement focuses on the sequential production execution path.

Warm-up iterations execute the same operation before recorded repetitions. They are intended to reduce one-time process/cache effects; the harness is not a cold-storage benchmark. Any report that needs cold-cache behavior must state and implement that separately.

## Peak RSS

Peak resident memory is measured by `scripts/measure-peak-rss.sh` using GNU `time` `%M`, whose documented unit is KiB. The default v0.1 matrix records peak RSS for:

- archive open, for every compression variant;
- selected-entry extraction, for every compression variant.

The v0.2 buffer-tuning matrix records separate peak-RSS process repetitions for every tested candidate/scenario/compression combination.

On Linux, install the `time` package if `/usr/bin/time` is unavailable. On macOS, install GNU time (for example through Homebrew, which normally exposes it as `gtime`). The wrapper rejects a non-GNU implementation instead of guessing at platform-specific units.

Peak RSS is measured in a separate process invocation from the throughput samples. Do not combine numbers from different machines or build profiles as though they were one run.

## Limit-accounting measurements

The harness does not weaken production limits to manufacture an accounting baseline.

### Structural limits

`Archive::open` always uses the production finite `Limits::default()` checks. There is intentionally no "structural limits disabled" benchmark because the production API has no such mode. Consequently, this harness measures real structural-limit cost as part of archive open but does **not** claim an isolated structural-accounting overhead number.

### Raw-output byte accounting

`extract --limit-mode none` uses `EntryReadLimits::unlimited()`.

`extract --limit-mode raw-bytes` uses the same `Archive::copy_entry_to` path with `max_decompressed_bytes = u64::MAX`. The limit cannot be reached by the generated dataset, so the difference is attributable to enabling the production raw-output accounting branch rather than to truncation or error handling.

### Row-scan accounting

The full-scan `absent` case is repeated with:

- `scan-rows`: `max_rows = u64::MAX`;
- `scan-bytes`: `max_decompressed_bytes = u64::MAX`;
- `scan-both`: both budgets enabled.

These use `find_first_with_limits` and therefore measure the production parser-consumed accounting path. The unlimited comparison uses `find_first` with the same predicate and absent target.

The early/middle/late/absent position matrix itself uses the unlimited path so match-position cost is not conflated with budget-accounting mode.

## v0.2 buffer-size tuning

Issue #63 adds `crates/pgdumpx/examples/buffer_tuning_runner.rs`, `scripts/run-buffer-tuning-benchmarks.sh`, and `scripts/summarize-buffer-tuning.py` for bounded, evidence-driven internal tuning.

Generate the two-table dataset, run the candidate matrix, then summarize it:

```bash
bash scripts/generate-benchmark-dataset.sh

PGDUMPX_BUFFER_BENCH_WARMUP=1 \
PGDUMPX_BUFFER_BENCH_REPETITIONS=5 \
PGDUMPX_BUFFER_BENCH_RSS_REPETITIONS=3 \
PGDUMPX_BUFFER_BENCH_RESULTS_DIR=target/benchmark-results/buffer-tuning \
  bash scripts/run-buffer-tuning-benchmarks.sh

python3 scripts/summarize-buffer-tuning.py \
  target/benchmark-results/buffer-tuning
```

The default candidate set is the small bounded set `4096 8192 16384 32768`, with the current 8192-byte production value as the baseline. The script evaluates two private production constants **independently**:

- `COPY_BUFFER_BYTES`: none/gzip/LZ4/Zstandard, each for single and sequential multi-table extraction;
- gzip `COMPRESSED_INPUT_BUFFER_BYTES`: gzip only, each for single and sequential multi-table extraction.

Each candidate is tested by changing one private constant in a detached worktree and rebuilding the same production path. This does not add a public runtime buffer knob, a benchmark-only extractor/decompressor, a parallel path, or a different compression backend.

Before measurement, the script runs `cargo test --package pgdumpx --all-features` so the selected configuration remains anchored to the existing correctness, `ScanLimits`, `EntryReadLimits`, decompression-error, and partial-output regression suite.

The summarizer uses medians per measured case and a predeclared selection rule: a non-baseline candidate must improve geometric-mean throughput by at least 5%, have no case worse than -3%, and increase median peak RSS by no more than 2048 KiB. If no candidate meets all thresholds, 8 KiB is retained.

The Issue #63 evidence is checked in under `benchmarks/results/issue-63/`. That recorded run retained **8 KiB for both production constants**; see its README and TSV summaries for the exact environment, repetitions, and decision. This is a tuning decision, not a public speedup claim.

## Compile and smoke checks

Both runners are normal crate examples, so workspace `--all-targets --all-features` lint/MSRV checks compile them. A focused local smoke sequence for the original runner is:

```bash
PGDUMPX_BENCH_ROWS=101 \
  bash scripts/generate-benchmark-dataset.sh

PGDUMPX_BENCH_WARMUP=0 \
PGDUMPX_BENCH_REPETITIONS=1 \
PGDUMPX_BENCH_RESULTS_DIR=target/benchmark-results/smoke \
  bash scripts/run-benchmarks.sh
```

Ordinary CI builds and runs only each benchmark runner's `--help` path. Full benchmark execution intentionally remains outside ordinary pull-request CI so CI noise is not presented as performance evidence.

## Comparator policy

`pg_restore` and adjacent libraries are not included in the default result table. A numerical ratio is acceptable only when both sides perform meaningfully equivalent work and material semantic differences are stated.

For example, `pg_restore --data-only --table=bench.rows` emits restore SQL/COPY framing, while `pgdumpx copy_entry_to` exposes only the selected decompressed table-data body. Those are useful adjacent operations but not byte-for-byte equivalent products. A report may measure both for context, but must state that extra formatting/output work instead of presenting the ratio as an apples-to-apples extraction speedup.

Likewise, `pg_restore --list` performs archive metadata parsing plus list rendering, while `Archive::open` parses metadata and builds pgdumpx indexes without rendering a list. It is not a strict archive-open comparator.

Do not add a public comparative claim merely because a command can be timed.

## Upstream references

- PostgreSQL 18 `pg_dump`: <https://www.postgresql.org/docs/18/app-pgdump.html>
- GNU time memory resources: <https://www.gnu.org/software/time/manual/html_node/Memory-Resources.html>

The repository's normative pgdumpx requirements remain in `docs/REQUIREMENTS.md` §10 and `ARCHITECTURE.md` §22.
