use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const SUCCESS_EXIT: i32 = 0;
const NO_MATCH_EXIT: i32 = 1;
const FAILURE_EXIT: i32 = 2;
const SUPPORTED_FIXTURE_ROWS: &str = "7";
const SUPPORTED_FIXTURE_BYTES: &str = "268";
const DEFAULT_ROW_BUDGET: usize = 100_000;
const ONE_OVER_DEFAULT_ROW_BUDGET: usize = DEFAULT_ROW_BUDGET + 1;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn find_help_exposes_finite_defaults_and_explicit_unlimited_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .args(["find", "--help"])
        .output()
        .expect("pgdumpx must execute");

    assert_eq!(output.status.code(), Some(SUCCESS_EXIT));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("help must be UTF-8");
    assert!(stdout.contains("--max-rows"));
    assert!(stdout.contains("100000"));
    assert!(stdout.contains("--max-decompressed-bytes"));
    assert!(stdout.contains("67108864"));
    assert!(stdout.contains("--unlimited"));
}

#[test]
fn supported_fixtures_reproduce_the_row_and_parser_consumed_byte_evidence() {
    for fixture_name in [
        "pg18-none-copy-basic.dump",
        "pg18-gzip-copy-basic.dump",
        "pg18-lz4-copy-basic.dump",
        "pg18-zstd-copy-basic.dump",
    ] {
        let fixture = fixture_path(fixture_name);
        let exact = run_find(
            &fixture,
            &[
                "--max-rows",
                SUPPORTED_FIXTURE_ROWS,
                "--max-decompressed-bytes",
                SUPPORTED_FIXTURE_BYTES,
            ],
            "NOT-PRESENT",
        );
        assert_clean_no_match(exact, fixture_name);

        let one_under = run_find(
            &fixture,
            &[
                "--max-rows",
                SUPPORTED_FIXTURE_ROWS,
                "--max-decompressed-bytes",
                "267",
            ],
            "NOT-PRESENT",
        );
        assert_resource_failure(one_under, "exceeding limit 267");
    }
}

#[test]
fn default_match_and_no_match_within_budget_keep_exit_zero_and_one() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    let matched = run_find(&fixture, &[], "EARLY-100");
    assert_eq!(matched.status.code(), Some(SUCCESS_EXIT));
    assert_eq!(matched.stdout, b"1\tEARLY-100\tcustomer-a\tplain\t\n");
    assert!(matched.stderr.is_empty());

    assert_clean_no_match(run_find(&fixture, &[], "NOT-PRESENT"), "none fixture");
}

#[test]
fn default_row_budget_stops_a_no_match_scan_and_explicit_modes_are_authoritative() {
    let archive = TempArchive::new(&archive_with_rows(ONE_OVER_DEFAULT_ROW_BUDGET));

    let default_exhausted = run_find(archive.path(), &[], "missing");
    assert_resource_failure(default_exhausted, "exceeding limit 100000");

    let bytes_only = run_find(
        archive.path(),
        &["--max-decompressed-bytes", "1000000"],
        "missing",
    );
    assert_resource_failure(bytes_only, "exceeding limit 100000");

    let rows_only = run_find(
        archive.path(),
        &["--max-rows", "100001"],
        "missing",
    );
    assert_clean_no_match(rows_only, "row override");

    let unlimited = run_find(archive.path(), &["--unlimited"], "missing");
    assert_clean_no_match(unlimited, "explicit unlimited");
}

#[test]
fn unlimited_conflicts_and_duplicates_are_usage_errors() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let cases: &[&[&str]] = &[
        &["--unlimited", "--max-rows", "1"],
        &["--max-rows", "1", "--unlimited"],
        &["--unlimited", "--max-decompressed-bytes", "1"],
        &["--max-decompressed-bytes", "1", "--unlimited"],
        &["--unlimited", "--unlimited"],
    ];

    for options in cases {
        let output = run_find(&fixture, options, "EARLY-100");
        assert_eq!(
            output.status.code(),
            Some(FAILURE_EXIT),
            "options={options:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

fn run_find(path: &Path, options: &[&str], value: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pgdumpx"));
    command.arg("find").args(options);
    command
        .arg(path)
        .arg("public.data")
        .arg("value")
        .arg(value)
        .output()
        .expect("pgdumpx must execute")
}

fn assert_clean_no_match(output: Output, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(NO_MATCH_EXIT),
        "context={context}; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

fn assert_resource_failure(output: Output, diagnostic: &str) {
    assert_eq!(output.status.code(), Some(FAILURE_EXIT));
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

struct TempArchive {
    path: PathBuf,
}

impl TempArchive {
    fn new(bytes: &[u8]) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pgdumpx-find-default-limits-{}-{id}.dump",
            std::process::id()
        ));
        fs::write(&path, bytes).expect("temporary archive must be writable");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempArchive {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn archive_with_rows(row_count: usize) -> Vec<u8> {
    let mut payload = Vec::with_capacity(row_count.saturating_mul(2).saturating_add(3));
    for _ in 0..row_count {
        payload.extend_from_slice(b"v\n");
    }
    payload.extend_from_slice(b"\\.\n");

    let mut bytes = complete_header();
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes);

    let offset = u64::try_from(bytes.len()).expect("archive length must fit u64");
    let offset_start = bytes.len() - 8;
    bytes[offset_start..offset_start + 8].copy_from_slice(&offset.to_le_bytes());

    bytes.push(BLK_DATA);
    write_int(&mut bytes, 2);
    write_int(
        &mut bytes,
        i32::try_from(payload.len()).expect("test payload must fit i32"),
    );
    bytes.extend_from_slice(&payload);
    write_int(&mut bytes, 0);
    bytes
}

fn write_table_entry(bytes: &mut Vec<u8>) {
    write_int(bytes, 1);
    write_int(bytes, 0);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE"));
    write_int(bytes, 2);
    write_string(
        bytes,
        Some(b"CREATE TABLE public.data (value text);\n"),
    );
    write_string(bytes, Some(b"DROP TABLE public.data;\n"));
    write_string(bytes, None);
    write_string(bytes, Some(b"public"));
    write_string(bytes, None);
    write_string(bytes, Some(b"heap"));
    write_int(bytes, 0);
    write_string(bytes, Some(b"postgres"));
    write_string(bytes, Some(b"false"));
    write_string(bytes, None);
    bytes.push(NO_DATA);
    bytes.extend_from_slice(&[0; 8]);
}

fn write_table_data_entry(bytes: &mut Vec<u8>) {
    write_int(bytes, 2);
    write_int(bytes, 1);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE DATA"));
    write_int(bytes, 3);
    write_string(bytes, None);
    write_string(bytes, None);
    write_string(bytes, Some(b"COPY public.data (value) FROM stdin;\n"));
    write_string(bytes, Some(b"public"));
    write_string(bytes, None);
    write_string(bytes, None);
    write_int(bytes, 0);
    write_string(bytes, Some(b"postgres"));
    write_string(bytes, Some(b"false"));
    write_string(bytes, Some(b"1"));
    write_string(bytes, None);
    bytes.push(POSITION_SET);
    bytes.extend_from_slice(&[0; 8]);
}

fn complete_header() -> Vec<u8> {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 16, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
    bytes
}

fn write_int(output: &mut Vec<u8>, value: i32) {
    output.push(u8::from(value.is_negative()));
    output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
}

fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_int(
                output,
                i32::try_from(bytes.len()).expect("test string must fit i32"),
            );
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}
