use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

const MANIFEST_PATH: &str = "tests/fixtures/manifest.toml";
const REQUIRED_FIXTURES: [(&str, &str); 2] = [
    ("pg18-none-copy-basic", "none"),
    ("pg18-gzip-copy-basic", "gzip"),
];
const EXPECTED_COLUMNS: [&str; 5] = [
    "order_id",
    "order_number",
    "customer_code",
    "note",
    "empty_text",
];
const REQUIRED_PURPOSES: [&str; 5] = [
    "header",
    "toc",
    "copy-text",
    "column-layout",
    "find-first",
];

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
fn alpha1_fixture_manifest_contract_is_satisfied() {
    let root = repository_root();

    assert!(
        root.join("tests/fixtures/README.md").is_file(),
        "fixture inventory and regeneration documentation is missing"
    );
    assert!(
        root.join("scripts/generate-alpha1-fixtures.sh").is_file(),
        "fixture regeneration script is missing"
    );

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

    for (required_name, required_compression) in REQUIRED_FIXTURES {
        let fixture = manifest
            .fixture
            .iter()
            .find(|fixture| fixture.name == required_name)
            .unwrap_or_else(|| panic!("required fixture entry missing: {required_name}"));
        assert_eq!(fixture.compression, required_compression);
    }
}

#[test]
fn checksum_verification_rejects_modified_bytes() {
    let root = repository_root();
    let manifest = load_manifest(&root);
    let fixture = manifest
        .fixture
        .iter()
        .find(|fixture| fixture.name == REQUIRED_FIXTURES[0].0)
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

    assert_eq!(fixture.archive_version, "1.16.0", "{}", fixture.name);
    assert!(
        fixture.generator.starts_with("pg_dump (PostgreSQL) 18.4"),
        "{} must record the exact PostgreSQL 18.4 pg_dump version output",
        fixture.name
    );
    assert_eq!(fixture.generator_image, "postgres:18.4-bookworm");
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

    let expected_compression_detail = match fixture.compression.as_str() {
        "none" => {
            assert!(fixture.command.contains("--compress=none"));
            "none"
        }
        "gzip" => {
            assert!(fixture.command.contains("--compress=gzip:6"));
            "level=6"
        }
        other => panic!("unexpected Alpha 1 compression {other:?}"),
    };
    assert_eq!(fixture.compression_detail, expected_compression_detail);
    assert!(fixture.command.contains("--format=custom"));
    assert!(fixture.command.contains("--dbname=pgdumpx_fixture"));
    assert!(fixture.command.contains("--table=public.orders"));

    for required_purpose in REQUIRED_PURPOSES {
        assert!(
            fixture
                .purpose
                .iter()
                .any(|purpose| purpose == required_purpose),
            "{} is missing purpose {required_purpose}",
            fixture.name
        );
    }
    assert!(
        fixture
            .purpose
            .iter()
            .any(|purpose| purpose == &fixture.compression),
        "{} is missing its compression purpose",
        fixture.name
    );
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
        &[1, 16, 0],
        "{} is not archive version 1.16.0",
        fixture.name
    );
    assert_eq!(
        sha256_hex(&bytes),
        fixture.sha256,
        "{} checksum mismatch",
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
        return Err(format!("absolute path is forbidden: {}", relative.display()));
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
