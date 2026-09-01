#!/usr/bin/env python3
"""Deterministic contract tests for the repository advisory policy."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERIFY_SCRIPT = ROOT / "scripts" / "verify-advisory-policy.py"

BASE_DENY = """\
[advisories]
yanked = "deny"
unmaintained = "all"
maximum-db-staleness = "P7D"
unused-ignored-advisory = "deny"
ignore = []
"""


class RepositoryContractTests(unittest.TestCase):
    def test_repository_policy_and_workflow_contract(self) -> None:
        deny_path = ROOT / "deny.toml"
        exceptions_path = ROOT / "advisory-exceptions.toml"
        workflow_path = ROOT / ".github" / "workflows" / "dependency-advisories.yml"

        self.assertTrue(deny_path.is_file(), "deny.toml must be committed at repository root")
        self.assertTrue(
            exceptions_path.is_file(),
            "advisory-exceptions.toml must own exception follow-up metadata",
        )
        self.assertTrue(
            VERIFY_SCRIPT.is_file(),
            "scripts/verify-advisory-policy.py must enforce exception metadata",
        )
        self.assertTrue(
            workflow_path.is_file(),
            "the dedicated dependency advisory workflow must be committed",
        )

        with deny_path.open("rb") as handle:
            deny = tomllib.load(handle)
        self.assertEqual(set(deny), {"advisories"})
        advisories = deny["advisories"]
        self.assertEqual(advisories["yanked"], "deny")
        self.assertEqual(advisories["unmaintained"], "all")
        self.assertEqual(advisories["maximum-db-staleness"], "P7D")
        self.assertEqual(advisories["unused-ignored-advisory"], "deny")
        self.assertIsInstance(advisories["ignore"], list)

        with exceptions_path.open("rb") as handle:
            exceptions = tomllib.load(handle)
        self.assertEqual(set(exceptions), {"exception"})
        self.assertIsInstance(exceptions["exception"], list)

        workflow = workflow_path.read_text(encoding="utf-8")
        required_fragments = (
            "pull_request:",
            "schedule:",
            "workflow_dispatch:",
            "permissions:\n  contents: read",
            "concurrency:",
            "timeout-minutes:",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            "dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772",
            "cargo install cargo-deny --version 0.20.2 --locked",
            "python3 scripts/verify-advisory-policy.py",
            "cargo deny --locked check advisories",
        )
        for fragment in required_fragments:
            self.assertIn(fragment, workflow)

        forbidden_fragments = (
            "cargo deny check licenses",
            "cargo deny check bans",
            "cargo deny check sources",
            "uses: actions/checkout@v",
        )
        for fragment in forbidden_fragments:
            self.assertNotIn(fragment, workflow)

    def test_empty_exception_policy_passes(self) -> None:
        result = run_verifier(BASE_DENY, "exception = []\n")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_bare_ignore_is_rejected(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = ["RUSTSEC-2026-0001"]',
        )
        result = run_verifier(deny, "exception = []\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("bare ignore", result.stderr)

    def test_ignore_without_follow_up_metadata_is_rejected(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = [{ id = "RUSTSEC-2026-0001", reason = "temporary transitive exposure" }]',
        )
        result = run_verifier(deny, "exception = []\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing exception metadata", result.stderr)

    def test_metadata_without_deny_ignore_is_rejected(self) -> None:
        exceptions = exception_record()
        result = run_verifier(BASE_DENY, exceptions)
        self.assertEqual(result.returncode, 1)
        self.assertIn("metadata without matching deny.toml ignore", result.stderr)

    def test_missing_required_exception_field_is_rejected(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = [{ id = "RUSTSEC-2026-0001", reason = "temporary transitive exposure" }]',
        )
        exceptions = exception_record().replace(
            'removal_condition = "upstream releases a non-vulnerable compatible version"\n',
            "",
        )
        result = run_verifier(deny, exceptions)
        self.assertEqual(result.returncode, 1)
        self.assertIn("removal_condition", result.stderr)

    def test_advisory_exception_with_tracking_issue_passes(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = [{ id = "RUSTSEC-2026-0001", reason = "temporary transitive exposure" }]',
        )
        result = run_verifier(deny, exception_record())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_yanked_exception_with_review_date_passes(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = [{ crate = "example-crate", reason = "no compatible replacement yet" }]',
        )
        exceptions = textwrap.dedent(
            """\
            [[exception]]
            kind = "yanked"
            identifier = "example-crate"
            reason = "no compatible replacement yet"
            affected_scope = "transitive build dependency on release packaging"
            review_after = "2026-12-01"
            removal_condition = "replace the dependency or update to a non-yanked release"
            """
        )
        result = run_verifier(deny, exceptions)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_exception_requires_review_date_or_tracking_issue(self) -> None:
        deny = BASE_DENY.replace(
            "ignore = []",
            'ignore = [{ id = "RUSTSEC-2026-0001", reason = "temporary transitive exposure" }]',
        )
        exceptions = exception_record().replace('tracking_issue = "#999"\n', "")
        result = run_verifier(deny, exceptions)
        self.assertEqual(result.returncode, 1)
        self.assertIn("review_after or tracking_issue", result.stderr)


def exception_record() -> str:
    return textwrap.dedent(
        """\
        [[exception]]
        kind = "advisory"
        identifier = "RUSTSEC-2026-0001"
        reason = "temporary transitive exposure"
        affected_scope = "transitive dependency used by the CLI"
        tracking_issue = "#999"
        removal_condition = "upstream releases a non-vulnerable compatible version"
        """
    )


def run_verifier(deny: str, exceptions: str) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        deny_path = root / "deny.toml"
        exceptions_path = root / "advisory-exceptions.toml"
        deny_path.write_text(deny, encoding="utf-8")
        exceptions_path.write_text(exceptions, encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(VERIFY_SCRIPT),
                "--deny",
                str(deny_path),
                "--exceptions",
                str(exceptions_path),
            ],
            check=False,
            capture_output=True,
            text=True,
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
