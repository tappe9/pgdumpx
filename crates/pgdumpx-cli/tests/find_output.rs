use std::{
    path::PathBuf,
    process::{Command, Output},
};

#[test]
fn official_fixture_normalizes_logical_line_and_carriage_returns() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    assert_match(
        run_find(&fixture, "THIRD-300"),
        b"3\tTHIRD-300\tcustomer-c\tline1\\nline2\tfilled\n",
    );
    assert_match(
        run_find(&fixture, "SIXTH-600"),
        b"6\tSIXTH-600\trepeat\tcarriage\\rreturn\tfilled\n",
    );
}

fn run_find(path: &PathBuf, value: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg("find")
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

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}
