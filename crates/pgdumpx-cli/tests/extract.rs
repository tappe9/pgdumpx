use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const USAGE_EXIT: i32 = 2;
const EXPECTED_COPY_STREAM: &[u8] = b"1\tEARLY-100\tcustomer-a\tplain\t\n\
2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n\
3\tTHIRD-300\tcustomer-c\tline1\\nline2\tfilled\n\
4\tMIDDLE-400\tcustomer-d\t\\N\tfilled\n\
5\tFIFTH-500\tcustomer-e\t\tfilled\n\
6\tSIXTH-600\trepeat\tcarriage\\rreturn\tfilled\n\
7\tLATE-700\tcustomer-g\tbackslash\\\\value\tfilled\n\
\\.\n\n\n";
const STREAM_BYTES: u64 = 270;

#[test]
fn extract_streams_only_selected_table_data_for_none_and_gzip() {
    for fixture in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let output = run_extract(&fixture_path(fixture), &[], "public.orders");
        assert_success(output, EXPECTED_COPY_STREAM);
    }
}

#[test]
fn explicit_limit_allows_exact_and_above_boundaries() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    for limit in [STREAM_BYTES, STREAM_BYTES + 1] {
        let output = run_extract(
            &fixture,
            &["--max-decompressed-bytes", &limit.to_string()],
            "public.orders",
        );
        assert_success(output, EXPECTED_COPY_STREAM);
    }
}

#[test]
fn limit_exhaustion_is_failure_after_partial_binary_stdout() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_extract(
        &fixture,
        &["--max-decompressed-bytes", "269"],
        "public.orders",
    );

    let code = output.status.code().expect("process must exit normally");
    assert!(code >= USAGE_EXIT, "unexpected exit {code}");
    assert_eq!(output.stdout, EXPECTED_COPY_STREAM[..269]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("exceeding limit 269"), "stderr={stderr:?}");
}

#[test]
fn omitted_limit_is_finite_and_documented_in_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg("extract")
        .output()
        .expect("pgdumpx must execute");

    assert_eq!(output.status.code(), Some(USAGE_EXIT));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1073741824"), "stderr={stderr:?}");
    assert!(stderr.contains("1 GiB"), "stderr={stderr:?}");
}

#[test]
fn malformed_zero_overflowing_duplicate_and_unknown_options_are_usage_errors() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let cases: &[&[&str]] = &[
        &["--max-decompressed-bytes"],
        &["--max-decompressed-bytes", "0"],
        &["--max-decompressed-bytes", "-1"],
        &["--max-decompressed-bytes", "not-a-number"],
        &["--max-decompressed-bytes", "18446744073709551616"],
        &[
            "--max-decompressed-bytes",
            "1",
            "--max-decompressed-bytes",
            "2",
        ],
        &["--unknown-limit", "1"],
    ];

    for options in cases {
        let output = run_extract(&fixture, options, "public.orders");
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
fn extract_reuses_exact_schema_table_selector_grammar() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    for selector in [
        "orders",
        ".orders",
        "public.",
        "public.orders.extra",
        "\"public\".orders",
    ] {
        let output = run_extract(&fixture, &[], selector);
        assert_eq!(
            output.status.code(),
            Some(USAGE_EXIT),
            "selector={selector:?}"
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn missing_table_is_runtime_failure_not_clean_eof() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_extract(&fixture, &[], "public.missing");

    assert_eq!(output.status.code(), Some(USAGE_EXIT));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requested table was not found"),
        "stderr={stderr:?}"
    );
}

fn run_extract(path: &Path, options: &[&str], selector: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pgdumpx"));
    command.arg("extract").args(options).arg(path).arg(selector);
    command.output().expect("pgdumpx must execute")
}

fn assert_success(output: Output, expected_stdout: &[u8]) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}
