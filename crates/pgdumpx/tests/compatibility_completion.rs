use pgdumpx::{
    Archive, EntryReadLimits, PgDumpError, TableDataRepresentation,
};
use std::{fs::File, path::{Path, PathBuf}};

#[test]
fn alpha3_completion_requires_insert_fixture_and_differential_check() {
    let root = repository_root();

    assert!(
        root.join("tests/fixtures/archives/pg18-none-insert-basic.dump")
            .is_file(),
        "official INSERT fixture is missing"
    );
    assert!(
        root.join("scripts/generate-alpha3-insert-fixture.sh")
            .is_file(),
        "INSERT fixture regeneration script is missing"
    );
    assert!(
        root.join("scripts/check-compatibility-differential.sh")
            .is_file(),
        "pg_restore differential check is missing"
    );

    let manifest = std::fs::read_to_string(root.join("tests/fixtures/manifest.toml"))
        .expect("fixture manifest must be readable");
    assert!(manifest.contains("name = \"pg18-none-insert-basic\""));
    assert!(manifest.contains("\"insert\""));

    let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("CI workflow must be readable");
    assert!(workflow.contains("Compatibility differential"));
    assert!(workflow.contains("check-compatibility-differential.sh"));
}

#[test]
fn official_insert_fixture_allows_raw_access_and_rejects_row_api_before_copy_parsing() {
    let path = repository_root().join("tests/fixtures/archives/pg18-none-insert-basic.dump");
    let mut archive = Archive::open(File::open(path).expect("INSERT fixture must be readable"))
        .expect("INSERT fixture must open through the production archive path");

    let (data_id, representation) = {
        let table = archive
            .table(b"public", b"orders")
            .expect("orders table must be indexed");
        (
            table.data_entry_id().expect("TABLE DATA must exist"),
            table
                .data_representation()
                .expect("representation metadata must be readable"),
        )
    };
    assert_eq!(representation, TableDataRepresentation::Insert);

    let mut raw = Vec::new();
    archive
        .copy_entry_to(
            data_id,
            &mut raw,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(1_048_576),
        )
        .expect("unsupported row representation must remain available to raw extraction");
    assert!(raw.starts_with(b"INSERT INTO public.orders VALUES ("));
    assert!(!raw.windows(b"COPY public.orders".len()).any(|window| window == b"COPY public.orders"));

    let error = match archive.table_rows(b"public", b"orders") {
        Ok(_) => panic!("row APIs must not parse INSERT payload bytes as COPY text"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PgDumpError::UnsupportedTableDataRepresentation {
            representation: TableDataRepresentation::Insert,
            ..
        }
    ));
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root must be resolvable")
}
