#!/usr/bin/env python3
from __future__ import annotations

import csv
import math
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path

BASELINE_BUFFER_BYTES = 8192
MIN_OVERALL_GAIN_PCT = 5.0
MAX_CASE_REGRESSION_PCT = 3.0
MAX_RSS_INCREASE_KIB = 2048.0
EXPECTED_SCENARIOS = ("single", "multi")
EXPECTED_COMPRESSIONS = ("none", "gzip", "lz4", "zstd")
RSS_LABEL = re.compile(
    r"^buffer=(?P<buffer>\d+)/(?P<compression>[^/]+)/(?P<scenario>single|multi)/repetition=(?P<repetition>\d+)$"
)


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"buffer tuning summary failed: {message}")


def median(values: list[float], label: str) -> float:
    if not values:
        fail(f"missing samples for {label}")
    return float(statistics.median(values))


def read_throughput(path: Path):
    samples: dict[tuple[int, str, str], list[float]] = defaultdict(list)
    with path.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        required = {
            "buffer_bytes",
            "scenario",
            "compression",
            "units_per_second",
            "outcome",
        }
        if not reader.fieldnames or not required.issubset(reader.fieldnames):
            fail(f"{path} is missing required columns")
        for row in reader:
            if row["outcome"] != "success":
                fail(f"non-success throughput outcome: {row}")
            key = (int(row["buffer_bytes"]), row["scenario"], row["compression"])
            samples[key].append(float(row["units_per_second"]))
    return samples


def read_rss(path: Path):
    samples: dict[tuple[int, str, str], list[float]] = defaultdict(list)
    with path.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if not reader.fieldnames or not {"label", "peak_rss_kib"}.issubset(reader.fieldnames):
            fail(f"{path} is missing required columns")
        for row in reader:
            match = RSS_LABEL.fullmatch(row["label"])
            if match is None:
                fail(f"unexpected peak-RSS label: {row['label']!r}")
            key = (
                int(match.group("buffer")),
                match.group("scenario"),
                match.group("compression"),
            )
            samples[key].append(float(row["peak_rss_kib"]))
    return samples


def write_tsv(path: Path, fieldnames: list[str], rows: list[dict[str, object]]) -> None:
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, delimiter="\t", lineterminator="\n")
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
    buffers = sorted({buffer for buffer, _, _ in throughput})
    if BASELINE_BUFFER_BYTES not in buffers:
        fail(f"missing {BASELINE_BUFFER_BYTES}-byte baseline")
    if {buffer for buffer, _, _ in rss} != set(buffers):
        fail("throughput and peak-RSS candidate sets differ")

    expected_cases = {
        (scenario, compression)
        for scenario in EXPECTED_SCENARIOS
        for compression in EXPECTED_COMPRESSIONS
    }
    for buffer in buffers:
        throughput_cases = {(scenario, compression) for b, scenario, compression in throughput if b == buffer}
        rss_cases = {(scenario, compression) for b, scenario, compression in rss if b == buffer}
        if throughput_cases != expected_cases:
            fail(f"buffer {buffer} throughput cases differ from required single/multi x four-compression matrix")
        if rss_cases != expected_cases:
            fail(f"buffer {buffer} peak-RSS cases differ from required single/multi x four-compression matrix")

    case_rows: list[dict[str, object]] = []
    candidate_rows: list[dict[str, object]] = []
    candidate_metrics: dict[int, tuple[float, float, float, bool]] = {}

    for buffer in buffers:
        ratios: list[float] = []
        case_deltas: list[float] = []
        rss_deltas: list[float] = []
        for scenario in EXPECTED_SCENARIOS:
            for compression in EXPECTED_COMPRESSIONS:
                key = (buffer, scenario, compression)
                baseline_key = (BASELINE_BUFFER_BYTES, scenario, compression)
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
                "buffer_bytes": buffer,
                "overall_geomean_throughput_delta_pct": f"{overall_gain_pct:.3f}",
                "worst_case_throughput_delta_pct": f"{worst_case_pct:.3f}",
                "max_median_peak_rss_delta_kib": f"{max_rss_delta_kib:.1f}",
                "qualified": str(qualified).lower(),
            }
        )

    qualified = [buffer for buffer in buffers if candidate_metrics[buffer][3]]
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
    write_tsv(
        results_dir / "case-summary.tsv",
        [
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
        ["key", "value"],
        [
            {"key": "baseline_buffer_bytes", "value": BASELINE_BUFFER_BYTES},
            {"key": "selected_buffer_bytes", "value": selected},
            {"key": "min_overall_gain_pct", "value": f"{MIN_OVERALL_GAIN_PCT:.1f}"},
            {"key": "max_case_regression_pct", "value": f"{MAX_CASE_REGRESSION_PCT:.1f}"},
            {"key": "max_rss_increase_kib", "value": f"{MAX_RSS_INCREASE_KIB:.1f}"},
            {"key": "selected_overall_gain_pct", "value": f"{selected_metrics[0]:.3f}"},
            {"key": "selected_worst_case_pct", "value": f"{selected_metrics[1]:.3f}"},
            {"key": "selected_max_rss_delta_kib", "value": f"{selected_metrics[2]:.1f}"},
            {"key": "reason", "value": reason},
        ],
    )

    print(f"selected_buffer_bytes={selected}")
    print(f"reason={reason}")


if __name__ == "__main__":
    main()
