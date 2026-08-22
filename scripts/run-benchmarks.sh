#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly DATA_DIR="${PGDUMPX_BENCH_DATA_DIR:-${ROOT}/target/benchmark-data}"
readonly WARMUP="${PGDUMPX_BENCH_WARMUP:-2}"
readonly REPETITIONS="${PGDUMPX_BENCH_REPETITIONS:-10}"
readonly TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
readonly RUNNER="${TARGET_DIR}/release/examples/benchmark_runner"
readonly COMMIT="$(git -C "${ROOT}" rev-parse HEAD)"
readonly SHORT_COMMIT="${COMMIT:0:12}"
readonly DEFAULT_RESULTS_DIR="${ROOT}/target/benchmark-results/$(date -u +%Y%m%dT%H%M%SZ)-${SHORT_COMMIT}"
readonly RESULTS_DIR="${PGDUMPX_BENCH_RESULTS_DIR:-${DEFAULT_RESULTS_DIR}}"
readonly RESULTS="${RESULTS_DIR}/throughput.tsv"
readonly RSS_RESULTS="${RESULTS_DIR}/peak-rss.tsv"
readonly METADATA="${RESULTS_DIR}/metadata.tsv"

fail() {
    echo "benchmark run failed: $*" >&2
    exit 1
}

for value_name in WARMUP REPETITIONS; do
    value="${!value_name}"
    case "${value}" in
        ''|*[!0-9]*) fail "${value_name} must be a non-negative integer" ;;
    esac
done
(( REPETITIONS > 0 )) || fail "PGDUMPX_BENCH_REPETITIONS must be greater than zero"

for command in cargo git rustc uname; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done
[[ -f "${DATA_DIR}/manifest.tsv" ]] || fail "missing ${DATA_DIR}/manifest.tsv; run scripts/generate-benchmark-dataset.sh first"
for compression in none gzip lz4 zstd; do
    [[ -f "${DATA_DIR}/pgdumpx-bench-${compression}.dump" ]] \
        || fail "missing ${DATA_DIR}/pgdumpx-bench-${compression}.dump"
done

mkdir -p "${RESULTS_DIR}"
rm -f "${RESULTS}" "${RSS_RESULTS}" "${METADATA}"

echo "Building release benchmark runner"
cargo build \
    --manifest-path "${ROOT}/Cargo.toml" \
    --release \
    --package pgdumpx \
    --example benchmark_runner \
    --all-features
[[ -x "${RUNNER}" ]] || fail "benchmark runner not found at ${RUNNER}"

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

{
    printf 'key\tvalue\n'
    printf 'commit\t%s\n' "${COMMIT}"
    printf 'working_tree_dirty\t%s\n' "${dirty}"
    printf 'os\t%s\n' "${os}"
    printf 'cpu\t%s\n' "${cpu_model}"
    printf 'rustc\t%s\n' "${rustc_version}"
    printf 'cargo\t%s\n' "${cargo_version}"
    printf 'runner\tcrates/pgdumpx/examples/benchmark_runner.rs\n'
    printf 'measurement_clock\tstd::time::Instant\n'
    printf 'peak_rss_tool\tGNU time %%M (KiB)\n'
    printf 'warmup_iterations\t%s\n' "${WARMUP}"
    printf 'measured_repetitions\t%s\n' "${REPETITIONS}"
    printf 'dataset_manifest\t%s\n' "${DATA_DIR}/manifest.tsv"
    tail -n +2 "${DATA_DIR}/manifest.tsv" | while IFS=$'\t' read -r key value; do
        printf 'dataset.%s\t%s\n' "${key}" "${value}"
    done
} > "${METADATA}"

append_run() {
    local output
    output="$(mktemp)"
    "${RUNNER}" "$@" --warmup "${WARMUP}" --repetitions "${REPETITIONS}" > "${output}"
    if [[ ! -e "${RESULTS}" ]]; then
        cat "${output}" > "${RESULTS}"
    else
        tail -n +2 "${output}" >> "${RESULTS}"
    fi
    rm -f "${output}"
}

for compression in none gzip lz4 zstd; do
    archive="${DATA_DIR}/pgdumpx-bench-${compression}.dump"
    echo "Benchmarking ${compression}"

    append_run open "${archive}"
    append_run extract "${archive}" --limit-mode none
    append_run extract "${archive}" --limit-mode raw-bytes
    append_run rows "${archive}"

    for position in early middle late absent; do
        append_run find "${archive}" --match "${position}" --limit-mode none
    done

    for limit_mode in scan-rows scan-bytes scan-both; do
        append_run find "${archive}" --match absent --limit-mode "${limit_mode}"
    done

    bash "${ROOT}/scripts/measure-peak-rss.sh" \
        "${RSS_RESULTS}" "${compression}/open" -- \
        "${RUNNER}" open "${archive}" --warmup 0 --repetitions 1
    bash "${ROOT}/scripts/measure-peak-rss.sh" \
        "${RSS_RESULTS}" "${compression}/extract" -- \
        "${RUNNER}" extract "${archive}" --limit-mode none --warmup 0 --repetitions 1
done

echo "Benchmark metadata: ${METADATA}"
echo "Throughput results: ${RESULTS}"
echo "Peak RSS results: ${RSS_RESULTS}"
