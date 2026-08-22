#!/usr/bin/env python3
"""Verify that repository-local Markdown links resolve to existing paths."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
MARKDOWN_FILES = sorted(
    path
    for path in ROOT.rglob("*.md")
    if not any(part in {"target", ".git"} for part in path.parts)
)

INLINE_LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")
IMAGE_LINK = re.compile(r"!\[[^\]]*\]\(([^)]+)\)")
REFERENCE_LINK = re.compile(r"^\s*\[[^\]]+\]:\s*(\S+)", re.MULTILINE)
SCHEMES = ("http://", "https://", "mailto:")


def normalize_destination(raw: str) -> str | None:
    destination = raw.strip()
    if destination.startswith("<") and ">" in destination:
        destination = destination[1 : destination.index(">")]
    else:
        # Markdown permits an optional title after whitespace. Repository paths here do
        # not contain spaces, so the first token is the destination.
        destination = destination.split(maxsplit=1)[0]

    if not destination or destination.startswith("#") or destination.startswith(SCHEMES):
        return None

    destination = unquote(destination.split("#", 1)[0])
    return destination or None


def resolve(source: Path, destination: str) -> Path:
    if destination.startswith("/"):
        return ROOT / destination.lstrip("/")
    return source.parent / destination


def main() -> int:
    failures: list[str] = []
    checked = 0

    for source in MARKDOWN_FILES:
        text = source.read_text(encoding="utf-8")
        matches = [
            *INLINE_LINK.findall(text),
            *IMAGE_LINK.findall(text),
            *REFERENCE_LINK.findall(text),
        ]
        for raw in matches:
            destination = normalize_destination(raw)
            if destination is None:
                continue
            checked += 1
            target = resolve(source, destination)
            if not target.exists():
                failures.append(
                    f"{source.relative_to(ROOT)}: {raw!r} -> "
                    f"{target.resolve(strict=False).relative_to(ROOT.resolve())}"
                )

    if failures:
        print("broken repository-local Markdown links:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"verified {checked} repository-local Markdown links across {len(MARKDOWN_FILES)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
