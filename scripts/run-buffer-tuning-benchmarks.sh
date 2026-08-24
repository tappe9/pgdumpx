#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly DATA_DIR="${PGDUMPX_BENCH_DATA_DIR:-${ROOT}/target/benchmark-data}"
readonly WARMUP="${PGDUMPX_BUFFER_BENCH_WARMUP:-1}"
readonly REPETITIONS="${PGDUMPX_BUFFER_BENCH_REPETITIONS:-5}"
readonly RSS_REPETITIONS="${PGDUMPX_BUFFER_BENCH_RSS_REPETITIONS:-3}"
readonly CANDIDATES="${PGDUMPX_BUFFER_BENCH_CANDIDATES:-4096 8192 16384 32768}"
readonly BASELINE_BUFFER_BYTES=8192
readonly COMMIT="$(git -C "${ROOT}" rev-parse HEAD)"
readonly SHORT_COMMIT="${COMMIT:0:12}"
readonly DEFAULT_RESULTS_DIR="${ROOT}/target/benchmark-results/buffer-tuning/$(date -u +%Y%m%dT%H%M%SZ)-${SHORT_COMMIT}"
readonly RESULTS_DIR="${PGDUMPX_BUFFER_BENCH_RESULTS_DIR:-${DEFAULT_RESULTS_DIR}}"
readonly THROUGHPUT="${RESULTS_DIR}/throughput.tsv"
readonly RSS_RESULTS="${RESULTS_DIR}/peak-rss.tsv"
readonly METADATA="${RESULTS_DIR}/metadata.tsv"
readonly WORKTREE_ROOT="${ROOT}/target/buffer-tuning-worktrees-${SHORT_COMMIT}-$$"

fail() {
    echo "buffer tuning benchmark failed: $*" >&2
    exit 1
}

for command in cargo git python3 rustc uname; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done

for value_name in WARMUP REPETITIONS RSS_REPETITIONS; do
    value="${!value_name}"
    case "${value}" in
        ''|*[!0-9]*) fail "${value_name} must be a non-negative integer" ;;
    esac
done
(( REPETITIONS > 0 )) || fail "PGDUMPX_BUFFER_BENCH_REPETITIONS must be greater than zero"
(( RSS_REPETITIONS > 0 )) || fail "PGDUMPX_BUFFER_BENCH_RSS_REPETITIONS must be greater than zero"

