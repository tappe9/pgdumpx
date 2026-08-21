use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const USAGE_EXIT: i32 = 2;
const SECOND_ROW: &[u8] = b"2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n";
const SECOND_ROW_END: u64 = 68;

#[test]
fn explicit_row_limit_allows_exact_and_above_boundary_match() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    for max_rows in [2, 3] {
        assert_match(
            run_find(
                &fixture,
                &["--max-rows", &max_rows.to_string()],
                "SECOND-200",
            ),
            SECOND_ROW,
        );
    }
}

#[test]
fn row_limit_prevents_the_next_row_from_being_evaluated() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_find(&fixture, &["--max-rows", "1"], "SECOND-200");

    assert_resource_failure(
        output,
        "row scan reached row 2 and 2 complete rows, exceeding limit 1",
    );
}

#[test]
fn explicit_byte_limit_allows_exact_and_above_boundary_match() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    for max_bytes in [SECOND_ROW_END, SECOND_ROW_END + 1] {
        assert_match(
            run_find(
                &fixture,
                &["--max-decompressed-bytes", &max_bytes.to_string()],
                "SECOND-200",
            ),
            SECOND_ROW,
        );
    }
}

#[test]
fn byte_limit_rejects_a_row_that_crosses_the_parser_consumed_budget() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_find(
        &fixture,
        &["--max-decompressed-bytes", "67"],
        "SECOND-200",
    );

    assert_resource_failure(
        output,
        "row scan reached 68 decompressed bytes in row 2 at offset 68, exceeding limit 67",
    );
}

#[test]
fn clean_no_match_remains_exit_one_but_limit_exhaustion_is_failure() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    let no_match = run_find(&fixture, &[], "NOT-PRESENT");
    assert_eq!(no_match.status.code(), Some(1));
    assert!(no_match.stdout.is_empty());
    assert!(no_match.stderr.is_empty());

    let exhausted = run_find(&fixture, &["--max-rows", "1"], "NOT-PRESENT");
    assert_resource_failure(exhausted, "exceeding limit 1");
}

#[test]
fn malformed_zero_overflowing_duplicate_and_unknown_options_are_usage_errors() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let cases: &[&[&str]] = &[
        &["--max-rows", "0"],
        &["--max-rows", "not-a-number"],
        &["--max-rows", "18446744073709551616"],
        &["--max-decompressed-bytes", "0"],
        &["--max-decompressed-bytes", "not-a-number"],
        &[
            "--max-decompressed-bytes",
            "18446744073709551616",
        ],
        &["--max-rows", "1", "--max-rows", "2"],
        &["--unknown-limit", "1"],
    ];

    for options in cases {
        let output = run_find(&fixture, options, "EARLY-100");
        assert_eq!(
            output.status.code(),
            Some(USAGE_EXIT),
            "options={options:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn both_limits_are_delegated_to_the_same_search_operation() {
    let fixture = fixture_path("pg18-gzip-copy-basic.dump");
    let output = run_find(
        &fixture,
        &[
            "--max-rows",
            "2",
            "--max-decompressed-bytes",
            "68",
        ],
        "SECOND-200",
    );

    assert_match(output, SECOND_ROW);
}

fn run_find(path: &Path, options: &[&str], value: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pgdumpx"));
    command.arg("find").args(options);
    command
        .arg(path)
        .arg("public.orders")
        .arg("order_number")
        .arg(value)
        .output()
        .expect("pgdumpx must execute")
}

fn assert_match(output: Output, expected_stdout: &[u8]) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
}

fn assert_resource_failure(output: Output, diagnostic: &str) {
    let code = output.status.code().expect("process must exit normally");
    assert!(code >= USAGE_EXIT, "unexpected exit {code}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(diagnostic),
        "stderr {stderr:?} did not contain {diagnostic:?}"
    );
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}
