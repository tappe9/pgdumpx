#!/usr/bin/env python3
"""Run one fuzz target, stage evidence, then propagate its saved status later."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

TARGETS = frozenset(
    {
        "archive_open",
        "toc_metadata",
        "entry_framing",
        "copy_rows",
        "copy_metadata",
        "limit_accounting",
    }
)
MAX_CAMPAIGN_SECONDS = 3_600
DEFAULT_WORKSPACE_ROOT = Path(__file__).resolve().parents[1]


class CampaignError(ValueError):
    """A deterministic campaign configuration or result error."""


@dataclass(frozen=True)
class CampaignPaths:
    artifact_dir: Path
    result_dir: Path
    log_path: Path
    status_path: Path
    metadata_path: Path


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a bounded pgdumpx fuzz campaign and preserve its exit status."
    )
    subparsers = parser.add_subparsers(dest="subcommand", required=True)

    run_parser = subparsers.add_parser(
        "run",
        help="run a command, stage logs/artifacts, and save its exit status",
    )
    add_shared_arguments(run_parser)
    run_parser.add_argument(
        "--max-total-time",
        type=int,
        required=True,
        help=f"positive campaign budget in seconds (maximum {MAX_CAMPAIGN_SECONDS})",
    )
    run_parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="command to execute after --",
    )

    propagate_parser = subparsers.add_parser(
        "propagate",
        help="exit with the status saved by a previous run subcommand",
    )
    add_shared_arguments(propagate_parser)

    return parser.parse_args(argv)


def add_shared_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--workspace-root",
        type=Path,
        default=DEFAULT_WORKSPACE_ROOT,
        help="repository workspace containing fuzz/",
    )
    parser.add_argument("--target", required=True, help="maintained fuzz target name")


def validate_target(target: str) -> str:
    if target not in TARGETS:
        raise CampaignError(f"unsupported fuzz target: {target}")
    return target


def validate_max_total_time(value: int) -> int:
    if value <= 0 or value > MAX_CAMPAIGN_SECONDS:
        raise CampaignError(
            f"max-total-time must be between 1 and {MAX_CAMPAIGN_SECONDS} seconds"
        )
    return value


def owned_child(root: Path, *parts: str) -> Path:
    resolved_root = root.resolve()
    candidate = resolved_root.joinpath(*parts).resolve()
    try:
        candidate.relative_to(resolved_root)
    except ValueError as error:
        raise CampaignError(f"path escapes workspace root: {candidate}") from error
    return candidate


def campaign_paths(workspace_root: Path, target: str) -> CampaignPaths:
    root = workspace_root.resolve()
    fuzz_root = owned_child(root, "fuzz")
    artifact_dir = owned_child(fuzz_root, "artifacts", target)
    result_dir = owned_child(fuzz_root, "campaign-results", target)
    return CampaignPaths(
        artifact_dir=artifact_dir,
        result_dir=result_dir,
        log_path=owned_child(result_dir, "campaign.log"),
        status_path=owned_child(result_dir, "status.txt"),
        metadata_path=owned_child(result_dir, "metadata.json"),
    )


def normalize_status(returncode: int) -> int:
    if returncode < 0:
        return min(255, 128 + abs(returncode))
    if returncode > 255:
        return 1
    return returncode


def atomic_write_text(path: Path, content: str) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(path)


def execute_and_record(
    workspace_root: Path,
    target: str,
    max_total_time: int,
    command: Sequence[str],
) -> int:
    target = validate_target(target)
    max_total_time = validate_max_total_time(max_total_time)
    command_parts = list(command)
    if command_parts and command_parts[0] == "--":
        command_parts.pop(0)
    if not command_parts:
        raise CampaignError("run requires a command after --")

    paths = campaign_paths(workspace_root, target)
    paths.artifact_dir.mkdir(parents=True, exist_ok=True)
    paths.result_dir.mkdir(parents=True, exist_ok=True)

    environment = os.environ.copy()
    environment.update(
        {
            "PGDUMPX_FUZZ_TARGET": target,
            "PGDUMPX_FUZZ_ARTIFACT_DIR": str(paths.artifact_dir),
            "PGDUMPX_FUZZ_RESULT_DIR": str(paths.result_dir),
            "PGDUMPX_FUZZ_MAX_TOTAL_TIME": str(max_total_time),
        }
    )

    returncode: int
    with paths.log_path.open("w", encoding="utf-8") as log:
        try:
            process = subprocess.Popen(
                command_parts,
                cwd=workspace_root.resolve(),
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                encoding="utf-8",
                errors="replace",
                bufsize=1,
            )
        except OSError as error:
            message = f"failed to start fuzz command: {error}\n"
            print(message, end="", file=sys.stderr)
            log.write(message)
            log.flush()
            returncode = 127
        else:
            if process.stdout is None:
                process.kill()
                process.wait()
                raise CampaignError("fuzz command stdout pipe was not created")
            for line in process.stdout:
                print(line, end="", flush=True)
                log.write(line)
                log.flush()
            returncode = normalize_status(process.wait())

    atomic_write_text(paths.status_path, f"{returncode}\n")
    metadata = {
        "schema_version": 1,
        "target": target,
        "max_total_time_seconds": max_total_time,
        "exit_status": returncode,
    }
    atomic_write_text(
        paths.metadata_path,
        json.dumps(metadata, indent=2, sort_keys=True) + "\n",
    )
    print(
        f"saved fuzz campaign status {returncode} for {target} in {paths.result_dir}"
    )

    # The workflow must upload evidence before the original status is re-emitted.
    return 0


def propagate_status(workspace_root: Path, target: str) -> int:
    target = validate_target(target)
    paths = campaign_paths(workspace_root, target)
    if not paths.status_path.is_file():
        raise CampaignError(f"status file does not exist: {paths.status_path}")

    raw_status = paths.status_path.read_text(encoding="utf-8").strip()
    try:
        status = int(raw_status, 10)
    except ValueError as error:
        raise CampaignError(
            f"status file does not contain an integer: {paths.status_path}"
        ) from error
    if status < 0 or status > 255:
        raise CampaignError(f"status file contains an invalid exit status: {status}")

    if status != 0:
        print(
            f"fuzz campaign for {target} failed with saved exit status {status}",
            file=sys.stderr,
        )
    else:
        print(f"fuzz campaign for {target} completed successfully")
    return status


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        if args.subcommand == "run":
            return execute_and_record(
                args.workspace_root,
                args.target,
                args.max_total_time,
                args.command,
            )
        if args.subcommand == "propagate":
            return propagate_status(args.workspace_root, args.target)
        raise CampaignError(f"unknown subcommand: {args.subcommand}")
    except (CampaignError, OSError) as error:
        print(f"fuzz campaign error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
