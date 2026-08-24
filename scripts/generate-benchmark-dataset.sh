#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly IMAGE_TAG="postgres:18.4-bookworm"
readonly IMAGE_DIGEST="postgres@sha256:882236b897e39051d2368c5ccc6cda944904723506b2dfc97f2a8f5bc9afa382"
readonly PLATFORM="linux/amd64"
readonly DATABASE="pgdumpx_benchmark"
readonly SOURCE_SQL="${ROOT}/benchmarks/dataset.sql"
readonly ROW_COUNT="${PGDUMPX_BENCH_ROWS:-250000}"
readonly DATA_DIR="${PGDUMPX_BENCH_DATA_DIR:-${ROOT}/target/benchmark-data}"
readonly CONTAINER="pgdumpx-benchmark-dataset-$$"
readonly MANIFEST="${DATA_DIR}/manifest.tsv"

fail() {
    echo "benchmark dataset generation failed: $*" >&2
    exit 1
}

case "${ROW_COUNT}" in
    ''|*[!0-9]*) fail "PGDUMPX_BENCH_ROWS must be an integer >= 3" ;;
esac
(( ROW_COUNT >= 3 )) || fail "PGDUMPX_BENCH_ROWS must be >= 3"

for command in docker python3 sha256sum; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done
[[ -f "${SOURCE_SQL}" ]] || fail "dataset SQL not found: ${SOURCE_SQL}"

cleanup() {
    docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "${DATA_DIR}"
mkdir -p "${DATA_DIR}"
docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true

echo "Pulling ${IMAGE_DIGEST} for ${PLATFORM}"
docker pull --platform "${PLATFORM}" "${IMAGE_DIGEST}" >/dev/null

echo "Starting PostgreSQL benchmark generator"
docker run \
    --detach \
    --rm \
    --platform "${PLATFORM}" \
    --name "${CONTAINER}" \
    --env POSTGRES_PASSWORD=benchmark-only \
    --env POSTGRES_DB="${DATABASE}" \
    --mount "type=bind,src=${SOURCE_SQL},dst=/benchmark/dataset.sql,readonly" \
    "${IMAGE_DIGEST}" >/dev/null

ready=false
for _ in $(seq 1 90); do
    if docker exec "${CONTAINER}" pg_isready --username=postgres --dbname="${DATABASE}" >/dev/null 2>&1; then
        ready=true
        break
    fi
    sleep 1
done
[[ "${ready}" == "true" ]] || fail "PostgreSQL did not become ready"

GENERATOR_VERSION="$(docker exec "${CONTAINER}" pg_dump --version | tr -d '\r')"
[[ "${GENERATOR_VERSION}" == "pg_dump (PostgreSQL) 18.4"* ]] \
    || fail "unexpected pg_dump version: ${GENERATOR_VERSION}"
readonly GENERATOR_VERSION

echo "Loading deterministic benchmark dataset (${ROW_COUNT} rows per table)"
docker exec "${CONTAINER}" \
    psql \
    --username=postgres \
    --dbname="${DATABASE}" \
    --no-psqlrc \
    --set="row_count=${ROW_COUNT}" \
    --file=/benchmark/dataset.sql >/dev/null

for table in rows rows_secondary; do
    actual_count="$(
        docker exec "${CONTAINER}" \
            psql --username=postgres --dbname="${DATABASE}" --no-psqlrc --tuples-only --no-align \
            --command="SELECT count(*) FROM bench.${table};" \
            | tr -d '[:space:]'
    )"
    [[ "${actual_count}" == "${ROW_COUNT}" ]] \
        || fail "expected ${ROW_COUNT} rows in bench.${table}, got ${actual_count}"
done

MIDDLE_ROW="$(( (ROW_COUNT + 1) / 2 ))"
for expected in "1:early" "${MIDDLE_ROW}:middle" "${ROW_COUNT}:late"; do
    row_no="${expected%%:*}"
    value="${expected#*:}"
    actual="$(
        docker exec "${CONTAINER}" \
            psql --username=postgres --dbname="${DATABASE}" --no-psqlrc --tuples-only --no-align \
            --command="SELECT match_key FROM bench.rows WHERE row_no = ${row_no};" \
            | tr -d '\r\n'
    )"
    [[ "${actual}" == "${value}" ]] || fail "row ${row_no} expected ${value}, got ${actual}"
done

