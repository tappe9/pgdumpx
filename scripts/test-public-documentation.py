#!/usr/bin/env python3
"""Verify that public documentation matches the published pgdumpx 0.2.0 release."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_SNIPPETS = {
    "README.md": [
        "`pgdumpx 0.2.0` is published on crates.io",
        "## Installation",
        "### CLI from crates.io",
        "### Library from crates.io",
        "### From source",
        "https://crates.io/crates/pgdumpx",
        "https://crates.io/crates/pgdumpx-cli",
        "https://docs.rs/pgdumpx/0.2.0/pgdumpx/",
        "cargo install pgdumpx-cli --version 0.2.0 --locked",
        "`Archive::open_path`",
        "`TableSelector`",
        "`ExtractionPlan`",
        "`MetadataFilter`",
        "`find_first_equal_with_limits`",
    ],
    "README.ja.md": [
        "`pgdumpx 0.2.0`はcrates.ioで公開済みです",
        "## インストール",
        "### crates.ioからCLIをinstall",
        "### crates.ioからlibraryを利用",
        "### ソースから",
        "https://crates.io/crates/pgdumpx",
        "https://crates.io/crates/pgdumpx-cli",
        "https://docs.rs/pgdumpx/0.2.0/pgdumpx/",
        "cargo install pgdumpx-cli --version 0.2.0 --locked",
        "`Archive::open_path`",
        "`TableSelector`",
        "`ExtractionPlan`",
        "`MetadataFilter`",
        "`find_first_equal_with_limits`",
    ],
    "ROADMAP.md": [
        "Status: **v0.2.0 released",
        "## v0.2 — Completed and released",
        "### Delivered",
        "#57–#63",
        "#70–#78",
        "#89",
        "#92",
        "https://crates.io/crates/pgdumpx/0.2.0",
        "https://crates.io/crates/pgdumpx-cli/0.2.0",
        "https://github.com/tappe9/pgdumpx/releases/tag/v0.2.0",
        "## v0.3+ — Deferred candidates",
    ],
    "docs/API-DESIGN.md": [
        "## 2.1 File-oriented convenience",
        "## 10.1 Owned table selectors",
        "## 11.1 Reusable extraction plans",
        "## 11.2 Sequential multi-table execution",
        "## 15.1 Metadata filtering",
        "## 16.1 Exact named-column equality",
    ],
    "SECURITY.md": [
        "Security fixes target the latest published 0.2.x release and the current `main` branch.",
        "https://crates.io/crates/pgdumpx",
        "https://github.com/tappe9/pgdumpx/releases",
        "## Dependency advisory policy",
        "## Fuzzing",
    ],
    "CONTRIBUTING.md": [
        "python3 scripts/test-public-documentation.py",
        "python3 scripts/test-release-readiness.py",
        "python3 scripts/verify-release-packaging.py",
        "[Release procedure](docs/RELEASING.md)",
    ],
    "docs/PACKAGING.md": [
        "Status: **0.2.0 published on crates.io**",
        "`pgdumpx 0.2.0` — published 2026-09-01 12:36:29 UTC",
        "`pgdumpx-cli 0.2.0` — published 2026-09-01 12:36:56 UTC",
        "ef4d0cb73fdf21a87dd7e2515adf83cdf4415e13707dfed41bd9ad4576e9dd6b",
        "9a6f0d8f690d4ab65bc9a2e5079397966b12d473629bdd21cfc939bcb56414fe",
    ],
    "docs/RELEASING.md": [
        "## 0.2.0 completion record",
        "Published library package",
        "Published CLI package",
        "GitHub Release",
        "https://github.com/tappe9/pgdumpx/releases/tag/v0.2.0",
    ],
    "docs/release-notes/0.2.0.md": [
        "## Published artifacts",
        "https://crates.io/crates/pgdumpx/0.2.0",
        "https://crates.io/crates/pgdumpx-cli/0.2.0",
        "https://docs.rs/pgdumpx/0.2.0/pgdumpx/",
        "cargo install pgdumpx-cli --version 0.2.0 --locked",
    ],
}

FORBIDDEN_SNIPPETS = {
    "README.md": [
        "Registry commands require the exact package version to be available",
        "Check the exact versions before installing",
        "v0.2 is planned",
        "no v0.2 production code has merged yet",
        "**Planned next:** v0.2",
    ],
    "README.ja.md": [
        "registry commandを使うには、対象package versionがcrates.ioに存在する必要があります",
        "crates.ioでpackage versionを確認してから",
        "v0.2は[Tracking Issue #56]",
        "v0.2のproduction codeはまだ`main`へmergeされていません",
        "**次に実装するv0.2:**",
    ],
    "ROADMAP.md": [
        "the 0.2.0 publication process is tracked separately from source delivery",
        "this roadmap does not infer their live state",
        "## v0.2 candidate",
        "No production implementation exists yet",
    ],
    "docs/API-DESIGN.md": [
        "The current repository is still in the design phase",
    ],
    "SECURITY.md": [
        "If no package has been published yet",
        "when one exists",
        "pgdumpx has no published release version yet",
        "## Current v0.1 scope boundary",
    ],
    "docs/PACKAGING.md": [
        "registry publication pending credentials",
    ],
    "docs/release-notes/0.2.0.md": [
        "After both crates.io packages are published:",
    ],
}

ALIGNED_README_IDENTIFIERS = [
    "0.2.0",
    "pgdumpx-cli",
    "Archive::open_path",
    "TableSelector",
    "ExtractionPlan",
    "MetadataFilter",
    "find_first_equal_with_limits",
    "100,000",
    "64 MiB",
    "Binary COPY",
]


def read_document(relative: str, errors: list[str]) -> str:
    path = ROOT / relative
    if not path.is_file():
        errors.append(f"missing public document: {relative}")
        return ""
    return path.read_text(encoding="utf-8")


def main() -> int:
    errors: list[str] = []
    documents = {
        relative: read_document(relative, errors)
        for relative in set(REQUIRED_SNIPPETS) | set(FORBIDDEN_SNIPPETS)
    }

    for relative, snippets in REQUIRED_SNIPPETS.items():
        text = documents[relative]
        for snippet in snippets:
            if snippet not in text:
                errors.append(f"{relative}: missing required text {snippet!r}")

    for relative, snippets in FORBIDDEN_SNIPPETS.items():
        text = documents[relative]
        for snippet in snippets:
            if snippet in text:
                errors.append(f"{relative}: stale text remains {snippet!r}")

    english = documents["README.md"]
    japanese = documents["README.ja.md"]
    for identifier in ALIGNED_README_IDENTIFIERS:
        missing = [
            relative
            for relative, text in (("README.md", english), ("README.ja.md", japanese))
            if identifier not in text
        ]
        if missing:
            errors.append(
                f"README alignment: {identifier!r} missing from {', '.join(missing)}"
            )

    if errors:
        print("public documentation contract failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print("public documentation contract passed for published pgdumpx 0.2.0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
