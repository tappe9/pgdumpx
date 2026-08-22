use pgdumpx::{Archive, DataLocation};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const FAILURE_EXIT: i32 = 2;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

const EXPECTED_NONE_INSPECT: &[u8] =
    b"archive_version=1.16.0\ncompression=none\nentries=7\ntables=1\ntable_data=1\n";
const EXPECTED_GZIP_INSPECT: &[u8] =
    b"archive_version=1.16.0\ncompression=gzip\nentries=7\ntables=1\ntable_data=1\n";
const EXPECTED_LIST: &[u8] = b"dump_id\tobject_type\tschema\tname\n3375\tENCODING\t-\tENCODING\n3376\tSTDSTRINGS\t-\tSTDSTRINGS\n3377\tSEARCHPATH\t-\tSEARCHPATH\n3378\tDATABASE\t-\tpgdumpx_fixture\n219\tTABLE\tpublic\torders\n3372\tTABLE DATA\tpublic\torders\n3224\tCONSTRAINT\tpublic\torders orders_pkey\n";

#[test]
fn inspect_has_exact_deterministic_output_for_official_none_and_gzip_fixtures() {
    let none = fixture_path("pg18-none-copy-basic.dump");
    let gzip = fixture_path("pg18-gzip-copy-basic.dump");

    assert_success(run("inspect", &none), EXPECTED_NONE_INSPECT);
    assert_success(run("inspect", &gzip), EXPECTED_GZIP_INSPECT);

    let first = run("inspect", &none);
    let second = run("inspect", &none);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
}

#[test]
fn list_preserves_toc_order_with_exact_output_for_official_none_and_gzip_fixtures() {
    for fixture in [
        fixture_path("pg18-none-copy-basic.dump"),
        fixture_path("pg18-gzip-copy-basic.dump"),
    ] {
        assert_success(run("list", &fixture), EXPECTED_LIST);
    }

    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let first = run("list", &fixture);
    let second = run("list", &fixture);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
}

#[test]
fn inspect_and_list_do_not_read_selected_entry_payload_bytes() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");
    let mut bytes = fs::read(&fixture).expect("fixture must be readable");
    let archive = Archive::open(Cursor::new(bytes.clone())).expect("fixture must open");
    let data_id = archive
        .table(b"public", b"orders")
        .and_then(|table| table.data_entry_id())
        .expect("orders TABLE DATA must exist");
    let offset = match archive
        .entry(data_id)
        .expect("TABLE DATA entry must resolve")
        .data_location()
    {
        DataLocation::Offset(offset) => usize::try_from(offset).expect("fixture offset fits usize"),
        other => panic!("expected recorded TABLE DATA offset, got {other:?}"),
    };
    let end = offset
        .checked_add(16)
        .expect("test corruption range must not overflow")
        .min(bytes.len());
    assert!(offset < end, "fixture TABLE DATA offset must be in range");
    bytes[offset..end].fill(0xff);

    let corrupted = TempArchive::new(&bytes);
    assert_success(run("inspect", corrupted.path()), EXPECTED_NONE_INSPECT);
    assert_success(run("list", corrupted.path()), EXPECTED_LIST);
}

#[test]
fn inspect_and_list_report_malformed_archives_only_on_stderr() {
    let malformed = TempArchive::new(b"not a PostgreSQL custom archive");

    for command in ["inspect", "list"] {
        let output = run(command, malformed.path());
        assert_eq!(output.status.code(), Some(FAILURE_EXIT));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        assert!(stderr.contains("archive"), "stderr={stderr:?}");
    }
}

#[test]
fn inspect_and_list_reject_extra_arguments_without_writing_stdout() {
    let fixture = fixture_path("pg18-none-copy-basic.dump");

    for command in ["inspect", "list"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
            .arg(command)
            .arg(&fixture)
            .arg("extra")
            .output()
            .expect("pgdumpx must execute");
        assert_eq!(output.status.code(), Some(FAILURE_EXIT));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

fn run(command: &str, path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .arg(command)
        .arg(path)
        .output()
        .expect("pgdumpx must execute")
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

struct TempArchive {
    path: PathBuf,
}

impl TempArchive {
    fn new(bytes: &[u8]) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pgdumpx-metadata-cli-{}-{id}.dump",
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
