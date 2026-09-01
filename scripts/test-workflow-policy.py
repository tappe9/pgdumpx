#!/usr/bin/env python3
"""Verify bounded, least-privilege, immutably pinned GitHub workflows."""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_DIR = ROOT / ".github" / "workflows"
ACTION_SHA_RE = re.compile(r"^[0-9a-fA-F]{40}$")
USES_RE = re.compile(r"^\s*uses:\s*([^@\s]+)@([^\s#]+)", re.MULTILINE)
JOB_RE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$", re.MULTILINE)


def _top_level_block(text: str, key: str) -> str | None:
    lines = text.splitlines()
    marker = f"{key}:"
    for index, line in enumerate(lines):
        if line.rstrip() != marker or line[: len(line) - len(line.lstrip())]:
            continue
        block = [line]
        for following in lines[index + 1 :]:
            if following and not following.startswith((" ", "\t")):
                break
            block.append(following)
        return "\n".join(block)
    return None


def _job_blocks(text: str) -> list[tuple[str, str]]:
    jobs_marker = re.search(r"^jobs:\s*$", text, re.MULTILINE)
    if jobs_marker is None:
        return []

    jobs_text = text[jobs_marker.end() :]
    matches = list(JOB_RE.finditer(jobs_text))
    blocks: list[tuple[str, str]] = []
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(jobs_text)
        blocks.append((match.group(1), jobs_text[match.start() : end]))
    return blocks


def _checkout_step_blocks(text: str) -> list[str]:
    lines = text.splitlines()
    blocks: list[str] = []
    for index, line in enumerate(lines):
        if "uses: actions/checkout@" not in line:
            continue

        uses_indent = len(line) - len(line.lstrip())
        step_indent = max(0, uses_indent - 2)
        start = index
        while start > 0:
            candidate = lines[start - 1]
            indent = len(candidate) - len(candidate.lstrip())
            if indent == step_indent and candidate.lstrip().startswith("- "):
                start -= 1
                break
            start -= 1

        end = index + 1
        while end < len(lines):
            candidate = lines[end]
            indent = len(candidate) - len(candidate.lstrip())
            if indent == step_indent and candidate.lstrip().startswith("- "):
                break
            if candidate and indent < step_indent:
                break
            end += 1
        blocks.append("\n".join(lines[start:end]))
    return blocks


def validate_workflow(path: Path, text: str) -> list[str]:
    errors: list[str] = []

    permissions = _top_level_block(text, "permissions")
    if permissions is None or not re.search(
        r"^  contents:\s*read\s*(?:#.*)?$", permissions, re.MULTILINE
    ):
        errors.append(f"{path}: top-level permissions must include contents: read")

    concurrency = _top_level_block(text, "concurrency")
    if concurrency is None:
        errors.append(f"{path}: missing top-level concurrency policy")
    else:
        if not re.search(r"^  group:\s*\S+", concurrency, re.MULTILINE):
            errors.append(f"{path}: concurrency must define a non-empty group")
        if not re.search(
            r"^  cancel-in-progress:\s*true\s*(?:#.*)?$",
            concurrency,
            re.MULTILINE,
        ):
            errors.append(f"{path}: concurrency must cancel in-progress duplicates")

    jobs = _job_blocks(text)
    if not jobs:
        errors.append(f"{path}: workflow must define at least one job")
    for job_name, block in jobs:
        if not re.search(
            r"^    timeout-minutes:\s*[1-9][0-9]*\s*(?:#.*)?$",
            block,
            re.MULTILINE,
        ):
            errors.append(f"{path}: job {job_name!r} must define a finite timeout")

    for action, revision in USES_RE.findall(text):
        if not ACTION_SHA_RE.fullmatch(revision):
            errors.append(
                f"{path}: {action}@{revision} must use an immutable 40-character SHA"
            )

    for block in _checkout_step_blocks(text):
        if not re.search(
            r"^\s+persist-credentials:\s*false\s*(?:#.*)?$",
            block,
            re.MULTILINE,
        ):
            errors.append(
                f"{path}: every actions/checkout step must disable credential persistence"
            )

    return errors


_VALID_WORKFLOW = """name: Contract fixture
on: [push]
permissions:
  contents: read
concurrency:
  group: fixture-${{ github.ref }}
  cancel-in-progress: true
jobs:
  verify:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: false
"""


class WorkflowPolicyTests(unittest.TestCase):
    def validate(self, text: str) -> list[str]:
        return validate_workflow(Path("fixture.yml"), text)

    def test_accepts_bounded_pinned_read_only_workflow(self) -> None:
        self.assertEqual(self.validate(_VALID_WORKFLOW), [])

    def test_rejects_additional_write_permission(self) -> None:
        errors = self.validate(
            _VALID_WORKFLOW.replace(
                "  contents: read\n",
                "  contents: read\n  issues: write\n",
            )
        )
        self.assertTrue(any("only contents: read" in error for error in errors))

    def test_rejects_floating_action_revision(self) -> None:
        errors = self.validate(
            _VALID_WORKFLOW.replace(
                "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
                "actions/checkout@v7",
            )
        )
        self.assertTrue(any("immutable 40-character SHA" in error for error in errors))

    def test_rejects_missing_job_timeout(self) -> None:
        errors = self.validate(_VALID_WORKFLOW.replace("    timeout-minutes: 10\n", ""))
        self.assertTrue(any("finite timeout" in error for error in errors))

    def test_rejects_missing_concurrency(self) -> None:
        errors = self.validate(
            _VALID_WORKFLOW.replace(
                "concurrency:\n  group: fixture-${{ github.ref }}\n  cancel-in-progress: true\n",
                "",
            )
        )
        self.assertTrue(any("missing top-level concurrency" in error for error in errors))

    def test_rejects_persisted_checkout_credentials(self) -> None:
        errors = self.validate(
            _VALID_WORKFLOW.replace("          persist-credentials: false\n", "")
        )
        self.assertTrue(any("disable credential persistence" in error for error in errors))

    def test_rejects_missing_read_only_contents_permission(self) -> None:
        errors = self.validate(_VALID_WORKFLOW.replace("  contents: read", "  contents: write"))
        self.assertTrue(any("contents: read" in error for error in errors))


def _run_self_tests() -> bool:
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(WorkflowPolicyTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return result.wasSuccessful()


def main() -> int:
    if not _run_self_tests():
        return 1

    workflow_paths = sorted(WORKFLOW_DIR.glob("*.yml")) + sorted(
        WORKFLOW_DIR.glob("*.yaml")
    )
    if not workflow_paths:
        print("workflow policy contract failed: no workflows found", file=sys.stderr)
        return 1

    errors: list[str] = []
    for path in workflow_paths:
        errors.extend(validate_workflow(path.relative_to(ROOT), path.read_text(encoding="utf-8")))

    if errors:
        print("workflow policy contract failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print(f"workflow policy contract passed for {len(workflow_paths)} workflows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
