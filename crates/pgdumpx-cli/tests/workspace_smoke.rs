use pgdumpx as _;
use std::path::Path;

#[test]
fn workspace_builds_library_and_cli() {
    assert!(Path::new(env!("CARGO_BIN_EXE_pgdumpx")).is_file());
}
