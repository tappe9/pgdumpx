#!/usr/bin/env python3
"""Verify the durable package and publication contract for pgdumpx 0.2.0."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.2.0"
REPOSITORY = "https://github.com/tappe9/pgdumpx"


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def require_equal(
    errors: list[str], label: str, actual: Any, expected: Any
) -> None:
    if actual != expected:
        errors.append(f"{label}={actual!r}, expected {expected!r}")


def require_file(errors: list[str], relative: str) -> Path | None:
    path = ROOT / relative
    if not path.is_file():
        errors.append(f"missing required release file: {relative}")
        return None
    return path


def require_snippets(
    errors: list[str], relative: str, snippets: list[str]
) -> None:
    path = require_file(errors, relative)
    if path is None:
        return
    text = path.read_text(encoding="utf-8")
    for snippet in snippets:
        if snippet not in text:
            errors.append(f"{relative}: missing required text {snippet!r}")


def verify_manifest_metadata(errors: list[str]) -> None:
    workspace = load_toml(ROOT / "Cargo.toml")
    workspace_package = workspace.get("workspace", {}).get("package", {})
    require_equal(
        errors,
        "workspace.package.version",
        workspace_package.get("version"),
        VERSION,
    )
    require_equal(
        errors,
        "workspace.package.homepage",
        workspace_package.get("homepage"),
        REPOSITORY,
    )

    expected = {
        "pgdumpx": {
            "description": (
                "Read-only, bounded inspection, extraction, and row scanning for "
                "PostgreSQL custom-format dumps"
            ),
            "documentation": "https://docs.rs/pgdumpx",
            "keywords": ["postgresql", "pg-dump", "backup", "parser", "forensics"],
            "categories": ["database", "parser-implementations"],
        },
        "pgdumpx-cli": {
            "description": (
                "Command-line inspection, extraction, and row scanning for PostgreSQL "
                "custom-format dumps"
            ),
            "documentation": "https://docs.rs/pgdumpx-cli",
            "keywords": ["postgresql", "pg-dump", "backup", "cli", "forensics"],
            "categories": ["database", "command-line-utilities"],
        },
    }

    for package_name, metadata in expected.items():
        manifest_path = ROOT / "crates" / package_name / "Cargo.toml"
        manifest = load_toml(manifest_path)
        package = manifest.get("package", {})
        require_equal(
            errors,
            f"{package_name}.package.version.workspace",
            package.get("version", {}).get("workspace")
            if isinstance(package.get("version"), dict)
            else None,
            True,
        )
        require_equal(
            errors,
            f"{package_name}.package.homepage.workspace",
            package.get("homepage", {}).get("workspace")
            if isinstance(package.get("homepage"), dict)
            else None,
            True,
        )
        require_equal(
            errors,
            f"{package_name}.package.publish",
            package.get("publish"),
            ["crates-io"],
        )
        for key, expected_value in metadata.items():
            require_equal(
                errors,
                f"{package_name}.package.{key}",
                package.get(key),
                expected_value,
            )

    cli = load_toml(ROOT / "crates" / "pgdumpx-cli" / "Cargo.toml")
    require_equal(
        errors,
        "pgdumpx-cli.package.autobins",
        cli.get("package", {}).get("autobins"),
        False,
    )
    require_equal(
        errors,
        "pgdumpx-cli binary targets",
        cli.get("bin"),
        [{"name": "pgdumpx", "path": "src/entrypoint.rs", "doc": False}],
    )
    dependency = cli.get("dependencies", {}).get("pgdumpx", {})
    require_equal(errors, "pgdumpx-cli dependency path", dependency.get("path"), "../pgdumpx")
    require_equal(errors, "pgdumpx-cli dependency version", dependency.get("version"), VERSION)
    require_equal(
        errors,
        "pgdumpx-cli dependency default-features",
        dependency.get("default-features"),
        False,
    )


def verify_lockfile(errors: list[str]) -> None:
    lockfile = load_toml(ROOT / "Cargo.lock")
    packages = lockfile.get("package", [])
    for name in ("pgdumpx", "pgdumpx-cli"):
        matches = [
            package
            for package in packages
            if package.get("name") == name and package.get("source") is None
        ]
        if len(matches) != 1:
            errors.append(f"Cargo.lock: expected one local package named {name}")
            continue
        require_equal(
            errors,
            f"Cargo.lock {name} version",
            matches[0].get("version"),
            VERSION,
        )


def verify_release_documents(errors: list[str]) -> None:
    require_snippets(
        errors,
        "CHANGELOG.md",
        [
            "# Changelog",
            "## [0.2.0]",
            "first public release",
        ],
    )
    require_snippets(
        errors,
        "docs/RELEASING.md",
        [
            "pgdumpx 0.2.0",
            "cargo publish --dry-run -p pgdumpx --locked",
            "cargo publish -p pgdumpx --locked",
            "cargo info pgdumpx@0.2.0",
            "cargo publish --dry-run -p pgdumpx-cli --locked",
            "cargo publish -p pgdumpx-cli --locked",
            "cargo info pgdumpx-cli@0.2.0",
            "git tag -a v0.2.0",
            "gh release create v0.2.0",
            "Do not create the tag or GitHub Release until both package versions",
            "If the library publishes but the CLI fails",
        ],
    )
    require_snippets(
        errors,
        "docs/PACKAGING.md",
        [
            "0.2.0",
            "cargo package --workspace --locked",
            "cargo publish --dry-run -p pgdumpx --locked",
            "pgdumpx-cli",
        ],
    )
    require_snippets(
        errors,
        "docs/release-notes/0.2.0.md",
        [
            "# pgdumpx 0.2.0",
            "first public release",
            "PostgreSQL custom-format dumps",
            "Archive versions 1.14 through 1.16",
        ],
    )


def verify_packaging_script(errors: list[str]) -> None:
    require_snippets(
        errors,
        "scripts/verify-release-packaging.py",
        [
            'VERSION = "0.2.0"',
            'run("cargo", "publish", "--dry-run", "-p", "pgdumpx", "--locked")',
            '"cargo",\n        "test",\n        "-p",\n        "pgdumpx-cli",',
            '"cargo",\n            "install",\n            "--path",\n            "crates/pgdumpx-cli",',
            "MAX_PACKAGE_BYTES",
        ],
    )


def main() -> int:
    errors: list[str] = []
    try:
        verify_manifest_metadata(errors)
        verify_lockfile(errors)
        verify_release_documents(errors)
        verify_packaging_script(errors)
    except (OSError, tomllib.TOMLDecodeError) as error:
        errors.append(str(error))

    if errors:
        print("release readiness contract failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(f"release readiness contract passed for pgdumpx {VERSION}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
