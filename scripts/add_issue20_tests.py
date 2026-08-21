from pathlib import Path

root = Path(__file__).resolve().parents[1]
path = root / "crates/pgdumpx-cli/tests/metadata.rs"
path.write_text(
    r'''use pgdumpx::Archive;
use std::{
    cell::Cell,
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Output},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const EXPECTED_LIST: &str = concat!(
    "DUMP_ID\tOBJECT_TYPE\tSCHEMA\tNAME\n",
    "1\tENCODING\t-\tENCODING\n",
    "2\tSTDSTRINGS\t-\tSTDSTRINGS\n",
    "3\tSEARCHPATH\t-\tSEARCHPATH\n",
    "4\tDATABASE\t-\tpgdumpx_fixture\n",
    "219\tTABLE\tpublic\torders\n",
    "220\tTABLE DATA\tpublic\torders\n",
    "3195\tCONSTRAINT\tpublic\torders orders_pkey\n",
);

#[test]
fn inspect_has_exact_deterministic_output_for_official_none_and_gzip_fixtures() {
    for (fixture_name, compression) in [
        ("pg18-none-copy-basic.dump", "none"),
        ("pg18-gzip-copy-basic.dump", "gzip"),
    ] {
        let expected = format!(
            "archive_version: 1.16.0\ncompression: {compression}\nentries: 7\ntables: 1\ntable_data: 1\n"
        );
        let first = run(&["inspect", fixture(fixture_name).to_str().unwrap()]);
        let second = run(&["inspect", fixture(fixture_name).to_str().unwrap()]);

        assert_success(&first);
        assert_success(&second);
        assert_eq!(first.stdout, expected.as_bytes());
        assert_eq!(second.stdout, first.stdout);
        assert!(first.stderr.is_empty());
        assert!(second.stderr.is_empty());
    }
}

#[test]
fn list_has_exact_toc_order_for_official_none_and_gzip_fixtures() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let first = run(&["list", fixture(fixture_name).to_str().unwrap()]);
        let second = run(&["list", fixture(fixture_name).to_str().unwrap()]);

        assert_success(&first);
        assert_success(&second);
        assert_eq!(first.stdout, EXPECTED_LIST.as_bytes());
        assert_eq!(second.stdout, first.stdout);
        assert!(first.stderr.is_empty());
        assert!(second.stderr.is_empty());
    }
}

#[test]
fn malformed_archive_uses_stderr_and_keeps_stdout_empty() {
    for command in ["inspect", "list"] {
        let path = temp_path("malformed");
        fs::write(&path, b"not a PostgreSQL custom archive").unwrap();
        let output = run(&[command, path.to_str().unwrap()]);
        let _ = fs::remove_file(path);

        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("pgdumpx: "));
    }
}

#[test]
fn metadata_commands_neither_seek_to_nor_require_table_data_payloads() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let bytes = fs::read(fixture(fixture_name)).unwrap();
        let bytes_read = Rc::new(Cell::new(0_usize));
        let seeks = Rc::new(Cell::new(0_usize));
        let reader = TrackingReader::new(bytes.clone(), Rc::clone(&bytes_read), Rc::clone(&seeks));
        Archive::open(reader).unwrap();
        assert_eq!(seeks.get(), 0, "Archive::open must remain metadata-only");

        let metadata_len = bytes_read.get();
        assert!(metadata_len < bytes.len(), "official fixture must contain a payload");
        let path = temp_path(fixture_name);
        fs::write(&path, &bytes[..metadata_len]).unwrap();

        for command in ["inspect", "list"] {
            let output = run(&[command, path.to_str().unwrap()]);
            assert_success(&output);
            assert!(output.stderr.is_empty());
        }
        let _ = fs::remove_file(path);
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .args(args)
        .output()
        .unwrap()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}

fn temp_path(label: impl AsRef<Path>) -> PathBuf {
    let label = label
        .as_ref()
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pgdumpx-{label}-{}-{sequence}.dump",
        std::process::id()
    ))
}

#[derive(Debug)]
struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<usize>>,
    seeks: Rc<Cell<usize>>,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>, bytes_read: Rc<Cell<usize>>, seeks: Rc<Cell<usize>>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read,
            seeks,
        }
    }
}

impl Read for TrackingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(output)?;
        self.bytes_read.set(self.bytes_read.get() + read);
        Ok(read)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.seeks.set(self.seeks.get() + 1);
        self.inner.seek(position)
    }
}
''',
    encoding="utf-8",
)
