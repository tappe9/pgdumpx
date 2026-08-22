use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

const MANIFEST_PATH: &str = "tests/fixtures/manifest.toml";
const REQUIRED_FIXTURES: [FixtureExpectation; 9] = [
    FixtureExpectation::new("pg18-none-copy-basic", "1.16.0", "none", "copy"),
    FixtureExpectation::new("pg18-gzip-copy-basic", "1.16.0", "gzip", "copy"),
    FixtureExpectation::new("pg18-lz4-copy-basic", "1.16.0", "lz4", "copy"),
    FixtureExpectation::new("pg18-zstd-copy-basic", "1.16.0", "zstd", "copy"),
    FixtureExpectation::new("pg18-none-insert-basic", "1.16.0", "none", "insert"),
    FixtureExpectation::new("pg16-none-copy-basic", "1.15.0", "none", "copy"),
    FixtureExpectation::new("pg16-gzip-copy-basic", "1.15.0", "gzip", "copy"),
    FixtureExpectation::new("pg15-none-copy-basic", "1.14.0", "none", "copy"),
    FixtureExpectation::new("pg15-gzip-copy-basic", "1.14.0", "gzip", "copy"),
];
const EXPECTED_COLUMNS: [&str; 5] = [
    "order_id",
    "order_number",
    "customer_code",
    "note",
    "empty_text",
];

#[derive(Debug, Clone, Copy)]
struct FixtureExpectation {
    name: &'static str,
    version: &'static str,
    compression: &'static str,
    representation: &'static str,
}

