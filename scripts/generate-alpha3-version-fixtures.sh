#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly PLATFORM="linux/amd64"
readonly DATABASE="pgdumpx_fixture"
readonly SOURCE_SQL="${ROOT}/tests/fixtures/source/alpha1-copy-basic.sql"
readonly ARCHIVE_DIR="${ROOT}/tests/fixtures/archives"
readonly PG16_IMAGE="postgres:16.15-bookworm"
readonly PG15_IMAGE="postgres:15.19-bookworm"

containers=()
cleanup() {
    for container in "${containers[@]:-}"; do
        docker rm --force "${container}" >/dev/null 2>&1 || true
    done
}
trap cleanup EXIT

fail() {
    echo "fixture generation failed: $*" >&2
    exit 1
}

command -v docker >/dev/null 2>&1 || fail "docker is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
[[ -f "${SOURCE_SQL}" ]] || fail "source SQL not found: ${SOURCE_SQL}"
mkdir -p "${ARCHIVE_DIR}"

generate_version() {
    local major="$1"
    local expected_archive_version="$2"
    local image_tag="$3"
    local none_compression="$4"
    local gzip_compression="$5"
    local container="pgdumpx-pg${major}-alpha3-fixtures"
    local none_name="pg${major}-none-copy-basic"
    local gzip_name="pg${major}-gzip-copy-basic"
    local none_container="/tmp/${none_name}.dump"
    local gzip_container="/tmp/${gzip_name}.dump"
    local none_archive="${ARCHIVE_DIR}/${none_name}.dump"
    local gzip_archive="${ARCHIVE_DIR}/${gzip_name}.dump"

    containers+=("${container}")
    docker rm --force "${container}" >/dev/null 2>&1 || true
    docker pull --platform "${PLATFORM}" "${image_tag}" >/dev/null

    local image_digest
    image_digest="$(docker image inspect --format '{{index .RepoDigests 0}}' "${image_tag}")"
    [[ "${image_digest}" == postgres@sha256:* ]] || fail "unexpected image digest: ${image_digest}"

    docker run \
        --detach \
        --rm \
        --platform "${PLATFORM}" \
        --name "${container}" \
        --env POSTGRES_PASSWORD=fixture-only \
        --env POSTGRES_DB="${DATABASE}" \
        --mount "type=bind,src=${SOURCE_SQL},dst=/docker-entrypoint-initdb.d/001-alpha1-copy-basic.sql,readonly" \
        "${image_digest}" >/dev/null

    local row_count=""
    for _ in $(seq 1 90); do
        row_count="$(
            docker exec "${container}" psql \
                --username=postgres \
                --dbname="${DATABASE}" \
                --tuples-only \
                --no-align \
                --command='SELECT count(*) FROM public.orders;' 2>/dev/null \
                | tr -d '[:space:]' || true
        )"
        [[ "${row_count}" == "7" ]] && break
        sleep 1
    done
    [[ "${row_count}" == "7" ]] || fail "PostgreSQL ${major} fixture database did not initialize"

    local generator_version
    generator_version="$(docker exec "${container}" pg_dump --version | tr -d '\r')"

    docker exec "${container}" pg_dump \
        --username=postgres \
        --dbname="${DATABASE}" \
        --format=custom \
        --compress="${none_compression}" \
        --encoding=UTF8 \
        --no-owner \
        --no-privileges \
        --no-comments \
        --strict-names \
        --table=public.orders \
        --file="${none_container}"

    docker exec "${container}" pg_dump \
        --username=postgres \
        --dbname="${DATABASE}" \
        --format=custom \
        --compress="${gzip_compression}" \
        --encoding=UTF8 \
        --no-owner \
        --no-privileges \
        --no-comments \
        --strict-names \
        --table=public.orders \
        --file="${gzip_container}"

    for archive in "${none_container}" "${gzip_container}"; do
        docker exec "${container}" pg_restore --list "${archive}" \
            | grep -Eq 'TABLE[[:space:]]+public[[:space:]]+orders' \
            || fail "TABLE public.orders missing from ${archive}"
        docker exec "${container}" pg_restore --list "${archive}" \
            | grep -Eq 'TABLE DATA[[:space:]]+public[[:space:]]+orders' \
            || fail "TABLE DATA public.orders missing from ${archive}"
    done

    docker cp "${container}:${none_container}" "${none_archive}" >/dev/null
    docker cp "${container}:${gzip_container}" "${gzip_archive}" >/dev/null
    chmod 0644 "${none_archive}" "${gzip_archive}"

    python3 - "${expected_archive_version}" "${none_archive}" "${gzip_archive}" <<'PY'
from pathlib import Path
import sys

expected = tuple(int(part) for part in sys.argv[1].split('.'))
for raw_path in sys.argv[2:]:
    path = Path(raw_path)
    prefix = path.read_bytes()[:8]
    if prefix[:5] != b"PGDMP":
        raise SystemExit(f"{path} has invalid PGDMP magic")
    if tuple(prefix[5:8]) != expected:
        raise SystemExit(
            f"{path} has archive version {tuple(prefix[5:8])}, expected {expected}"
        )
PY

    local none_sha gzip_sha
    none_sha="$(sha256sum "${none_archive}" | awk '{print $1}')"
    gzip_sha="$(sha256sum "${gzip_archive}" | awk '{print $1}')"

    printf 'major=%s\n' "${major}"
    printf 'archive_version=%s\n' "${expected_archive_version}"
    printf 'generator=%s\n' "${generator_version}"
    printf 'generator_image=%s\n' "${image_tag}"
    printf 'generator_image_digest=%s\n' "${image_digest}"
    printf 'generator_platform=%s\n' "${PLATFORM}"
    printf 'none_command=docker exec %s pg_dump --username=postgres --dbname=%s --format=custom --compress=%s --encoding=UTF8 --no-owner --no-privileges --no-comments --strict-names --table=public.orders --file=%s\n' "${container}" "${DATABASE}" "${none_compression}" "${none_container}"
    printf 'none_sha256=%s\n' "${none_sha}"
    printf 'gzip_command=docker exec %s pg_dump --username=postgres --dbname=%s --format=custom --compress=%s --encoding=UTF8 --no-owner --no-privileges --no-comments --strict-names --table=public.orders --file=%s\n' "${container}" "${DATABASE}" "${gzip_compression}" "${gzip_container}"
    printf 'gzip_sha256=%s\n' "${gzip_sha}"
    printf '\n'

    docker rm --force "${container}" >/dev/null 2>&1 || true
}

generate_version 16 1.15.0 "${PG16_IMAGE}" none gzip:6
generate_version 15 1.14.0 "${PG15_IMAGE}" 0 6
