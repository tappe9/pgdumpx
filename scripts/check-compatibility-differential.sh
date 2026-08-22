#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly IMAGE_DIGEST="postgres@sha256:882236b897e39051d2368c5ccc6cda944904723506b2dfc97f2a8f5bc9afa382"
readonly PLATFORM="linux/amd64"
readonly PGDUMPX="${ROOT}/target/debug/pgdumpx"

fail() {
    echo "compatibility differential check failed: $*" >&2
    exit 1
}

for command in docker python3; do
    command -v "${command}" >/dev/null 2>&1 || fail "${command} is required"
done
[[ -x "${PGDUMPX}" ]] || fail "build pgdumpx-cli first: cargo build -p pgdumpx-cli --all-features"
[[ -f "${ROOT}/tests/fixtures/manifest.toml" ]] || fail "fixture manifest is missing"

echo "Pulling ${IMAGE_DIGEST} for ${PLATFORM}"
docker pull --platform "${PLATFORM}" "${IMAGE_DIGEST}" >/dev/null

python3 - "${ROOT}" "${IMAGE_DIGEST}" "${PLATFORM}" "${PGDUMPX}" <<'PY'
from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import tomllib

root = Path(sys.argv[1])
image = sys.argv[2]
platform = sys.argv[3]
pgdumpx = Path(sys.argv[4])
manifest = tomllib.loads((root / "tests/fixtures/manifest.toml").read_text(encoding="utf-8"))
fixtures = manifest.get("fixture", [])
if not fixtures:
    raise SystemExit("fixture manifest contains no fixtures")

COPY_HEADER = b"COPY public.orders (order_id, order_number, customer_code, note, empty_text) FROM stdin;\n"
COPY_END = b"\\.\n"
INSERT_PREFIX = b"INSERT INTO public.orders VALUES ("
INSERT_END = b");\n"
EXPECTED_ROWS = 7


def run_checked(command: list[str], *, label: str) -> bytes:
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", errors="replace")
        raise SystemExit(f"{label} failed with exit {result.returncode}:\n{stderr}")
    return result.stdout


def restore_data(relative_path: str) -> bytes:
    archive_in_container = f"/work/{relative_path}"
    return run_checked(
        [
            "docker",
            "run",
            "--rm",
            "--platform",
            platform,
            "--mount",
            f"type=bind,src={root},dst=/work,readonly",
            image,
            "pg_restore",
            "--data-only",
            "--file=-",
            archive_in_container,
        ],
        label=f"pg_restore {relative_path}",
    )


def copy_rows_from_raw(raw: bytes, name: str) -> list[bytes]:
    end = raw.find(COPY_END)
    if end < 0:
        raise SystemExit(f"{name}: raw extraction has no COPY terminator")
    trailing = raw[end + len(COPY_END):]
    if trailing.strip(b"\r\n"):
        raise SystemExit(f"{name}: unexpected bytes after raw COPY terminator")
    rows = raw[:end].splitlines()
    if len(rows) != EXPECTED_ROWS:
        raise SystemExit(f"{name}: raw COPY row count is {len(rows)}, expected {EXPECTED_ROWS}")
    return rows


def copy_rows_from_restore(restored: bytes, name: str) -> list[bytes]:
    start = restored.find(COPY_HEADER)
    if start < 0:
        raise SystemExit(f"{name}: pg_restore output has no expected COPY header")
    data_start = start + len(COPY_HEADER)
    end = restored.find(COPY_END, data_start)
    if end < 0:
        raise SystemExit(f"{name}: pg_restore COPY output has no terminator")
    rows = restored[data_start:end].splitlines()
    if len(rows) != EXPECTED_ROWS:
        raise SystemExit(f"{name}: pg_restore COPY row count is {len(rows)}, expected {EXPECTED_ROWS}")
    return rows


def insert_region(data: bytes, name: str, source: str) -> bytes:
    start = data.find(INSERT_PREFIX)
    if start < 0:
        raise SystemExit(f"{name}: {source} has no INSERT payload")
    end = data.rfind(INSERT_END, start)
    if end < 0:
        raise SystemExit(f"{name}: {source} has no complete INSERT terminator")
    region = data[start:end + len(INSERT_END)].rstrip(b"\r\n")
    count = region.count(INSERT_PREFIX)
    if count != EXPECTED_ROWS:
        raise SystemExit(f"{name}: {source} INSERT count is {count}, expected {EXPECTED_ROWS}")
    return region


checked = 0
for fixture in fixtures:
    name = fixture["name"]
    relative_path = fixture["path"]
    archive = root / relative_path
    purposes = set(fixture.get("purpose", []))
    if not archive.is_file():
        raise SystemExit(f"{name}: fixture is missing: {relative_path}")

    is_copy = "copy-text" in purposes
    is_insert = "insert" in purposes
    if is_copy == is_insert:
        raise SystemExit(f"{name}: fixture must declare exactly one COPY/INSERT representation")

    raw = run_checked(
        [str(pgdumpx), "extract", str(archive), "public.orders"],
        label=f"pgdumpx extract {name}",
    )
    restored = restore_data(relative_path)

    if is_copy:
        actual = copy_rows_from_raw(raw, name)
        expected = copy_rows_from_restore(restored, name)
        if actual != expected:
            raise SystemExit(f"{name}: pgdumpx COPY rows differ from pg_restore")
        representation = "copy"
    else:
        actual = insert_region(raw, name, "pgdumpx raw extraction")
        expected = insert_region(restored, name, "pg_restore output")
        if actual != expected:
            raise SystemExit(f"{name}: pgdumpx INSERT bytes differ from pg_restore")
        representation = "insert"

    checked += 1
    print(
        f"verified {name}: archive={fixture['archive_version']} "
        f"compression={fixture['compression']} representation={representation}"
    )

if checked != len(fixtures):
    raise SystemExit(f"verified {checked} fixtures but manifest contains {len(fixtures)}")
print(f"compatibility differential verified {checked} official fixtures")
PY
