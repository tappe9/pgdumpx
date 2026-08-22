use std::path::{Path, PathBuf};

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

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root must be resolvable")
}