impl FixtureExpectation {
    const fn new(
        name: &'static str,
        version: &'static str,
        compression: &'static str,
        representation: &'static str,
    ) -> Self {
        Self {
            name,
            version,
            compression,
            representation,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FixtureManifest {
    manifest_version: u32,
    fixture: Vec<FixtureRecord>,
}

#[derive(Debug, Deserialize)]
struct FixtureRecord {
    name: String,
    path: String,
    source: String,
    archive_version: String,
    generator: String,
    generator_image: String,
    generator_image_digest: String,
    generator_platform: String,
    command: String,
    compression: String,
    compression_detail: String,
    sha256: String,
    purpose: Vec<String>,
    expected_tables: Vec<String>,
    expected_row_count: usize,
    expected_columns: Vec<String>,
}

#[test]
fn official_fixture_manifest_contract_is_satisfied() {
    let root = repository_root();

    assert!(
        root.join("tests/fixtures/README.md").is_file(),
        "fixture inventory and regeneration documentation is missing"
    );
    for script in [
        "scripts/generate-alpha1-fixtures.sh",
        "scripts/generate-alpha3-version-fixtures.sh",
        "scripts/generate-alpha3-compression-fixture.sh",
        "scripts/generate-alpha3-insert-fixture.sh",
    ] {
        assert!(
            root.join(script).is_file(),
            "fixture regeneration script is missing: {script}"
        );
    }

    let manifest = load_manifest(&root);
    assert_eq!(manifest.manifest_version, 1, "unsupported manifest version");

    let mut names = HashSet::new();
    let mut paths = HashSet::new();
    for fixture in &manifest.fixture {
        assert!(
            names.insert(fixture.name.as_str()),
            "duplicate fixture name: {}",
            fixture.name
        );
        assert!(
            paths.insert(fixture.path.as_str()),
            "duplicate fixture path: {}",
            fixture.path
        );
        validate_fixture(&root, fixture);
    }

    for expected in REQUIRED_FIXTURES {
        let fixture = manifest
            .fixture
            .iter()
            .find(|fixture| fixture.name == expected.name)
            .unwrap_or_else(|| panic!("required fixture entry missing: {}", expected.name));
        assert_eq!(fixture.archive_version, expected.version);
        assert_eq!(fixture.compression, expected.compression);
        assert_eq!(representation(fixture), expected.representation);
    }
}

#[test]
fn checksum_verification_rejects_modified_bytes() {
    let root = repository_root();
    let manifest = load_manifest(&root);
    let fixture = manifest
        .fixture
        .iter()
        .find(|fixture| fixture.name == REQUIRED_FIXTURES[0].name)
        .expect("none fixture must exist");
    let fixture_path = resolve_repository_file(&root, &fixture.path).expect("valid fixture path");
    let mut bytes = fs::read(fixture_path).expect("fixture bytes must be readable");

    assert!(!bytes.is_empty(), "official fixture must not be empty");
    assert_eq!(sha256_hex(&bytes), fixture.sha256);

    bytes[0] ^= 1;
    assert_ne!(
        sha256_hex(&bytes),
        fixture.sha256,
        "checksum verification must reject fixture-byte drift"
    );
}

#[test]
fn repository_paths_reject_escape_components() {
    let root = repository_root();

    assert!(resolve_repository_file(&root, "../outside.dump").is_err());
    assert!(resolve_repository_file(&root, "/tmp/outside.dump").is_err());
    assert!(resolve_repository_file(&root, "./tests/fixtures/manifest.toml").is_err());
}

fn validate_fixture(root: &Path, fixture: &FixtureRecord) {
    assert_non_empty("name", &fixture.name, &fixture.name);
    assert_non_empty("path", &fixture.path, &fixture.name);
    assert_non_empty("source", &fixture.source, &fixture.name);
    assert_non_empty("generator", &fixture.generator, &fixture.name);
    assert_non_empty("command", &fixture.command, &fixture.name);
    assert_non_empty("sha256", &fixture.sha256, &fixture.name);

    let (version_bytes, generator_prefix, image, version_purpose) =
        match fixture.archive_version.as_str() {
            "1.16.0" => (
                [1, 16, 0],
                "pg_dump (PostgreSQL) 18.4",
                "postgres:18.4-bookworm",
                None,
            ),
            "1.15.0" => (
                [1, 15, 0],
                "pg_dump (PostgreSQL) 16.15",
                "postgres:16.15-bookworm",
                Some("archive-1.15"),
            ),
            "1.14.0" => (
                [1, 14, 0],
                "pg_dump (PostgreSQL) 15.19",
                "postgres:15.19-bookworm",
                Some("archive-1.14"),
            ),
            other => panic!("{} has unexpected archive version {other:?}", fixture.name),
        };
    assert!(
        fixture.generator.starts_with(generator_prefix),
        "{} must record the expected pg_dump version output",
        fixture.name
    );
    assert_eq!(fixture.generator_image, image);
    assert_eq!(fixture.generator_platform, "linux/amd64");
    assert!(
        fixture
            .generator_image_digest
            .strip_prefix("postgres@sha256:")
            .is_some_and(is_lower_hex_sha256),
        "{} has an invalid generator image digest",
        fixture.name
    );
    assert!(
        is_lower_hex_sha256(&fixture.sha256),
        "{} has an invalid fixture SHA-256",
        fixture.name
    );

    validate_compression(fixture);
    assert!(fixture.command.contains("--format=custom"));
    assert!(fixture.command.contains("--dbname=pgdumpx_fixture"));
    assert!(fixture.command.contains("--table=public.orders"));
    assert_has_purpose(fixture, "header");
    assert_has_purpose(fixture, "toc");
    assert_has_purpose(fixture, &fixture.compression);

    match representation(fixture) {
        "copy" => {
            assert!(!fixture.command.contains("--inserts"));
            assert_has_purpose(fixture, "copy-text");
            if let Some(version_purpose) = version_purpose {
                assert_has_purpose(fixture, version_purpose);
                assert_has_purpose(fixture, "selected-entry");
            } else {
                assert_has_purpose(fixture, "column-layout");
                assert_has_purpose(fixture, "find-first");
            }
        }
        "insert" => {
            assert_eq!(fixture.archive_version, "1.16.0");
            assert!(fixture.command.contains("--inserts"));
            assert_has_purpose(fixture, "insert");
            assert_has_purpose(fixture, "row-api-rejection");
            assert_has_purpose(fixture, "selected-entry");
            assert_has_purpose(fixture, "differential");
        }
        other => panic!("{} has unexpected representation {other:?}", fixture.name),
    }

    if fixture.archive_version == "1.14.0" {
        assert_has_purpose(fixture, "legacy-compression");
    }

    assert!(
        fixture
            .expected_tables
            .iter()
            .any(|table| table == "public.orders"),
        "{} must record public.orders as an expected table",
        fixture.name
    );
    assert_eq!(fixture.expected_row_count, 7);
    let actual_columns: Vec<&str> = fixture
        .expected_columns
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(actual_columns, EXPECTED_COLUMNS);

    let source_path = resolve_repository_file(root, &fixture.source)
        .unwrap_or_else(|error| panic!("{} source path is invalid: {error}", fixture.name));
    assert_eq!(
        source_path,
        root.join("tests/fixtures/source/alpha1-copy-basic.sql")
            .canonicalize()
            .expect("source SQL must exist")
    );

    let fixture_path = resolve_repository_file(root, &fixture.path)
        .unwrap_or_else(|error| panic!("{} archive path is invalid: {error}", fixture.name));
    let bytes = fs::read(&fixture_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", fixture_path.display()));
    assert!(bytes.len() >= 8, "{} archive is truncated", fixture.name);
    assert_eq!(&bytes[..5], b"PGDMP", "{} has invalid magic", fixture.name);
    assert_eq!(
        &bytes[5..8],
        version_bytes.as_slice(),
        "{} archive-version bytes do not match its manifest record",
        fixture.name
    );
    assert_eq!(
        sha256_hex(&bytes),
        fixture.sha256,
        "{} checksum mismatch",
        fixture.name
    );
}

fn representation(fixture: &FixtureRecord) -> &'static str {
    let copy = fixture.purpose.iter().any(|purpose| purpose == "copy-text");
    let insert = fixture.purpose.iter().any(|purpose| purpose == "insert");
    match (copy, insert) {
        (true, false) => "copy",
        (false, true) => "insert",
        _ => panic!(
            "{} must declare exactly one table-data representation purpose",
            fixture.name
        ),
    }
}

fn validate_compression(fixture: &FixtureRecord) {
    match (
        fixture.archive_version.as_str(),
        fixture.compression.as_str(),
    ) {
        ("1.14.0", "none") => {
            assert!(fixture.command.contains("--compress=0"));
            assert_eq!(fixture.compression_detail, "legacy-level=0");
        }
        ("1.14.0", "gzip") => {
            assert!(fixture.command.contains("--compress=6"));
            assert_eq!(fixture.compression_detail, "legacy-level=6");
        }
        ("1.16.0", "lz4") => {
            assert!(fixture.command.contains("--compress=lz4:1"));
            assert_eq!(fixture.compression_detail, "level=1");
        }
        ("1.16.0", "zstd") => {
            assert!(fixture.command.contains("--compress=zstd:3"));
            assert_eq!(fixture.compression_detail, "level=3");
        }
        (_, "none") => {
            assert!(fixture.command.contains("--compress=none"));
            assert_eq!(fixture.compression_detail, "none");
        }
        (_, "gzip") => {
            assert!(fixture.command.contains("--compress=gzip:6"));
            assert_eq!(fixture.compression_detail, "level=6");
        }
        (_, other) => panic!("unexpected fixture compression {other:?}"),
    }
}

fn assert_has_purpose(fixture: &FixtureRecord, purpose: &str) {
    assert!(
        fixture.purpose.iter().any(|candidate| candidate == purpose),
        "{} is missing purpose {purpose}",
        fixture.name
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root must be resolvable")
}

fn load_manifest(root: &Path) -> FixtureManifest {
    let path = root.join(MANIFEST_PATH);
    let content = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "fixture manifest not found or unreadable at {}: {error}",
            path.display()
        )
    });
    toml::from_str(&content).unwrap_or_else(|error| {
        panic!(
            "fixture manifest at {} is invalid TOML: {error}",
            path.display()
        )
    })
}

fn resolve_repository_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty() {
        return Err("path is empty".to_owned());
    }
    if relative.is_absolute() {
        return Err(format!(
            "absolute path is forbidden: {}",
            relative.display()
        ));
    }
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "path contains a non-normal component: {}",
            relative.display()
        ));
    }

    let joined = root.join(relative);
    let canonical = joined
        .canonicalize()
        .map_err(|error| format!("{} cannot be resolved: {error}", joined.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "path escapes repository root: {}",
            canonical.display()
        ));
    }
    if !canonical.is_file() {
        return Err(format!("path is not a file: {}", canonical.display()));
    }

    Ok(canonical)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn assert_non_empty(field: &str, value: &str, fixture_name: &str) {
    assert!(
        !value.trim().is_empty(),
        "fixture {fixture_name:?} has an empty {field} field"
    );
}
