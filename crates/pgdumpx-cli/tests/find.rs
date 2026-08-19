use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const USAGE_EXIT: i32 = 2;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn official_none_fixture_finds_early_middle_and_late_rows_with_exact_output() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    assert_match(
        run_find(&fixture, "public.orders", "order_number", "EARLY-100"),
        b"1\tEARLY-100\tcustomer-a\tplain\t\n",
    );
    assert_match(
        run_find(&fixture, "public.orders", "order_number", "MIDDLE-400"),
        b"4\tMIDDLE-400\tcustomer-d\t\\N\tfilled\n",
    );
    assert_match(
        run_find(&fixture, "public.orders", "order_number", "LATE-700"),
        b"7\tLATE-700\tcustomer-g\tbackslash\\\\value\tfilled\n",
    );
}

#[test]
fn official_gzip_fixture_uses_the_same_end_to_end_find_path() {
    let fixture = fixture_path("pg18-gzip-copy-basic.dump");
    let output = run_find(&fixture, "public.orders", "order_number", "SECOND-200");

    assert_match(
        output,
        b"2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n",
    );
}

#[test]
fn no_match_is_exit_one_with_clean_stdout_and_stderr() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_find(
        &fixture,
        "public.orders",
        "order_number",
        "NOT-PRESENT",
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_table_and_column_are_runtime_failures_not_no_match() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    let unknown_table = run_find(&fixture, "public.missing", "order_number", "EARLY-100");
    assert_failure(unknown_table, "table");

    let unknown_column = run_find(&fixture, "public.orders", "missing", "EARLY-100");
    assert_failure(unknown_column, "column");
}

#[test]
fn malformed_archive_and_unsupported_representation_are_runtime_failures() {
    let malformed = TempArchive::new(b"not a PostgreSQL custom archive");
    let malformed_output = run_find(malformed.path(), "public.data", "value", "match");
    assert_failure(malformed_output, "archive");

    let unsupported = TempArchive::new(&archive_with_table_data(None, b"not COPY text"));
    let unsupported_output = run_find(unsupported.path(), "public.data", "value", "match");
    assert_failure(unsupported_output, "unsupported");
}

#[test]
fn unavailable_column_metadata_is_a_failure_not_no_match() {
    let archive = TempArchive::new(&archive_with_table_data(Some(b""), b"match\n\\.\n"));
    let output = run_find(archive.path(), "public.data", "value", "match");

    assert_failure(output, "metadata");
}

#[test]
fn matched_row_output_is_ascii_and_binary_safe_for_non_utf8_fields() {
    let archive = TempArchive::new(&archive_with_table_data(
        Some(b"COPY public.data (key, payload) FROM stdin;\n"),
        b"match\t\\377\n\\.\n",
    ));
    let output = run_find(archive.path(), "public.data", "key", "match");

    assert_match(output, b"match\t\\377\n");
}

#[test]
fn schema_table_grammar_accepts_exactly_one_separator_and_no_sql_quoting() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    assert_eq!(
        run_find(&fixture, "public.orders", "order_number", "EARLY-100")
            .status
            .code(),
        Some(0)
    );

    for invalid in [
        "publicorders",
        "public.orders.extra",
        ".orders",
        "public.",
        "\"public\".\"orders\"",
    ] {
        let output = run_find(&fixture, invalid, "order_number", "EARLY-100");
        assert_eq!(
            output.status.code(),
            Some(USAGE_EXIT),
            "selector {invalid:?} must be a usage failure; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn utf8_query_arguments_are_accepted_without_lossy_conversion() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = run_find(&fixture, "public.orders", "order_number", "未登録");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_query_argument_is_rejected_as_usage_error() {
    use std::os::unix::ffi::OsStringExt;

    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let output = Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg("find")
        .arg(fixture)
        .arg("public.orders")
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .arg("value")
        .output()
        .expect("pgdumpx must execute");

    assert_eq!(output.status.code(), Some(USAGE_EXIT));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn wrong_command_and_wrong_argument_count_are_usage_failures() {
    for args in [
        vec!["inspect"],
        vec!["find"],
        vec!["find", "only-a-file"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
            .args(args)
            .output()
            .expect("pgdumpx must execute");
        assert_eq!(output.status.code(), Some(USAGE_EXIT));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

fn run_find(path: &Path, table: &str, column: &str, value: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg("find")
        .arg(path)
        .arg(table)
        .arg(column)
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

fn assert_failure(output: Output, expected_diagnostic: &str) {
    let code = output.status.code().expect("process must exit normally");
    assert!(code >= USAGE_EXIT, "unexpected exit {code}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    assert!(
        stderr.contains(expected_diagnostic),
        "stderr {stderr:?} did not contain {expected_diagnostic:?}"
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
            "pgdumpx-cli-{}-{id}.dump",
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

fn archive_with_table_data(copy_statement: Option<&[u8]>, payload: &[u8]) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 2);
    write_table_entry(&mut bytes);
    write_table_data_entry(&mut bytes, copy_statement);

    let offset = u64::try_from(bytes.len()).unwrap();
    let offset_start = bytes.len() - 8;
    bytes[offset_start..offset_start + 8].copy_from_slice(&offset.to_le_bytes());

    bytes.push(BLK_DATA);
    write_int(&mut bytes, 2);
    write_int(&mut bytes, i32::try_from(payload.len()).unwrap());
    bytes.extend_from_slice(payload);
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
        Some(b"CREATE TABLE public.data (key text, payload bytea);\n"),
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

fn write_table_data_entry(bytes: &mut Vec<u8>, copy_statement: Option<&[u8]>) {
    write_int(bytes, 2);
    write_int(bytes, 1);
    write_string(bytes, Some(b"1259"));
    write_string(bytes, Some(b"16385"));
    write_string(bytes, Some(b"data"));
    write_string(bytes, Some(b"TABLE DATA"));
    write_int(bytes, 3);
    write_string(bytes, None);
    write_string(bytes, None);
    write_string(bytes, copy_statement);
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
            write_int(output, i32::try_from(bytes.len()).unwrap());
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}
