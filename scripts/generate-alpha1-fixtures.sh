#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly IMAGE_TAG="postgres:18.4-bookworm"
readonly PLATFORM="linux/amd64"
readonly CONTAINER="pgdumpx-pg18-fixtures"
readonly DATABASE="pgdumpx_fixture"
readonly SOURCE_RELATIVE="tests/fixtures/source/alpha1-copy-basic.sql"
readonly SOURCE_SQL="${ROOT}/${SOURCE_RELATIVE}"
readonly ARCHIVE_DIR="${ROOT}/tests/fixtures/archives"
readonly MANIFEST="${ROOT}/tests/fixtures/manifest.toml"
readonly NONE_NAME="pg18-none-copy-basic"
readonly GZIP_NAME="pg18-gzip-copy-basic"
readonly NONE_CONTAINER_PATH="/tmp/${NONE_NAME}.dump"
readonly GZIP_CONTAINER_PATH="/tmp/${GZIP_NAME}.dump"
readonly NONE_ARCHIVE="${ARCHIVE_DIR}/${NONE_NAME}.dump"
readonly GZIP_ARCHIVE="${ARCHIVE_DIR}/${GZIP_NAME}.dump"
readonly RESTORED_DATA_CONTAINER_PATH="/tmp/orders-data.sql"
readonly WORK_DIR="$(mktemp -d)"

cleanup() {
    docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true
    rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

fail() {
    echo "fixture generation failed: $*" >&2
    exit 1
}

command -v docker >/dev/null 2>&1 || fail "docker is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
[[ -f "${SOURCE_SQL}" ]] || fail "source SQL not found: ${SOURCE_SQL}"

mkdir -p "${ARCHIVE_DIR}"
docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true

echo "Pulling ${IMAGE_TAG} for ${PLATFORM}"
docker pull --platform "${PLATFORM}" "${IMAGE_TAG}"

IMAGE_DIGEST="$(docker image inspect --format '{{index .RepoDigests 0}}' "${IMAGE_TAG}")"
[[ "${IMAGE_DIGEST}" == postgres@sha256:* ]] \
    || fail "unexpected image digest: ${IMAGE_DIGEST}"
readonly IMAGE_DIGEST

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
            psql \
            --username=postgres \
            --dbname="${DATABASE}" \
            --tuples-only \
            --no-align \
            --command='SELECT count(*) FROM public.orders;' \
            2>/dev/null \
            | tr -d '[:space:]' \
            || true
    )"
    [[ "${row_count}" == "7" ]] && break
    sleep 1
done

if [[ "${row_count}" != "7" ]]; then
    docker logs "${CONTAINER}" >&2 || true
    fail "public.orders was not initialized with seven rows"
fi

GENERATOR_VERSION="$(docker exec "${CONTAINER}" pg_dump --version | tr -d '\r')"
[[ "${GENERATOR_VERSION}" == "pg_dump (PostgreSQL) 18.4"* ]] \
    || fail "unexpected pg_dump version: ${GENERATOR_VERSION}"
readonly GENERATOR_VERSION

readonly NONE_COMMAND="docker exec ${CONTAINER} pg_dump --username=postgres --dbname=${DATABASE} --format=custom --compress=none --encoding=UTF8 --no-owner --no-privileges --no-comments --strict-names --table=public.orders --file=${NONE_CONTAINER_PATH}"
readonly GZIP_COMMAND="docker exec ${CONTAINER} pg_dump --username=postgres --dbname=${DATABASE} --format=custom --compress=gzip:6 --encoding=UTF8 --no-owner --no-privileges --no-comments --strict-names --table=public.orders --file=${GZIP_CONTAINER_PATH}"

echo "Generating ${NONE_NAME}"
docker exec "${CONTAINER}" \
    pg_dump \
    --username=postgres \
    --dbname="${DATABASE}" \
    --format=custom \
    --compress=none \
    --encoding=UTF8 \
    --no-owner \
    --no-privileges \
    --no-comments \
    --strict-names \
    --table=public.orders \
    --file="${NONE_CONTAINER_PATH}"

