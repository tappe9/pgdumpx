#!/usr/bin/env python3
"""Verify that public documentation matches pgdumpx 0.2.0."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_SNIPPETS = {
    "README.md": [
        "Current source version: `0.2.0`",
        "## Installation",
        "### From crates.io",
        "### From source",
        "Registry commands require",
        "`Archive::open_path`",
        "`TableSelector`",
        "`ExtractionPlan`",
        "`MetadataFilter`",
        "`find_first_equal_with_limits`",
    ],
    "README.ja.md": [
        "現在のソースバージョン: `0.2.0`",
        "## インストール",
        "### crates.io から",
        "### ソースから",
        "crates.ioでpackage versionを確認してから",
        "`Archive::open_path`",
        "`TableSelector`",
        "`ExtractionPlan`",
        "`MetadataFilter`",
        "`find_first_equal_with_limits`",
    ],
    "ROADMAP.md": [
        "## v0.2 — Completed",
        "### Delivered",
        "#57–#63",
        "#70–#78",
        "#89",
        "#92",
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
        "latest published 0.2.x release",
        "current `main`",
        "If no package has been published yet",
    ],
    "CONTRIBUTING.md": [
        "python3 scripts/test-public-documentation.py",
        "python3 scripts/test-release-readiness.py",
        "python3 scripts/verify-release-packaging.py",
        "[Release procedure](docs/RELEASING.md)",
    ],
}

FORBIDDEN_SNIPPETS = {
    "README.md": [
        "v0.2 is planned",
        "no v0.2 production code has merged yet",
        "**Planned next:** v0.2",
    ],
    "README.ja.md": [
        "v0.2は[Tracking Issue #56]",
        "v0.2のproduction codeはまだ`main`へmergeされていません",
        "**次に実装するv0.2:**",
    ],
    "ROADMAP.md": [
        "## v0.2 candidate",
        "No production implementation exists yet",
    ],
    "docs/API-DESIGN.md": [
        "The current repository is still in the design phase",
    ],
    "SECURITY.md": [
        "pgdumpx has no published release version yet",
        "## Current v0.1 scope boundary",
    ],
}

ALIGNED_README_IDENTIFIERS = [
    "0.2.0",
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

    print("public documentation contract passed for pgdumpx 0.2.0")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
