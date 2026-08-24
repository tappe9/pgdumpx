#!/usr/bin/env python3
from __future__ import annotations

import csv
import math
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import NoReturn

BASELINE_BUFFER_BYTES = 8192
MIN_OVERALL_GAIN_PCT = 5.0
MAX_CASE_REGRESSION_PCT = 3.0
MAX_RSS_INCREASE_KIB = 2048.0
BUFFER_KINDS = ("copy", "gzip-input")
EXPECTED_SCENARIOS = ("single", "multi")
EXPECTED_COMPRESSIONS = ("none", "gzip", "lz4", "zstd")
RSS_LABEL = re.compile(
    r"^kind=(?P<kind>copy|gzip-input)/buffer=(?P<buffer>\d+)/"
    r"(?P<compression>[^/]+)/(?P<scenario>single|multi)/repetition=(?P<repetition>\d+)$"
)


def fail(message: str) -> NoReturn:
    raise SystemExit(f"buffer tuning summary failed: {message}")


def median(values: list[float], label: str) -> float:
    if not values:
        fail(f"missing samples for {label}")
    return float(statistics.median(values))


def expected_cases(kind: str) -> set[tuple[str, str]]:
    compressions = EXPECTED_COMPRESSIONS if kind == "copy" else ("gzip",)
    return {
        (scenario, compression)
        for scenario in EXPECTED_SCENARIOS
        for compression in compressions
    }


def read_throughput(path: Path):
    samples: dict[tuple[str, int, str, str], list[float]] = defaultdict(list)
    with path.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        required = {
            "buffer_kind",
            "buffer_bytes",
            "scenario",
            "compression",
            "units_per_second",
            "outcome",
        }
        if not reader.fieldnames or not required.issubset(reader.fieldnames):
            fail(f"{path} is missing required columns")
        for row in reader:
            if row["buffer_kind"] not in BUFFER_KINDS:
                fail(f"unknown buffer kind: {row['buffer_kind']!r}")
            if row["outcome"] != "success":
                fail(f"non-success throughput outcome: {row}")
            key = (
                row["buffer_kind"],
                int(row["buffer_bytes"]),
                row["scenario"],
                row["compression"],
            )
            samples[key].append(float(row["units_per_second"]))
    return samples


def read_rss(path: Path):
    samples: dict[tuple[str, int, str, str], list[float]] = defaultdict(list)
    with path.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if not reader.fieldnames or not {"label", "peak_rss_kib"}.issubset(reader.fieldnames):
            fail(f"{path} is missing required columns")
        for row in reader:
            match = RSS_LABEL.fullmatch(row["label"])
            if match is None:
                fail(f"unexpected peak-RSS label: {row['label']!r}")
            key = (
                match.group("kind"),
                int(match.group("buffer")),
                match.group("scenario"),
                match.group("compression"),
            )
            samples[key].append(float(row["peak_rss_kib"]))
    return samples


