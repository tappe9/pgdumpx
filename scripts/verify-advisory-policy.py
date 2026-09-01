#!/usr/bin/env python3
"""Cross-check cargo-deny ignores against repository-owned follow-up metadata."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any

ADVISORY_ID = re.compile(r"^RUSTSEC-\d{4}-\d{4}$")
TRACKING_ISSUE = re.compile(r"^(?:#\d+|https://github\.com/[^/]+/[^/]+/issues/\d+)$")
ALLOWED_EXCEPTION_FIELDS = {
    "kind",
    "identifier",
    "reason",
    "affected_scope",
    "review_after",
    "tracking_issue",
    "removal_condition",
}


class PolicyError(ValueError):
    """A deterministic repository policy validation failure."""


@dataclass(frozen=True)
class ExceptionKey:
    kind: str
    identifier: str

    def display(self) -> str:
        return f"{self.kind}:{self.identifier}"


@dataclass(frozen=True)
class IgnoreEntry:
    key: ExceptionKey
    reason: str


@dataclass(frozen=True)
class ExceptionEntry:
    key: ExceptionKey
    reason: str


def parse_args() -> argparse.Namespace:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Verify deny.toml ignores and advisory exception metadata stay aligned."
    )
    parser.add_argument("--deny", type=Path, default=root / "deny.toml")
    parser.add_argument(
        "--exceptions",
        type=Path,
        default=root / "advisory-exceptions.toml",
    )
    return parser.parse_args()


def load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            document = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(f"cannot read {path}: {error}") from error
    if not isinstance(document, dict):
        raise PolicyError(f"{path} must contain a TOML document")
    return document


def require_text(record: dict[str, Any], field: str, context: str) -> str:
    value = record.get(field)
    if not isinstance(value, str) or not value.strip():
        raise PolicyError(f"{context} requires non-empty {field}")
    return value.strip()


def collect_ignores(document: dict[str, Any]) -> dict[ExceptionKey, IgnoreEntry]:
    advisories = document.get("advisories")
    if not isinstance(advisories, dict):
        raise PolicyError("deny.toml requires an [advisories] table")
    raw_ignores = advisories.get("ignore", [])
    if not isinstance(raw_ignores, list):
        raise PolicyError("advisories.ignore must be an array")

    ignores: dict[ExceptionKey, IgnoreEntry] = {}
    for index, raw in enumerate(raw_ignores):
        context = f"advisories.ignore[{index}]"
        if isinstance(raw, str):
            raise PolicyError(
                f"{context} is a bare ignore; use an object with id/crate and reason"
            )
        if not isinstance(raw, dict):
            raise PolicyError(f"{context} must be an object")

        unknown = set(raw) - {"id", "crate", "reason"}
        if unknown:
            raise PolicyError(f"{context} has unsupported fields: {sorted(unknown)}")

        advisory_id = raw.get("id")
        crate = raw.get("crate")
        if (advisory_id is None) == (crate is None):
            raise PolicyError(f"{context} must set exactly one of id or crate")

        reason = require_text(raw, "reason", context)
        if advisory_id is not None:
            if not isinstance(advisory_id, str) or not ADVISORY_ID.fullmatch(advisory_id):
                raise PolicyError(f"{context}.id must be a RUSTSEC-YYYY-NNNN identifier")
            key = ExceptionKey("advisory", advisory_id)
        else:
            if not isinstance(crate, str) or not crate.strip():
                raise PolicyError(f"{context}.crate must be a non-empty crate name")
            key = ExceptionKey("yanked", crate.strip())

        if key in ignores:
            raise PolicyError(f"duplicate deny.toml ignore for {key.display()}")
        ignores[key] = IgnoreEntry(key, reason)
    return ignores


def validate_review_metadata(record: dict[str, Any], context: str) -> None:
    review_after = record.get("review_after")
    tracking_issue = record.get("tracking_issue")
    if review_after is None and tracking_issue is None:
        raise PolicyError(f"{context} requires review_after or tracking_issue")

    if review_after is not None:
        if isinstance(review_after, date):
            pass
        elif isinstance(review_after, str):
            try:
                date.fromisoformat(review_after)
            except ValueError as error:
                raise PolicyError(f"{context}.review_after must be YYYY-MM-DD") from error
        else:
            raise PolicyError(f"{context}.review_after must be YYYY-MM-DD")

    if tracking_issue is not None:
        if not isinstance(tracking_issue, str) or not TRACKING_ISSUE.fullmatch(
            tracking_issue.strip()
        ):
            raise PolicyError(
                f"{context}.tracking_issue must be #123 or a GitHub issue URL"
            )


def collect_exceptions(document: dict[str, Any]) -> dict[ExceptionKey, ExceptionEntry]:
    raw_exceptions = document.get("exception")
    if not isinstance(raw_exceptions, list):
        raise PolicyError("advisory-exceptions.toml requires exception = [] or [[exception]]")

    exceptions: dict[ExceptionKey, ExceptionEntry] = {}
    for index, raw in enumerate(raw_exceptions):
        context = f"exception[{index}]"
        if not isinstance(raw, dict):
            raise PolicyError(f"{context} must be a table")
        unknown = set(raw) - ALLOWED_EXCEPTION_FIELDS
        if unknown:
            raise PolicyError(f"{context} has unsupported fields: {sorted(unknown)}")

        kind = require_text(raw, "kind", context)
        if kind not in {"advisory", "yanked"}:
            raise PolicyError(f"{context}.kind must be advisory or yanked")
        identifier = require_text(raw, "identifier", context)
        if kind == "advisory" and not ADVISORY_ID.fullmatch(identifier):
            raise PolicyError(f"{context}.identifier must be a RUSTSEC-YYYY-NNNN identifier")

        reason = require_text(raw, "reason", context)
        require_text(raw, "affected_scope", context)
        require_text(raw, "removal_condition", context)
        validate_review_metadata(raw, context)

        key = ExceptionKey(kind, identifier)
        if key in exceptions:
            raise PolicyError(f"duplicate exception metadata for {key.display()}")
        exceptions[key] = ExceptionEntry(key, reason)
    return exceptions


def verify(deny_path: Path, exceptions_path: Path) -> int:
    ignores = collect_ignores(load_toml(deny_path))
    exceptions = collect_exceptions(load_toml(exceptions_path))

    missing = sorted(
        (key.display() for key in ignores.keys() - exceptions.keys()),
    )
    if missing:
        raise PolicyError(f"missing exception metadata for {', '.join(missing)}")

    extra = sorted(
        (key.display() for key in exceptions.keys() - ignores.keys()),
    )
    if extra:
        raise PolicyError(
            "metadata without matching deny.toml ignore for " + ", ".join(extra)
        )

    for key, ignore in ignores.items():
        exception = exceptions[key]
        if ignore.reason != exception.reason:
            raise PolicyError(
                f"reason mismatch for {key.display()}: deny.toml and metadata must match"
            )

    print(
        f"verified {len(ignores)} advisory/yanked exception(s) across "
        f"{deny_path} and {exceptions_path}"
    )
    return 0


def main() -> int:
    args = parse_args()
    try:
        return verify(args.deny, args.exceptions)
    except PolicyError as error:
        print(f"advisory policy error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
