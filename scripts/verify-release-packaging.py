#!/usr/bin/env python3
"""Verify pgdumpx 0.2.0 package contents and publication boundaries."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.2.0"
REPOSITORY = "https://github.com/tappe9/pgdumpx"
MAX_PACKAGE_BYTES = 10_000_000
PROJECT_PACKAGES = {"pgdumpx", "pgdumpx-cli"}
PACKAGE_DIRS = {
    "pgdumpx": ROOT / "crates" / "pgdumpx",
    "pgdumpx-cli": ROOT / "crates" / "pgdumpx-cli",
}
PACKAGE_EXPECTATIONS = {
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
REQUIRED_METADATA = {
    "version": VERSION,
    "edition": "2024",
    "license": "MIT OR Apache-2.0",
    "repository": REPOSITORY,
    "homepage": REPOSITORY,
}
LICENSE_FILES = {"LICENSE-APACHE", "LICENSE-MIT"}
REQUIRED_PACKAGE_FILES = {"README.md", *LICENSE_FILES}
FORBIDDEN_PACKAGE_PARTS = {
    ".github",
    ".plan",
    "benchmark-data",
    "benchmarks",
    "fixtures",
    "scripts",
}
FORBIDDEN_FILE_NAMES = {
    ".env",
    ".npmrc",
    "credentials",
    "credentials.toml",
    "id_rsa",
    "id_ed25519",
}
FORBIDDEN_RUNTIME_PACKAGES = {
    "libpq",
    "libpq-sys",
    "pq-sys",
    "postgres",
    "postgres-native-tls",
    "postgres-openssl",
    "postgres-protocol",
    "postgres-types",
    "tokio-postgres",
}
REQUIRED_DEFAULT_RUNTIME_PACKAGES = {"flate2", "lz4_flex", "ruzstd"}
ALLOWED_LICENSE_IDS = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MIT",
    "MIT-0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
}
TOKEN_RE = re.compile(r"\(|\)|\bAND\b|\bOR\b|\bWITH\b|[A-Za-z0-9.+-]+")


class AuditError(RuntimeError):
    pass


def run(
    *args: str,
    cwd: Path = ROOT,
    capture: bool = True,
    display_output: bool = True,
) -> subprocess.CompletedProcess[str]:
    print("+", " ".join(args), flush=True)
    result = subprocess.run(
        args,
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )
    if capture and display_output:
        if result.stdout:
            print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
        if result.stderr:
            print(
                result.stderr,
                end="" if result.stderr.endswith("\n") else "\n",
                file=sys.stderr,
            )
    if result.returncode != 0:
        if capture and not display_output:
            if result.stdout:
                print(result.stdout, file=sys.stderr)
            if result.stderr:
                print(result.stderr, file=sys.stderr)
        raise AuditError(f"command failed with exit code {result.returncode}: {' '.join(args)}")
    return result


def clean_status() -> str:
    return run("git", "status", "--porcelain", display_output=False).stdout.strip()


def cargo_metadata() -> dict:
    result = run(
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        display_output=False,
    )
    return json.loads(result.stdout)


def package_by_name(metadata: dict, name: str) -> dict:
    matches = [package for package in metadata["packages"] if package["name"] == name]
    if len(matches) != 1:
        raise AuditError(f"expected one package named {name}, found {len(matches)}")
    return matches[0]


def verify_project_metadata(metadata: dict) -> None:
    for name in sorted(PROJECT_PACKAGES):
        package = package_by_name(metadata, name)
        for key, expected in REQUIRED_METADATA.items():
            actual = package.get(key)
            if actual != expected:
                raise AuditError(f"{name}: metadata {key}={actual!r}, expected {expected!r}")
        for key, expected in PACKAGE_EXPECTATIONS[name].items():
            actual = package.get(key)
            if actual != expected:
                raise AuditError(f"{name}: metadata {key}={actual!r}, expected {expected!r}")
        if not package.get("readme"):
            raise AuditError(f"{name}: package readme metadata is missing")
        if package.get("rust_version") != "1.85.0":
            raise AuditError(
                f"{name}: rust-version={package.get('rust_version')!r}, expected '1.85.0'"
            )
        if package.get("publish") != ["crates-io"]:
            raise AuditError(
                f"{name}: publish={package.get('publish')!r}, expected ['crates-io']"
            )

    cli = package_by_name(metadata, "pgdumpx-cli")
    dependencies = [
        dependency
        for dependency in cli.get("dependencies", [])
        if dependency.get("name") == "pgdumpx" and dependency.get("kind") is None
    ]
    if len(dependencies) != 1:
        raise AuditError("pgdumpx-cli: expected one normal pgdumpx dependency")
    dependency = dependencies[0]
    if dependency.get("req") != "^0.2.0":
        raise AuditError(
            f"pgdumpx-cli: pgdumpx version requirement={dependency.get('req')!r}, "
            "expected '^0.2.0'"
        )
    if not dependency.get("path"):
        raise AuditError("pgdumpx-cli: pgdumpx workspace dependency must retain its path")
    if dependency.get("uses_default_features"):
        raise AuditError("pgdumpx-cli: pgdumpx dependency must disable default features")


def verify_license_copies() -> None:
    for license_name in sorted(LICENSE_FILES):
        canonical = (ROOT / license_name).read_bytes()
        for package, package_dir in PACKAGE_DIRS.items():
            packaged_copy = package_dir / license_name
            if not packaged_copy.is_file():
                raise AuditError(f"{package}: missing package-local {license_name}")
            if packaged_copy.read_bytes() != canonical:
                raise AuditError(
                    f"{package}: {license_name} differs from repository root copy"
                )


def normal_runtime_closure(metadata: dict, root_name: str) -> list[dict]:
    resolve = metadata.get("resolve")
    if not resolve:
        raise AuditError("cargo metadata did not return a resolve graph")

    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in resolve["nodes"]}
    root = package_by_name(metadata, root_name)

    seen: set[str] = set()
    stack = [root["id"]]
    while stack:
        package_id = stack.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        node = nodes.get(package_id)
        if not node:
            raise AuditError(f"resolve graph is missing node {package_id}")
        for dep in node.get("deps", []):
            dep_kinds = dep.get("dep_kinds", [])
            if any(kind.get("kind") is None for kind in dep_kinds):
                stack.append(dep["pkg"])

    return [packages[package_id] for package_id in seen]


def tokenize_license(expression: str) -> list[str]:
    normalized = expression.replace("/", " OR ")
    tokens = TOKEN_RE.findall(normalized)
    compact = re.sub(r"\s+", "", normalized)
    reconstructed = "".join(tokens)
    if compact != reconstructed:
        raise AuditError(f"unsupported SPDX license expression syntax: {expression!r}")
    return tokens


def license_is_acceptable(expression: str) -> bool:
    tokens = tokenize_license(expression)
    index = 0

    def parse_or() -> bool:
        nonlocal index
        value = parse_and()
        while index < len(tokens) and tokens[index] == "OR":
            index += 1
            rhs = parse_and()
            value = value or rhs
        return value

    def parse_and() -> bool:
        nonlocal index
        value = parse_factor()
        while index < len(tokens) and tokens[index] == "AND":
            index += 1
            rhs = parse_factor()
            value = value and rhs
        return value

    def parse_factor() -> bool:
        nonlocal index
        if index >= len(tokens):
            raise AuditError(f"truncated SPDX license expression: {expression!r}")
        token = tokens[index]
        if token == "(":
            index += 1
            value = parse_or()
            if index >= len(tokens) or tokens[index] != ")":
                raise AuditError(f"unbalanced SPDX license expression: {expression!r}")
            index += 1
            return value
        if token in {"AND", "OR", "WITH", ")"}:
            raise AuditError(f"invalid SPDX license expression: {expression!r}")
        index += 1
        value = token in ALLOWED_LICENSE_IDS
        if index < len(tokens) and tokens[index] == "WITH":
            index += 1
            if index >= len(tokens):
                raise AuditError(f"truncated SPDX WITH expression: {expression!r}")
            index += 1
            return False
        return value

    result = parse_or()
    if index != len(tokens):
        raise AuditError(f"unparsed SPDX license expression tail: {expression!r}")
    return result


def verify_runtime_dependencies(metadata: dict) -> None:
    closure = normal_runtime_closure(metadata, "pgdumpx-cli")
    names = {package["name"] for package in closure}

    missing = REQUIRED_DEFAULT_RUNTIME_PACKAGES - names
    if missing:
        raise AuditError(
            "default pgdumpx-cli runtime graph is missing compression backends: "
            + ", ".join(sorted(missing))
        )

    forbidden = FORBIDDEN_RUNTIME_PACKAGES & names
    if forbidden:
        raise AuditError(
            "PostgreSQL runtime dependency unexpectedly present: "
            + ", ".join(sorted(forbidden))
        )

    native = [
        f"{package['name']} {package['version']} (links={package['links']})"
        for package in closure
        if package.get("links")
    ]
    if native:
        raise AuditError(
            "default runtime graph contains native-linked Cargo packages; "
            "document and explicitly accept them before release:\n  " + "\n  ".join(native)
        )

    print("Runtime dependency license audit:")
    for package in sorted(closure, key=lambda item: (item["name"], item["version"])):
        if package["name"] in PROJECT_PACKAGES:
            continue
        expression = package.get("license")
        if not expression:
            raise AuditError(
                f"{package['name']} {package['version']}: dependency license metadata is missing"
            )
        if not license_is_acceptable(expression):
            raise AuditError(
                f"{package['name']} {package['version']}: license {expression!r} "
                "is outside the accepted permissive set"
            )
        print(f"  {package['name']} {package['version']}: {expression}")


def package_file_list(package: str) -> set[str]:
    result = run(
        "cargo",
        "package",
        "--list",
        "-p",
        package,
        "--locked",
        display_output=False,
    )
    return {line.strip() for line in result.stdout.splitlines() if line.strip()}


def verify_package_contents(package: str) -> None:
    files = package_file_list(package)
    missing = REQUIRED_PACKAGE_FILES - files
    if missing:
        raise AuditError(f"{package}: package is missing required files: {sorted(missing)}")

    rejected = []
    for path in sorted(files):
        parts = Path(path).parts
        lowered_parts = {part.lower() for part in parts}
        if lowered_parts & FORBIDDEN_PACKAGE_PARTS:
            rejected.append(path)
            continue
        if Path(path).name.lower() in FORBIDDEN_FILE_NAMES:
            rejected.append(path)
            continue
        if path.endswith((".dump", ".pem", ".key")):
            rejected.append(path)
    if rejected:
        raise AuditError(
            f"{package}: package contains forbidden repository or sensitive files:\n  "
            + "\n  ".join(rejected)
        )

    print(f"{package}: {len(files)} packaged files")
    for path in sorted(files):
        print(f"  {path}")


def verify_package_builds() -> None:
    run("cargo", "package", "--workspace", "--locked")


def verify_package_archives() -> None:
    for package in sorted(PROJECT_PACKAGES):
        archive = ROOT / "target" / "package" / f"{package}-{VERSION}.crate"
        if not archive.is_file():
            raise AuditError(f"{package}: expected package archive is missing: {archive}")
        size = archive.stat().st_size
        if size <= 0:
            raise AuditError(f"{package}: package archive is empty")
        if size > MAX_PACKAGE_BYTES:
            raise AuditError(
                f"{package}: package archive is {size} bytes, exceeding "
                f"{MAX_PACKAGE_BYTES} bytes"
            )
        print(f"{package}: package archive size {size} bytes")


def verify_feature_builds() -> None:
    commands = [
        ("cargo", "check", "-p", "pgdumpx", "--locked", "--no-default-features"),
        (
            "cargo",
            "check",
            "-p",
            "pgdumpx",
            "--locked",
            "--no-default-features",
            "--features",
            "lz4",
        ),
        (
            "cargo",
            "check",
            "-p",
            "pgdumpx",
            "--locked",
            "--no-default-features",
            "--features",
            "zstd",
        ),
        ("cargo", "check", "-p", "pgdumpx", "--locked"),
        ("cargo", "check", "-p", "pgdumpx-cli", "--locked"),
    ]
    for command in commands:
        run(*command)


def verify_cli_package_behavior() -> None:
    run(
        "cargo",
        "test",
        "-p",
        "pgdumpx-cli",
        "--locked",
    )

    with TemporaryDirectory(prefix="pgdumpx-install-") as temporary_root:
        install_root = Path(temporary_root)
        run(
            "cargo",
            "install",
            "--path",
            "crates/pgdumpx-cli",
            "--locked",
            "--root",
            str(install_root),
            "--force",
        )
        executable = "pgdumpx.exe" if os.name == "nt" else "pgdumpx"
        binary = install_root / "bin" / executable
        if not binary.is_file():
            raise AuditError(f"installed CLI binary is missing: {binary}")

        version = run(str(binary), "--version", display_output=False).stdout.strip()
        if version != f"pgdumpx {VERSION}":
            raise AuditError(
                f"installed CLI version output={version!r}, expected 'pgdumpx {VERSION}'"
            )

        help_output = run(str(binary), "--help", display_output=False).stdout
        missing_commands = [
            command
            for command in ("inspect", "list", "extract", "find")
            if command not in help_output
        ]
        if missing_commands:
            raise AuditError(
                "installed CLI help is missing commands: " + ", ".join(missing_commands)
            )
        print(f"installed CLI smoke passed: {version}")


def verify_library_publish_dry_run() -> None:
    run("cargo", "publish", "--dry-run", "-p", "pgdumpx", "--locked")


def verify_unsafe_policy() -> None:
    workspace_manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    if 'unsafe_code = "forbid"' not in workspace_manifest:
        raise AuditError("workspace no longer forbids project-authored unsafe code")
    for manifest_path in (ROOT / "crates").glob("*/Cargo.toml"):
        text = manifest_path.read_text(encoding="utf-8")
        if "[lints]" not in text or "workspace = true" not in text:
            raise AuditError(f"{manifest_path}: crate does not inherit workspace lints")


def main() -> int:
    try:
        before = clean_status()
        if before:
            raise AuditError("verification must start from a clean source tree")

        metadata = cargo_metadata()
        verify_project_metadata(metadata)
        verify_license_copies()
        verify_runtime_dependencies(metadata)
        verify_unsafe_policy()
        for package in sorted(PROJECT_PACKAGES):
            verify_package_contents(package)
        verify_feature_builds()
        verify_cli_package_behavior()
        verify_package_builds()
        verify_package_archives()
        verify_library_publish_dry_run()

        after = clean_status()
        if after:
            raise AuditError(
                "packaging verification modified the source tree unexpectedly:\n" + after
            )
    except (AuditError, json.JSONDecodeError, OSError) as error:
        print(f"packaging audit failed: {error}", file=sys.stderr)
        return 1

    print("release packaging audit passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