def write_tsv(path: Path, fieldnames: list[str], rows: list[dict[str, object]]) -> None:
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=fieldnames,
            delimiter="\t",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: summarize-buffer-tuning.py <RESULTS_DIR>")
    results_dir = Path(sys.argv[1])
    throughput_path = results_dir / "throughput.tsv"
    rss_path = results_dir / "peak-rss.tsv"
    if not throughput_path.is_file() or not rss_path.is_file():
        fail("throughput.tsv and peak-rss.tsv are required")

    throughput = read_throughput(throughput_path)
    rss = read_rss(rss_path)
    case_rows: list[dict[str, object]] = []
    candidate_rows: list[dict[str, object]] = []
    decision_rows: list[dict[str, object]] = []

    for kind in BUFFER_KINDS:
        throughput_buffers = sorted({buffer for k, buffer, _, _ in throughput if k == kind})
        rss_buffers = sorted({buffer for k, buffer, _, _ in rss if k == kind})
        if not throughput_buffers:
            fail(f"missing throughput samples for {kind}")
        if throughput_buffers != rss_buffers:
            fail(f"throughput and peak-RSS candidate sets differ for {kind}")
        if BASELINE_BUFFER_BYTES not in throughput_buffers:
            fail(f"missing {BASELINE_BUFFER_BYTES}-byte baseline for {kind}")

        required_cases = expected_cases(kind)
        for buffer in throughput_buffers:
            throughput_cases = {
                (scenario, compression)
                for k, b, scenario, compression in throughput
                if k == kind and b == buffer
            }
            rss_cases = {
                (scenario, compression)
                for k, b, scenario, compression in rss
                if k == kind and b == buffer
            }
            if throughput_cases != required_cases:
                fail(f"buffer {kind}/{buffer} throughput cases do not match the required matrix")
            if rss_cases != required_cases:
                fail(f"buffer {kind}/{buffer} peak-RSS cases do not match the required matrix")

        candidate_metrics: dict[int, tuple[float, float, float, bool]] = {}
        for buffer in throughput_buffers:
            ratios: list[float] = []
            case_deltas: list[float] = []
            rss_deltas: list[float] = []
            for scenario, compression in sorted(required_cases):
                key = (kind, buffer, scenario, compression)
                baseline_key = (kind, BASELINE_BUFFER_BYTES, scenario, compression)
                rate = median(throughput[key], f"throughput {key}")
                baseline_rate = median(throughput[baseline_key], f"throughput {baseline_key}")
                if rate <= 0.0 or baseline_rate <= 0.0:
                    fail(f"non-positive throughput for {key}")
                rate_ratio = rate / baseline_rate
                delta_pct = (rate_ratio - 1.0) * 100.0
                rss_median = median(rss[key], f"peak RSS {key}")
                baseline_rss = median(rss[baseline_key], f"peak RSS {baseline_key}")
                rss_delta = rss_median - baseline_rss
                ratios.append(rate_ratio)
                case_deltas.append(delta_pct)
                rss_deltas.append(rss_delta)
                case_rows.append(
                    {
                        "buffer_kind": kind,
                        "buffer_bytes": buffer,
                        "scenario": scenario,
                        "compression": compression,
                        "throughput_samples": len(throughput[key]),
                        "median_units_per_second": f"{rate:.3f}",
                        "baseline_median_units_per_second": f"{baseline_rate:.3f}",
                        "throughput_delta_pct": f"{delta_pct:.3f}",
                        "rss_samples": len(rss[key]),
                        "median_peak_rss_kib": f"{rss_median:.1f}",
                        "baseline_median_peak_rss_kib": f"{baseline_rss:.1f}",
                        "peak_rss_delta_kib": f"{rss_delta:.1f}",
                    }
                )

            geometric_ratio = math.exp(sum(math.log(ratio) for ratio in ratios) / len(ratios))
            overall_gain_pct = (geometric_ratio - 1.0) * 100.0
            worst_case_pct = min(case_deltas)
            max_rss_delta_kib = max(rss_deltas)
            qualified = (
                buffer != BASELINE_BUFFER_BYTES
                and overall_gain_pct >= MIN_OVERALL_GAIN_PCT
                and worst_case_pct >= -MAX_CASE_REGRESSION_PCT
                and max_rss_delta_kib <= MAX_RSS_INCREASE_KIB
            )
            candidate_metrics[buffer] = (
                overall_gain_pct,
                worst_case_pct,
                max_rss_delta_kib,
                qualified,
            )
            candidate_rows.append(
                {
                    "buffer_kind": kind,
                    "buffer_bytes": buffer,
                    "overall_geomean_throughput_delta_pct": f"{overall_gain_pct:.3f}",
                    "worst_case_throughput_delta_pct": f"{worst_case_pct:.3f}",
                    "max_median_peak_rss_delta_kib": f"{max_rss_delta_kib:.1f}",
                    "qualified": str(qualified).lower(),
                }
            )

        qualified = [buffer for buffer in throughput_buffers if candidate_metrics[buffer][3]]
        if qualified:
            selected = max(
                qualified,
                key=lambda buffer: (
                    candidate_metrics[buffer][0],
                    -candidate_metrics[buffer][2],
                    -buffer,
                ),
            )
            reason = "candidate met all predeclared throughput and peak-RSS thresholds"
        else:
            selected = BASELINE_BUFFER_BYTES
            reason = "no candidate met all predeclared throughput and peak-RSS thresholds"

        selected_metrics = candidate_metrics[selected]
        decision_rows.append(
            {
                "buffer_kind": kind,
                "baseline_buffer_bytes": BASELINE_BUFFER_BYTES,
                "selected_buffer_bytes": selected,
                "min_overall_gain_pct": f"{MIN_OVERALL_GAIN_PCT:.1f}",
                "max_case_regression_pct": f"{MAX_CASE_REGRESSION_PCT:.1f}",
                "max_rss_increase_kib": f"{MAX_RSS_INCREASE_KIB:.1f}",
                "selected_overall_gain_pct": f"{selected_metrics[0]:.3f}",
                "selected_worst_case_pct": f"{selected_metrics[1]:.3f}",
                "selected_max_rss_delta_kib": f"{selected_metrics[2]:.1f}",
                "reason": reason,
            }
        )
        print(f"{kind}.selected_buffer_bytes={selected}")
        print(f"{kind}.reason={reason}")

    write_tsv(
        results_dir / "case-summary.tsv",
        [
            "buffer_kind",
            "buffer_bytes",
            "scenario",
            "compression",
            "throughput_samples",
            "median_units_per_second",
            "baseline_median_units_per_second",
            "throughput_delta_pct",
            "rss_samples",
            "median_peak_rss_kib",
            "baseline_median_peak_rss_kib",
            "peak_rss_delta_kib",
        ],
        case_rows,
    )
    write_tsv(
        results_dir / "candidate-summary.tsv",
        [
            "buffer_kind",
            "buffer_bytes",
            "overall_geomean_throughput_delta_pct",
            "worst_case_throughput_delta_pct",
            "max_median_peak_rss_delta_kib",
            "qualified",
        ],
        candidate_rows,
    )
    write_tsv(
        results_dir / "decision.tsv",
        [
            "buffer_kind",
            "baseline_buffer_bytes",
            "selected_buffer_bytes",
            "min_overall_gain_pct",
            "max_case_regression_pct",
            "max_rss_increase_kib",
            "selected_overall_gain_pct",
            "selected_worst_case_pct",
            "selected_max_rss_delta_kib",
            "reason",
        ],
        decision_rows,
    )


if __name__ == "__main__":
    main()
