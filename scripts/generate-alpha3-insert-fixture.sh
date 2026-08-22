#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly IMAGE_TAG="postgres:18.4-bookworm"
readonly IMAGE_DIGEST="postgres@sha256:882236b897e39051d2368c5ccc6cda944904723506b2dfc97f2a8f5bc9afa382"
readonly PLATFORM="linux/amd64"
readonly DATABASE="pgdumpx_fixture"
readonly SOURCE_SQL="${ROOT}/tests/fixtures/source/alpha1-copy-basic.sql"
readonly ARCHIVE_DIR="${ROOT}/tests/fixtures/archives"
readonly NAME="pg18-none-insert-basic"
readonly CONTAINER="pgdumpx-pg18-insert-fixture"
readonly CONTAINER_ARCHIVE="/tmp/${NAME}.dump"
readonly ARCHIVE="${ARCHIVE_DIR}/${NAME}.dump"
readonly PROVENANCE="${ARCHIVE_DIR}/${NAME}.provenance"
readonly RESTORED="/tmp/${NAME}-data.sql"
readonly WORK_DIR="$(mktemp -d)"

cleanup() {
    docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true
    rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

fail() {
    echo "INSERT fixture generation failed: $*" >&2
    exit 1
}

for command in docker python3 sha256sum; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done
[[ -f "${SOURCE_SQL}" ]] || fail "source SQL not found: ${SOURCE_SQL}"
mkdir -p "${ARCHIVE_DIR}"
docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true

echo "Pulling ${IMAGE_DIGEST} for ${PLATFORM}"
docker pull --platform "${PLATFORM}" "${IMAGE_DIGEST}" >/dev/null

echo "Starting ${IMAGE_DIGEST}"
docker run \
    --detach \
    --rm \
    --platform "${PLATFORM}" \
    --name "${CONTAINER}" \
    --env POSTGRES_PASSWORD=fixture-only \
    --env POSTGRES_DB="${DATABASE}" \
    --mount "type=bind,src=${SOURCE_SQL},dst=/docker-entrypoint-initdb.d/001-alpha1-copy-basic.sql,readonly" \
    "${IMAGE_DIGEST}" >/dev/null

row_count=""
for _ in $(seq 1 90); do
    row_count="$(
        docker exec "${CONTAINER}" \
            psql --username=postgres --dbname="${DATABASE}" --tuples-only --no-align \
            --command='SELECT count(*) FROM public.orders;' 2>/dev/null \
            | tr -d '[:space:]' || true
    )"
    [[ "${row_count}" == "7" ]] && break
    sleep 1
done
[[ "${row_count}" == "7" ]] || fail "public.orders was not initialized with seven rows"

GENERATOR_VERSION="$(docker exec "${CONTAINER}" pg_dump --version | tr -d '\r')"
[[ "${GENERATOR_VERSION}" == "pg_dump (PostgreSQL) 18.4"* ]] \
    || fail "unexpected pg_dump version: ${GENERATOR_VERSION}"
readonly GENERATOR_VERSION
readonly DUMP_COMMAND="docker exec ${CONTAINER} pg_dump --username=postgres --dbname=${DATABASE} --format=custom --compress=none --inserts --encoding=UTF8 --no-owner --no-privileges --no-comments --strict-names --table=public.orders --file=${CONTAINER_ARCHIVE}"

echo "Generating ${NAME}"
docker exec "${CONTAINER}" \
    pg_dump \
    --username=postgres \
    --dbname="${DATABASE}" \
    --format=custom \
    --compress=none \
    --inserts \
    --encoding=UTF8 \
    --no-owner \
    --no-privileges \
    --no-comments \
    --strict-names \
    --table=public.orders \
    --file="${CONTAINER_ARCHIVE}"

docker exec "${CONTAINER}" pg_restore --list "${CONTAINER_ARCHIVE}" > "${WORK_DIR}/archive.list"
grep -Eq 'TABLE[[:space:]]+public[[:space:]]+orders' "${WORK_DIR}/archive.list" \
    || fail "TABLE public.orders is missing"
grep -Eq 'TABLE DATA[[:space:]]+public[[:space:]]+orders' "${WORK_DIR}/archive.list" \
    || fail "TABLE DATA public.orders is missing"

docker exec "${CONTAINER}" pg_restore --data-only --file="${RESTORED}" "${CONTAINER_ARCHIVE}"
docker cp "${CONTAINER}:${RESTORED}" "${WORK_DIR}/orders-data.sql" >/dev/null
python3 - "${WORK_DIR}/orders-data.sql" <<'PY'
from pathlib import Path
import sys

text = Path(sys.argv[1]).read_text(encoding="utf-8")
insert_lines = [line for line in text.splitlines() if line.startswith("INSERT INTO public.orders VALUES (")]
if len(insert_lines) != 7:
    raise SystemExit(f"expected seven INSERT rows, found {len(insert_lines)}")
if "COPY public.orders " in text:
    raise SystemExit("INSERT fixture unexpectedly contains COPY output")
for expected in [
    "'EARLY-100'",
    "'SECOND-200'",
    "'THIRD-300'",
    "'MIDDLE-400'",
    "'FIFTH-500'",
    "'SIXTH-600'",
    "'LATE-700'",
]:
    if not any(expected in line for line in insert_lines):
        raise SystemExit(f"expected value {expected} is missing from INSERT output")
PY

docker cp "${CONTAINER}:${CONTAINER_ARCHIVE}" "${ARCHIVE}" >/dev/null
chmod 0644 "${ARCHIVE}"
python3 - "${ARCHIVE}" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
prefix = path.read_bytes()[:12]
if prefix[:5] != b"PGDMP":
    raise SystemExit(f"{path} has invalid PGDMP magic")
if prefix[5:8] != bytes((1, 16, 0)):
    raise SystemExit(f"{path} is not archive version 1.16.0")
if prefix[11] != 0:
    raise SystemExit(f"{path} compression byte is {prefix[11]}, expected none (0)")
PY

SHA256="$(sha256sum "${ARCHIVE}" | awk '{print $1}')"
cat > "${PROVENANCE}" <<EOF
name=${NAME}
source=tests/fixtures/source/alpha1-copy-basic.sql
archive_version=1.16.0
generator=${GENERATOR_VERSION}
generator_image=${IMAGE_TAG}
generator_image_digest=${IMAGE_DIGEST}
generator_platform=${PLATFORM}
command=${DUMP_COMMAND}
compression=none
representation=insert
sha256=${SHA256}
expected_tables=public.orders
expected_row_count=7
EOF

printf 'Generator: %s\n' "${GENERATOR_VERSION}"
printf 'Image tag: %s\n' "${IMAGE_TAG}"
printf 'Image digest: %s\n' "${IMAGE_DIGEST}"
printf 'Command: %s\n' "${DUMP_COMMAND}"
printf '%s  %s\n' "${SHA256}" "${ARCHIVE#${ROOT}/}"
