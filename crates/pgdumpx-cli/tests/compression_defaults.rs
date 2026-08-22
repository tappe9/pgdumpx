use std::{
    path::PathBuf,
    process::{Command, Output},
};

const FIXTURES: [&str; 4] = [
    "pg18-none-copy-basic.dump",
    "pg18-gzip-copy-basic.dump",
    "pg18-lz4-copy-basic.dump",
    "pg18-zstd-copy-basic.dump",
];

#[test]
fn default_cli_extracts_all_v01_compression_backends() {
    let baseline = run_extract(FIXTURES[0]);
    assert_success(FIXTURES[0], &baseline);

    for fixture in FIXTURES.iter().skip(1).copied() {
        let output = run_extract(fixture);
        assert_success(fixture, &output);
        assert_eq!(
            output.stdout, baseline.stdout,
            "fixture {fixture} decoded to different table-data bytes"
        );
    }
}

fn run_extract(fixture: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg("extract")
        .arg(fixture_path(fixture))
        .arg("public.orders")
        .output()
        .expect("pgdumpx must execute")
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}

fn assert_success(fixture: &str, output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "fixture={fixture}; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "fixture={fixture}");
    assert!(!output.stdout.is_empty(), "fixture={fixture}");
}