printf 'key\tvalue\n' > "${MANIFEST}"
printf 'dataset_version\t2\n' >> "${MANIFEST}"
printf 'row_count\t%s\n' "${ROW_COUNT}" >> "${MANIFEST}"
printf 'rows_per_table\t%s\n' "${ROW_COUNT}" >> "${MANIFEST}"
printf 'schema\tbench\n' >> "${MANIFEST}"
printf 'table\trows\n' >> "${MANIFEST}"
printf 'secondary_table\trows_secondary\n' >> "${MANIFEST}"
printf 'match_column\tmatch_key\n' >> "${MANIFEST}"
printf 'early_row\t1\n' >> "${MANIFEST}"
printf 'middle_row\t%s\n' "${MIDDLE_ROW}" >> "${MANIFEST}"
printf 'late_row\t%s\n' "${ROW_COUNT}" >> "${MANIFEST}"
printf 'generator\t%s\n' "${GENERATOR_VERSION}" >> "${MANIFEST}"
printf 'image_tag\t%s\n' "${IMAGE_TAG}" >> "${MANIFEST}"
printf 'image_digest\t%s\n' "${IMAGE_DIGEST}" >> "${MANIFEST}"
printf 'platform\t%s\n' "${PLATFORM}" >> "${MANIFEST}"
printf 'archive_version\t1.16.0\n' >> "${MANIFEST}"
printf 'source_sha256\t%s\n' "$(sha256sum "${SOURCE_SQL}" | awk '{print $1}')" >> "${MANIFEST}"

for entry in \
    'none:none:0:0' \
    'gzip:gzip:6:1' \
    'lz4:lz4:1:2' \
    'zstd:zstd:3:3'
do
    IFS=: read -r name method level compression_byte <<< "${entry}"
    if [[ "${name}" == "none" ]]; then
        compress="none"
    else
        compress="${method}:${level}"
    fi

    container_archive="/tmp/pgdumpx-bench-${name}.dump"
    archive="${DATA_DIR}/pgdumpx-bench-${name}.dump"
    echo "Generating ${name} archive with --compress=${compress}"
    docker exec "${CONTAINER}" \
        pg_dump \
        --username=postgres \
        --dbname="${DATABASE}" \
        --format=custom \
        --compress="${compress}" \
        --encoding=UTF8 \
        --no-owner \
        --no-privileges \
        --no-comments \
        --strict-names \
        --table=bench.rows \
        --table=bench.rows_secondary \
        --file="${container_archive}"

    restore_list="$(docker exec "${CONTAINER}" pg_restore --list "${container_archive}")"
    grep -Eq 'TABLE DATA[[:space:]]+bench[[:space:]]+rows([[:space:]]|$)' <<< "${restore_list}" \
        || fail "TABLE DATA bench.rows is missing from ${name} archive"
    grep -Eq 'TABLE DATA[[:space:]]+bench[[:space:]]+rows_secondary([[:space:]]|$)' <<< "${restore_list}" \
        || fail "TABLE DATA bench.rows_secondary is missing from ${name} archive"

    docker cp "${CONTAINER}:${container_archive}" "${archive}" >/dev/null
    chmod 0644 "${archive}"

    python3 - "${archive}" "${compression_byte}" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
expected_compression = int(sys.argv[2])
prefix = path.read_bytes()[:12]
if prefix[:5] != b"PGDMP":
    raise SystemExit(f"{path} has invalid PGDMP magic")
if prefix[5:8] != bytes((1, 16, 0)):
    raise SystemExit(f"{path} is not archive version 1.16.0")
if prefix[11] != expected_compression:
    raise SystemExit(
        f"{path} has compression byte {prefix[11]}, expected {expected_compression}"
    )
PY

    archive_sha256="$(sha256sum "${archive}" | awk '{print $1}')"
    printf 'archive.%s.path\t%s\n' "${name}" "${archive}" >> "${MANIFEST}"
    printf 'archive.%s.compress\t%s\n' "${name}" "${compress}" >> "${MANIFEST}"
    printf 'archive.%s.sha256\t%s\n' "${name}" "${archive_sha256}" >> "${MANIFEST}"
    printf 'archive.%s.command\tpg_dump -Fc --compress=%s --table=bench.rows --table=bench.rows_secondary\n' "${name}" "${compress}" >> "${MANIFEST}"
done

echo "Benchmark dataset written to ${DATA_DIR}"
echo "Metadata written to ${MANIFEST}"
