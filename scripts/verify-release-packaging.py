#!/usr/bin/env python3
"""Verify v0.1 package contents, dependency licenses, and runtime boundaries."""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.1.0"
PROJECT_PACKAGES = {"pgdumpx", "pgdumpx-cli"}
REQUIRED_METADATA = {
    "version": VERSION,
    "edition": "2024",
    "license": "MIT OR Apache-2.0",
    "repository": "https://github.com/tappe9/pgdumpx",
}
REQUIRED_PACKAGE_FILES = {"README.md", "LICENSE-APACHE", "LICENSE-MIT"}
FORBIDDEN_PACKAGE_PARTS = {"fixtures", "benchmarks", "benchmark-data"}
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
        if not package.get("description"):
            raise AuditError(f"{name}: package description is missing")
        if not package.get("readme"):
            raise AuditError(f"{name}: package readme metadata is missing")
        if package.get("rust_version") != "1.85.0":
            raise AuditError(
                f"{name}: rust-version={package.get('rust_version')!r}, expected '1.85.0'"
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
            # No current runtime dependency needs a license exception. If one is
            # introduced, document and explicitly add support instead of silently
            # treating it as equivalent to the base license.
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
        parts = set(Path(path).parts)
        if parts & FORBIDDEN_PACKAGE_PARTS or path.endswith(".dump"):
            rejected.append(path)
    if rejected:
        raise AuditError(
            f"{package}: package contains non-distribution fixture/benchmark data:\n  "
            + "\n  ".join(rejected)
        )

    print(f"{package}: {len(files)} packaged files")
    for path in sorted(files):
        print(f"  {path}")


def extract_crate(crate_path: Path, destination: Path) -> None:
    if not crate_path.is_file():
        raise AuditError(f"expected package archive was not created: {crate_path}")
    with tarfile.open(crate_path, "r:gz") as archive:
        root = destination.resolve()
        for member in archive.getmembers():
            target = (destination / member.name).resolve()
            if root not in target.parents and target != root:
                raise AuditError(f"unsafe path in generated .crate archive: {member.name}")
        archive.extractall(destination)


def verify_package_builds() -> None:
    # The library can be fully verified directly before first publication.
    run("cargo", "package", "-p", "pgdumpx", "--locked")

    # The CLI depends on pgdumpx 0.1.0, which intentionally is not published yet.
    # Create the production .crate with Cargo, then verify that packaged source by
    # substituting only the just-packaged sibling crate for the unavailable registry
    # copy. This preserves the packaged CLI manifest/features and production source.
    run("cargo", "package", "-p", "pgdumpx-cli", "--locked", "--no-verify")

    package_root = ROOT / "target" / "package"
    library_crate = package_root / f"pgdumpx-{VERSION}.crate"
    cli_crate = package_root / f"pgdumpx-cli-{VERSION}.crate"

    with tempfile.TemporaryDirectory(prefix="pgdumpx-package-verify-") as temp_dir:
        temp = Path(temp_dir)
        extract_crate(library_crate, temp)
        extract_crate(cli_crate, temp)
        staged_library = temp / f"pgdumpx-{VERSION}"
        staged_cli = temp / f"pgdumpx-cli-{VERSION}"

        manifest = staged_cli / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        header = "[dependencies.pgdumpx]\n"
        if header not in text:
            raise AuditError("normalized CLI package manifest has no pgdumpx dependency table")
        text = text.replace(header, header + f'path = "../{staged_library.name}"\n', 1)
        manifest.write_text(text, encoding="utf-8")
        run("cargo", "check", "--manifest-path", str(manifest), cwd=temp)


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
        (
            "cargo",
            "test",
            "-p",
            "pgdumpx-cli",
            "--locked",
            "--test",
            "compression_defaults",
        ),
    ]
    for command in commands:
        run(*command)


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
        verify_runtime_dependencies(metadata)
        verify_unsafe_policy()
        for package in sorted(PROJECT_PACKAGES):
            verify_package_contents(package)
        verify_feature_builds()
        verify_package_builds()

        after = clean_status()
        if after:
            raise AuditError(
                "packaging verification modified the source tree unexpectedly:\n" + after
            )
    except (AuditError, json.JSONDecodeError, OSError, tarfile.TarError) as error:
        print(f"packaging audit failed: {error}", file=sys.stderr)
        return 1

    print("release packaging audit passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