echo "Generating ${GZIP_NAME}"
docker exec "${CONTAINER}" \
    pg_dump \
    --username=postgres \
    --dbname="${DATABASE}" \
    --format=custom \
    --compress=gzip:6 \
    --encoding=UTF8 \
    --no-owner \
    --no-privileges \
    --no-comments \
    --strict-names \
    --table=public.orders \
    --file="${GZIP_CONTAINER_PATH}"

docker exec "${CONTAINER}" pg_restore --list "${NONE_CONTAINER_PATH}" \
    > "${WORK_DIR}/none.list"
docker exec "${CONTAINER}" pg_restore --list "${GZIP_CONTAINER_PATH}" \
    > "${WORK_DIR}/gzip.list"

for list_file in "${WORK_DIR}/none.list" "${WORK_DIR}/gzip.list"; do
    grep -Eq 'TABLE[[:space:]]+public[[:space:]]+orders' "${list_file}" \
        || fail "TABLE public.orders is missing from ${list_file}"
    grep -Eq 'TABLE DATA[[:space:]]+public[[:space:]]+orders' "${list_file}" \
        || fail "TABLE DATA public.orders is missing from ${list_file}"
done

docker exec "${CONTAINER}" \
    pg_restore \
    --data-only \
    --file="${RESTORED_DATA_CONTAINER_PATH}" \
    "${NONE_CONTAINER_PATH}"
docker cp "${CONTAINER}:${RESTORED_DATA_CONTAINER_PATH}" \
    "${WORK_DIR}/orders-data.sql" >/dev/null

python3 - "${WORK_DIR}/orders-data.sql" <<'PY'
from pathlib import Path
import sys

text = Path(sys.argv[1]).read_text(encoding="utf-8")
rows = []
in_copy = False
for line in text.splitlines():
    if line.startswith("COPY public.orders "):
        expected = (
            "COPY public.orders (order_id, order_number, customer_code, note, empty_text) "
            "FROM stdin;"
        )
        if line != expected:
            raise SystemExit(f"unexpected COPY column layout: {line}")
        in_copy = True
        continue
    if in_copy and line == r"\.":
        break
    if in_copy:
        rows.append(line.split("\t"))

expected_order_numbers = [
    "EARLY-100",
    "SECOND-200",
    "THIRD-300",
    "MIDDLE-400",
    "FIFTH-500",
    "SIXTH-600",
    "LATE-700",
]
if [row[1] for row in rows] != expected_order_numbers:
    raise SystemExit(f"unexpected row order: {rows}")
if rows[3][3] != r"\N":
    raise SystemExit("NULL note was not preserved as COPY \\N")
if rows[4][3] != "":
    raise SystemExit("empty non-NULL note was not preserved")
expected_escapes = {
    "SECOND-200": r"tab\tvalue",
    "THIRD-300": r"line1\nline2",
    "SIXTH-600": r"carriage\rreturn",
    "LATE-700": r"backslash\\value",
}
for row in rows:
    expected_note = expected_escapes.get(row[1])
    if expected_note is not None and row[3] != expected_note:
        raise SystemExit(f"unexpected COPY escape spelling for {row[1]}: {row[3]!r}")
PY

docker cp "${CONTAINER}:${NONE_CONTAINER_PATH}" "${NONE_ARCHIVE}" >/dev/null
docker cp "${CONTAINER}:${GZIP_CONTAINER_PATH}" "${GZIP_ARCHIVE}" >/dev/null
chmod 0644 "${NONE_ARCHIVE}" "${GZIP_ARCHIVE}"

python3 - "${NONE_ARCHIVE}" "${GZIP_ARCHIVE}" <<'PY'
from pathlib import Path
import sys

for raw_path in sys.argv[1:]:
    path = Path(raw_path)
    prefix = path.read_bytes()[:8]
    if prefix[:5] != b"PGDMP":
        raise SystemExit(f"{path} has invalid PGDMP magic")
    if prefix[5:8] != bytes((1, 16, 0)):
        raise SystemExit(f"{path} has archive version bytes {tuple(prefix[5:8])}, expected 1.16.0")
PY