read -r -a candidate_values <<< "${CANDIDATES}"
(( ${#candidate_values[@]} > 0 )) || fail "at least one buffer candidate is required"
baseline_found=false
for candidate in "${candidate_values[@]}"; do
    case "${candidate}" in
        ''|*[!0-9]*) fail "buffer candidates must be positive byte counts: ${candidate}" ;;
    esac
    (( candidate > 0 )) || fail "buffer candidates must be greater than zero"
    if (( candidate == BASELINE_BUFFER_BYTES )); then
        baseline_found=true
    fi
done
[[ "${baseline_found}" == "true" ]] || fail "candidate set must contain the 8192-byte production baseline"

[[ -f "${DATA_DIR}/manifest.tsv" ]] || fail "missing ${DATA_DIR}/manifest.tsv; run scripts/generate-benchmark-dataset.sh first"
for compression in none gzip lz4 zstd; do
    [[ -f "${DATA_DIR}/pgdumpx-bench-${compression}.dump" ]] \
        || fail "missing ${DATA_DIR}/pgdumpx-bench-${compression}.dump"
done

readonly DATASET_VERSION="$(awk -F '\t' '$1 == "dataset_version" { print $2 }' "${DATA_DIR}/manifest.tsv")"
readonly SECONDARY_TABLE="$(awk -F '\t' '$1 == "secondary_table" { print $2 }' "${DATA_DIR}/manifest.tsv")"
[[ "${DATASET_VERSION}" == "2" ]] || fail "buffer tuning requires benchmark dataset_version=2"
[[ "${SECONDARY_TABLE}" == "rows_secondary" ]] || fail "buffer tuning requires bench.rows_secondary"

mkdir -p "${RESULTS_DIR}" "${WORKTREE_ROOT}"
rm -f "${THROUGHPUT}" "${RSS_RESULTS}" "${METADATA}"

worktrees=()
cleanup() {
    for worktree in "${worktrees[@]:-}"; do
        git -C "${ROOT}" worktree remove --force "${worktree}" >/dev/null 2>&1 || true
    done
    rm -rf "${WORKTREE_ROOT}"
    git -C "${ROOT}" worktree prune >/dev/null 2>&1 || true
}
trap cleanup EXIT

cpu_model="unknown"
if command -v lscpu >/dev/null 2>&1; then
    cpu_model="$(lscpu | awk -F: '/Model name/{sub(/^[[:space:]]+/, "", $2); print $2; exit}')"
elif command -v sysctl >/dev/null 2>&1; then
    cpu_model="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || sysctl -n hw.model 2>/dev/null || echo unknown)"
fi
cpu_model="$(printf '%s' "${cpu_model}" | tr '\t\r\n' '   ')"
os="$(uname -a | tr '\t\r\n' '   ')"
rustc_version="$(rustc --version | tr '\t\r\n' '   ')"
cargo_version="$(cargo --version | tr '\t\r\n' '   ')"
dirty="false"
[[ -z "$(git -C "${ROOT}" status --porcelain)" ]] || dirty="true"
production_buffer_expression="$(
    sed -n 's/^const COPY_BUFFER_BYTES: usize = \(.*\);$/\1/p' "${ROOT}/crates/pgdumpx/src/raw_entry.rs"
)"
[[ -n "${production_buffer_expression}" ]] || fail "COPY_BUFFER_BYTES production constant was not found"

{
    printf 'key\tvalue\n'
    printf 'commit\t%s\n' "${COMMIT}"
    printf 'working_tree_dirty\t%s\n' "${dirty}"
    printf 'os\t%s\n' "${os}"
    printf 'cpu\t%s\n' "${cpu_model}"
    printf 'rustc\t%s\n' "${rustc_version}"
    printf 'cargo\t%s\n' "${cargo_version}"
    printf 'runner\tcrates/pgdumpx/examples/buffer_tuning_runner.rs\n'
    printf 'production_buffer_path\tcrates/pgdumpx/src/raw_entry.rs:COPY_BUFFER_BYTES\n'
    printf 'production_buffer_expression\t%s\n' "${production_buffer_expression}"
    printf 'baseline_buffer_bytes\t%s\n' "${BASELINE_BUFFER_BYTES}"
    printf 'candidate_buffer_bytes\t%s\n' "${CANDIDATES}"
    printf 'candidate_method\tdetached worktree; replace private COPY_BUFFER_BYTES; rebuild production code\n'
    printf 'measurement_clock\tstd::time::Instant\n'
    printf 'peak_rss_tool\tGNU time %%M (KiB)\n'
    printf 'warmup_iterations\t%s\n' "${WARMUP}"
    printf 'measured_repetitions\t%s\n' "${REPETITIONS}"
    printf 'peak_rss_repetitions\t%s\n' "${RSS_REPETITIONS}"
    printf 'dataset_manifest\t%s\n' "${DATA_DIR}/manifest.tsv"
    tail -n +2 "${DATA_DIR}/manifest.tsv" | while IFS=$'\t' read -r key value; do
        printf 'dataset.%s\t%s\n' "${key}" "${value}"
    done
} > "${METADATA}"

printf 'buffer_bytes\tscenario\tcompression\tarchive_version\trepetition\telapsed_ns\tunits\tunit\tunits_per_second\toutcome\n' > "${THROUGHPUT}"

for candidate in "${candidate_values[@]}"; do
    worktree="${WORKTREE_ROOT}/${candidate}"
    worktrees+=("${worktree}")
    git -C "${ROOT}" worktree add --detach "${worktree}" "${COMMIT}" >/dev/null

    python3 - "${worktree}/crates/pgdumpx/src/raw_entry.rs" "${candidate}" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
candidate = int(sys.argv[2])
text = path.read_text()
pattern = re.compile(r"^const COPY_BUFFER_BYTES: usize = [^;]+;$", re.MULTILINE)
updated, count = pattern.subn(f"const COPY_BUFFER_BYTES: usize = {candidate};", text)
if count != 1:
    raise SystemExit(f"expected exactly one COPY_BUFFER_BYTES constant, found {count}")
path.write_text(updated)
PY

    target_dir="${ROOT}/target/buffer-tuning-build/${candidate}"
    echo "Building ${candidate}-byte production candidate"
    CARGO_TARGET_DIR="${target_dir}" cargo build \
        --manifest-path "${worktree}/Cargo.toml" \
        --release \
        --package pgdumpx \
        --example buffer_tuning_runner \
        --all-features
    runner="${target_dir}/release/examples/buffer_tuning_runner"
    [[ -x "${runner}" ]] || fail "candidate runner not found at ${runner}"

    for compression in none gzip lz4 zstd; do
        archive="${DATA_DIR}/pgdumpx-bench-${compression}.dump"
        for scenario in single multi; do
            output="$(mktemp)"
            "${runner}" "${scenario}" "${archive}" \
                --warmup "${WARMUP}" \
                --repetitions "${REPETITIONS}" > "${output}"
            tail -n +2 "${output}" | awk -v candidate="${candidate}" \
                'BEGIN { OFS="\t" } { print candidate, $0 }' >> "${THROUGHPUT}"
            rm -f "${output}"

            for rss_repetition in $(seq 1 "${RSS_REPETITIONS}"); do
                bash "${ROOT}/scripts/measure-peak-rss.sh" \
                    "${RSS_RESULTS}" \
                    "buffer=${candidate}/${compression}/${scenario}/repetition=${rss_repetition}" -- \
                    "${runner}" "${scenario}" "${archive}" --warmup 0 --repetitions 1
            done
        done
    done

done

echo "Buffer tuning metadata: ${METADATA}"
echo "Buffer tuning throughput: ${THROUGHPUT}"
echo "Buffer tuning peak RSS: ${RSS_RESULTS}"
