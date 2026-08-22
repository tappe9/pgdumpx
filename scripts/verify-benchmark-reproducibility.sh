#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROW_COUNT="${PGDUMPX_BENCH_REPRO_ROWS:-101}"
readonly TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
readonly CLI="${TARGET_DIR}/debug/pgdumpx"
readonly WORK_DIR="$(mktemp -d)"
readonly FIRST="${WORK_DIR}/first"
readonly SECOND="${WORK_DIR}/second"

fail() {
    echo "benchmark reproducibility check failed: $*" >&2
    exit 1
}

cleanup() {
    rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

for command in cargo cmp sha256sum; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done

case "${ROW_COUNT}" in
    ''|*[!0-9]*) fail "PGDUMPX_BENCH_REPRO_ROWS must be an integer >= 3" ;;
esac
(( ROW_COUNT >= 3 )) || fail "PGDUMPX_BENCH_REPRO_ROWS must be >= 3"

echo "Generating first deterministic dataset"
PGDUMPX_BENCH_ROWS="${ROW_COUNT}" \
PGDUMPX_BENCH_DATA_DIR="${FIRST}" \
    bash "${ROOT}/scripts/generate-benchmark-dataset.sh"

echo "Generating second deterministic dataset"
PGDUMPX_BENCH_ROWS="${ROW_COUNT}" \
PGDUMPX_BENCH_DATA_DIR="${SECOND}" \
    bash "${ROOT}/scripts/generate-benchmark-dataset.sh"

echo "Building pgdumpx CLI for production-path extraction checks"
cargo build \
    --manifest-path "${ROOT}/Cargo.toml" \
    --package pgdumpx-cli \
    --all-features
[[ -x "${CLI}" ]] || fail "pgdumpx CLI not found at ${CLI}"

reference_sha=""
for compression in none gzip lz4 zstd; do
    first_output="${WORK_DIR}/first-${compression}.copy"
    second_output="${WORK_DIR}/second-${compression}.copy"

    "${CLI}" extract "${FIRST}/pgdumpx-bench-${compression}.dump" bench.rows > "${first_output}"
    "${CLI}" extract "${SECOND}/pgdumpx-bench-${compression}.dump" bench.rows > "${second_output}"

    cmp -s "${first_output}" "${second_output}" \
        || fail "${compression} extraction differs across two regenerations"

    current_sha="$(sha256sum "${first_output}" | awk '{print $1}')"
    if [[ -z "${reference_sha}" ]]; then
        reference_sha="${current_sha}"
    elif [[ "${current_sha}" != "${reference_sha}" ]]; then
        fail "${compression} extraction differs from the none-compression logical dataset"
    fi
    echo "${compression}: ${current_sha}"
done

first_rows="$(awk -F '\t' '$1 == "row_count" { print $2 }' "${FIRST}/manifest.tsv")"
second_rows="$(awk -F '\t' '$1 == "row_count" { print $2 }' "${SECOND}/manifest.tsv")"
[[ "${first_rows}" == "${ROW_COUNT}" && "${second_rows}" == "${ROW_COUNT}" ]] \
    || fail "manifest row counts do not match requested dataset size"

echo "Reproducibility verified for ${ROW_COUNT} rows; logical COPY SHA-256=${reference_sha}"
