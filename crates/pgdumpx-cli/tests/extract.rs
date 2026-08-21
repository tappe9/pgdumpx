use std::{
    fs,
    path::{Path, PathBuf},
    process::{self, Command, Output},
    sync::atomic::{AtomicU64, Ordering},
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
const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

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
fn extract_passes_invalid_utf8_bytes_to_stdout_unchanged() {
    let payload = [0xff, 0x00, 0x80, b'\n'];
    let archive = TempArchive::new(archive_with_table_data(&payload));
    let output = run_extract(archive.path(), &[], "public.orders");

    assert_success(output, &payload);
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

    assert_runtime_failure(output, "requested table was not found");
}

#[test]
fn table_without_data_is_runtime_failure_not_clean_eof() {
    let archive = TempArchive::new(build_archive(1, |output| {
        write_table_entry(output, 1, b"public", b"orders", b"41");
    }));
    let output = run_extract(archive.path(), &[], "public.orders");

    assert_runtime_failure(output, "has no related TABLE DATA entry");
}

#[test]
fn malformed_table_data_relationship_is_runtime_failure_not_clean_eof() {
    let archive = TempArchive::new(build_archive(2, |output| {
        write_table_entry(output, 1, b"public", b"orders", b"41");
        write_table_data_entry(output, 2, b"public", b"orders", b"42", 1, 0);
    }));
    let output = run_extract(archive.path(), &[], "public.orders");

    assert_runtime_failure(output, "conflicts with TABLE DATA dump ID 2");
}

#[test]
fn malformed_archive_is_runtime_failure_not_clean_eof() {
    let archive = TempArchive::new(b"not-a-pgdump".to_vec());
    let output = run_extract(archive.path(), &[], "public.orders");

    assert_runtime_failure(output, "invalid archive magic");
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

fn assert_runtime_failure(output: Output, expected_stderr: &str) {
    assert_eq!(output.status.code(), Some(USAGE_EXIT));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected_stderr), "stderr={stderr:?}");
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
    fn new(bytes: Vec<u8>) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pgdumpx-extract-{}-{id}.dump",
            process::id()
        ));
        fs::write(&path, bytes).unwrap();
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

fn archive_with_table_data(payload: &[u8]) -> Vec<u8> {
    let mut data_offset_start = None;
    let mut output = complete_header();
    write_int(&mut output, 2);
    write_table_entry(&mut output, 1, b"public", b"orders", b"41");
    write_table_data_entry(
        &mut output,
        2,
        b"public",
        b"orders",
        b"41",
        1,
        0,
    );

    // The data offset is the final eight bytes of the TABLE DATA TOC entry.
    data_offset_start = Some(output.len() - 8);
    let data_offset = u64::try_from(output.len()).unwrap();
    let start = data_offset_start.unwrap();
    output[start..start + 8].copy_from_slice(&data_offset.to_le_bytes());
    output.extend_from_slice(&data_block(2, payload));
    output
}

fn build_archive(entry_count: i32, write_entries: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut output = complete_header();
    write_int(&mut output, entry_count);
    write_entries(&mut output);
    output
}

fn complete_header() -> Vec<u8> {
    let mut output = b"PGDMP".to_vec();
    output.extend_from_slice(&[1, 16, 0]);
    output.push(4);
    output.push(8);
    output.push(1);
    output.push(0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut output, value);
    }
    write_string(&mut output, Some(b"database"));
    write_string(&mut output, Some(b"18.4"));
    write_string(&mut output, Some(b"18.4"));
    output
}

fn write_table_entry(output: &mut Vec<u8>, id: i32, schema: &[u8], name: &[u8], oid: &[u8]) {
    write_int(output, id);
    write_int(output, 0);
    write_string(output, Some(b"1259"));
    write_string(output, Some(oid));
    write_string(output, Some(name));
    write_string(output, Some(b"TABLE"));
    write_int(output, SECTION_PRE_DATA);
    write_string(output, Some(b"CREATE TABLE"));
    write_string(output, Some(b"DROP TABLE"));
    write_string(output, None);
    write_string(output, Some(schema));
    write_string(output, Some(b""));
    write_string(output, Some(b"heap"));
    write_int(output, i32::from(b'r'));
    write_string(output, Some(b"postgres"));
    write_string(output, Some(b"false"));
    write_dependencies(output, &[]);
    output.push(NO_DATA);
    output.extend_from_slice(&0_u64.to_le_bytes());
}

fn write_table_data_entry(
    output: &mut Vec<u8>,
    id: i32,
    schema: &[u8],
    name: &[u8],
    oid: &[u8],
    table_id: i32,
    offset: u64,
) {
    write_int(output, id);
    write_int(output, 1);
    write_string(output, Some(b"0"));
    write_string(output, Some(oid));
    write_string(output, Some(name));
    write_string(output, Some(b"TABLE DATA"));
    write_int(output, SECTION_DATA);
    write_string(output, None);
    write_string(output, None);
    write_string(
        output,
        Some(b"COPY public.orders (value) FROM stdin;\n"),
    );
    write_string(output, Some(schema));
    write_string(output, None);
    write_string(output, None);
    write_int(output, 0);
    write_string(output, Some(b"postgres"));
    write_string(output, Some(b"false"));
    write_dependencies(output, &[table_id]);
    output.push(POSITION_SET);
    output.extend_from_slice(&offset.to_le_bytes());
}

fn write_dependencies(output: &mut Vec<u8>, dependencies: &[i32]) {
    for dependency in dependencies {
        let dependency = dependency.to_string();
        write_string(output, Some(dependency.as_bytes()));
    }
    write_string(output, None);
}

fn data_block(dump_id: i32, payload: &[u8]) -> Vec<u8> {
    let mut output = vec![BLK_DATA];
    write_int(&mut output, dump_id);
    write_int(&mut output, i32::try_from(payload.len()).unwrap());
    output.extend_from_slice(payload);
    write_int(&mut output, 0);
    output
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