NONE_SHA256="$(sha256sum "${NONE_ARCHIVE}" | awk '{print $1}')"
GZIP_SHA256="$(sha256sum "${GZIP_ARCHIVE}" | awk '{print $1}')"
readonly NONE_SHA256 GZIP_SHA256

FIXTURE_GENERATOR_VERSION="${GENERATOR_VERSION}" \
GENERATOR_IMAGE="${IMAGE_TAG}" \
GENERATOR_IMAGE_DIGEST="${IMAGE_DIGEST}" \
GENERATOR_PLATFORM="${PLATFORM}" \
NONE_COMMAND="${NONE_COMMAND}" \
GZIP_COMMAND="${GZIP_COMMAND}" \
NONE_SHA256="${NONE_SHA256}" \
GZIP_SHA256="${GZIP_SHA256}" \
MANIFEST_PATH="${MANIFEST}" \
python3 <<'PY'
from pathlib import Path
import json
import os


def quoted(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def string_array(values: list[str]) -> str:
    return "[" + ", ".join(quoted(value) for value in values) + "]"


common = {
    "source": "tests/fixtures/source/alpha1-copy-basic.sql",
    "archive_version": "1.16.0",
    "generator": os.environ["FIXTURE_GENERATOR_VERSION"],
    "generator_image": os.environ["GENERATOR_IMAGE"],
    "generator_image_digest": os.environ["GENERATOR_IMAGE_DIGEST"],
    "generator_platform": os.environ["GENERATOR_PLATFORM"],
    "expected_tables": ["public.orders"],
    "expected_row_count": 7,
    "expected_columns": [
        "order_id",
        "order_number",
        "customer_code",
        "note",
        "empty_text",
    ],
}
fixtures = [
    {
        **common,
        "name": "pg18-none-copy-basic",
        "path": "tests/fixtures/archives/pg18-none-copy-basic.dump",
        "command": os.environ["NONE_COMMAND"],
        "compression": "none",
        "compression_detail": "none",
        "sha256": os.environ["NONE_SHA256"],
        "purpose": [
            "header",
            "toc",
            "none",
            "copy-text",
            "column-layout",
            "find-first",
        ],
    },
    {
        **common,
        "name": "pg18-gzip-copy-basic",
        "path": "tests/fixtures/archives/pg18-gzip-copy-basic.dump",
        "command": os.environ["GZIP_COMMAND"],
        "compression": "gzip",
        "compression_detail": "level=6",
        "sha256": os.environ["GZIP_SHA256"],
        "purpose": [
            "header",
            "toc",
            "gzip",
            "copy-text",
            "column-layout",
            "find-first",
        ],
    },
]

lines = ["manifest_version = 1", ""]
for fixture in fixtures:
    lines.extend(
        [
            "[[fixture]]",
            f"name = {quoted(fixture['name'])}",
            f"path = {quoted(fixture['path'])}",
            f"source = {quoted(fixture['source'])}",
            f"archive_version = {quoted(fixture['archive_version'])}",
            f"generator = {quoted(fixture['generator'])}",
            f"generator_image = {quoted(fixture['generator_image'])}",
            f"generator_image_digest = {quoted(fixture['generator_image_digest'])}",
            f"generator_platform = {quoted(fixture['generator_platform'])}",
            f"command = {quoted(fixture['command'])}",
            f"compression = {quoted(fixture['compression'])}",
            f"compression_detail = {quoted(fixture['compression_detail'])}",
            f"sha256 = {quoted(fixture['sha256'])}",
            f"purpose = {string_array(fixture['purpose'])}",
            f"expected_tables = {string_array(fixture['expected_tables'])}",
            f"expected_row_count = {fixture['expected_row_count']}",
            f"expected_columns = {string_array(fixture['expected_columns'])}",
            "",
        ]
    )

Path(os.environ["MANIFEST_PATH"]).write_text("\n".join(lines), encoding="utf-8")
PY

printf 'Generator: %s\n' "${GENERATOR_VERSION}"
printf 'Image: %s\n' "${IMAGE_DIGEST}"
printf '%s  %s\n' "${NONE_SHA256}" "${NONE_ARCHIVE#${ROOT}/}"
printf '%s  %s\n' "${GZIP_SHA256}" "${GZIP_ARCHIVE#${ROOT}/}"
