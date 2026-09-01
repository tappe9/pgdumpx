#!/usr/bin/env python3
"""Deterministic contract tests for scheduled fuzz campaign orchestration."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "scripts" / "run-fuzz-campaign.py"
WORKFLOW = ROOT / ".github" / "workflows" / "scheduled-fuzz.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
TARGETS = (
    "archive_open",
    "toc_metadata",
    "entry_framing",
    "copy_rows",
    "copy_metadata",
    "limit_accounting",
)


class WorkflowContractTests(unittest.TestCase):
    def test_repository_owns_bounded_scheduled_campaign(self) -> None:
        self.assertTrue(RUNNER.is_file(), "the testable fuzz campaign runner must exist")
        self.assertTrue(WORKFLOW.is_file(), "the dedicated scheduled fuzz workflow must exist")

        workflow = WORKFLOW.read_text(encoding="utf-8")
        required_fragments = (
            "schedule:",
            '- cron: "29 3 * * 0"',
            "workflow_dispatch:",
            "push:",
            "permissions:\n  contents: read",
            "concurrency:",
            "cancel-in-progress: true",
            "timeout-minutes: 15",
            "CAMPAIGN_SECONDS: ${{ github.event_name == 'push' && '10' || '300' }}",
            "cargo install cargo-fuzz --version 0.13.2 --locked",
            "cargo +nightly fuzz build --fuzz-dir fuzz",
            "python3 scripts/run-fuzz-campaign.py run",
            '"-max_total_time=$CAMPAIGN_SECONDS"',
            "-max_len=65536",
            "-timeout=10",
            '"-artifact_prefix=fuzz/artifacts/$TARGET/"',
            "if: always()",
            "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            "scheduled-fuzz-${{ matrix.target }}-${{ github.sha }}-${{ github.run_id }}",
            "fuzz/artifacts/${{ matrix.target }}/",
            "fuzz/campaign-results/${{ matrix.target }}/",
            "retention-days: 7",
            "python3 scripts/run-fuzz-campaign.py propagate",
        )
        for fragment in required_fragments:
            self.assertIn(fragment, workflow)

        for target in TARGETS:
            self.assertIn(f"- {target}", workflow)

        self.assertNotIn("pull_request:", workflow)
        run_position = workflow.index("python3 scripts/run-fuzz-campaign.py run")
        upload_position = workflow.index("actions/upload-artifact@")
        propagate_position = workflow.index("python3 scripts/run-fuzz-campaign.py propagate")
        self.assertLess(run_position, upload_position)
        self.assertLess(upload_position, propagate_position)

    def test_pull_request_smoke_remains_the_existing_64_run_gate(self) -> None:
        ci = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "for target in archive_open toc_metadata entry_framing copy_rows copy_metadata limit_accounting; do",
            ci,
        )
        self.assertIn("-runs=64 -max_len=65536 -timeout=2", ci)
        self.assertNotIn("-max_total_time", ci)


class RunnerBehaviorTests(unittest.TestCase):
    def test_success_status_is_recorded_and_propagated(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = stub_command(
                """
                import os
                from pathlib import Path
                Path(os.environ["PGDUMPX_FUZZ_ARTIFACT_DIR"]).mkdir(parents=True, exist_ok=True)
                print("stub campaign succeeded")
                """
            )
            result = run_campaign(root, "archive_open", 5, command)
            self.assertEqual(result.returncode, 0, result.stderr)

            result_dir = root / "fuzz" / "campaign-results" / "archive_open"
            artifact_dir = root / "fuzz" / "artifacts" / "archive_open"
            self.assertTrue(artifact_dir.is_dir())
            self.assertEqual((result_dir / "status.txt").read_text().strip(), "0")
            self.assertIn(
                "stub campaign succeeded",
                (result_dir / "campaign.log").read_text(encoding="utf-8"),
            )
            metadata = json.loads(
                (result_dir / "metadata.json").read_text(encoding="utf-8")
            )
            self.assertEqual(metadata["target"], "archive_open")
            self.assertEqual(metadata["max_total_time_seconds"], 5)
            self.assertEqual(metadata["exit_status"], 0)

            propagated = propagate(root, "archive_open")
            self.assertEqual(propagated.returncode, 0, propagated.stderr)

    def test_nonzero_status_is_saved_after_artifact_staging_and_reemitted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            command = stub_command(
                """
                import os
                import sys
                from pathlib import Path
                artifact_dir = Path(os.environ["PGDUMPX_FUZZ_ARTIFACT_DIR"])
                artifact_dir.mkdir(parents=True, exist_ok=True)
                (artifact_dir / "crash-stub").write_bytes(b"minimized-safe-stub")
                print("stub campaign failed after staging")
                sys.exit(23)
                """
            )
            result = run_campaign(root, "copy_rows", 5, command)
            self.assertEqual(result.returncode, 0, result.stderr)

            result_dir = root / "fuzz" / "campaign-results" / "copy_rows"
            artifact = root / "fuzz" / "artifacts" / "copy_rows" / "crash-stub"
            self.assertEqual(artifact.read_bytes(), b"minimized-safe-stub")
            self.assertEqual((result_dir / "status.txt").read_text().strip(), "23")
            self.assertIn(
                "stub campaign failed after staging",
                (result_dir / "campaign.log").read_text(encoding="utf-8"),
            )

            propagated = propagate(root, "copy_rows")
            self.assertEqual(propagated.returncode, 23)

    def test_target_name_cannot_escape_owned_artifact_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = run_campaign(root, "../escape", 5, stub_command("print('no')"))
            self.assertEqual(result.returncode, 2)
            self.assertIn("unsupported fuzz target", result.stderr)
            self.assertFalse((root / "fuzz" / "escape").exists())
            self.assertFalse((root / "escape").exists())

    def test_non_positive_time_budget_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = run_campaign(
                Path(directory),
                "archive_open",
                0,
                stub_command("print('no')"),
            )
            self.assertEqual(result.returncode, 2)
            self.assertIn("max-total-time", result.stderr)

    def test_missing_status_cannot_be_reported_as_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = propagate(Path(directory), "archive_open")
            self.assertEqual(result.returncode, 2)
            self.assertIn("status file", result.stderr)


def run_campaign(
    root: Path,
    target: str,
    max_total_time: int,
    command: list[str],
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(RUNNER),
            "run",
            "--workspace-root",
            str(root),
            "--target",
            target,
            "--max-total-time",
            str(max_total_time),
            "--",
            *command,
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def propagate(root: Path, target: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(RUNNER),
            "propagate",
            "--workspace-root",
            str(root),
            "--target",
            target,
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def stub_command(source: str) -> list[str]:
    return [sys.executable, "-c", textwrap.dedent(source)]


if __name__ == "__main__":
    unittest.main(verbosity=2)
