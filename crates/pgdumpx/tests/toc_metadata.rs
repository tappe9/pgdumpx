use pgdumpx::{Archive, ArchiveVersion, DataLocation};
use std::{io::Cursor, path::PathBuf};

#[test]
fn official_fixtures_expose_complete_toc_metadata_without_hiding_version_absence() {
    for (fixture_name, version, expected_relkind) in [
        (
            "pg15-none-copy-basic.dump",
            ArchiveVersion::new(1, 14, 0),
            None,
        ),
        (
            "pg16-none-copy-basic.dump",
            ArchiveVersion::new(1, 15, 0),
            None,
        ),
        (
            "pg18-none-copy-basic.dump",
            ArchiveVersion::new(1, 16, 0),
            Some(i32::from(b'r')),
        ),
    ] {
        let archive = Archive::open(Cursor::new(fixture(fixture_name)))
            .unwrap_or_else(|error| panic!("{fixture_name} failed to open: {error}"));
        assert_eq!(archive.header().version(), version);

        let table = archive
            .table(b"public", b"orders")
            .expect("orders table must be indexed");
        let table_id = table.table_entry_id();
        let data_id = table.data_entry_id().expect("TABLE DATA must exist");

        let table_entry = archive.entry(table_id).expect("TABLE entry must resolve");
        assert_eq!(table_entry.catalog_table_oid().as_bytes(), b"1259");
        assert!(table_entry
            .catalog_oid()
            .as_bytes()
            .iter()
            .all(u8::is_ascii_digit));
        assert_eq!(table_entry.name().as_bytes(), b"orders");
        assert_eq!(table_entry.description().as_bytes(), b"TABLE");
        assert_eq!(table_entry.namespace().map(|value| value.as_bytes()), Some(b"public".as_slice()));
        assert_eq!(
            table_entry.table_access_method().map(|value| value.as_bytes()),
            Some(b"heap".as_slice())
        );
        assert_eq!(table_entry.relation_kind(), expected_relkind);
        assert!(table_entry.definition().is_some());
        assert!(table_entry.drop_statement().is_some());
        assert!(table_entry.copy_statement().is_none());
        assert!(table_entry.owner().is_some());
        assert!(table_entry.dependencies().is_empty());
        assert_eq!(table_entry.data_location(), DataLocation::NoData);

        let data_entry = archive.entry(data_id).expect("TABLE DATA entry must resolve");
        assert_eq!(data_entry.description().as_bytes(), b"TABLE DATA");
        assert_eq!(data_entry.namespace().map(|value| value.as_bytes()), Some(b"public".as_slice()));
        assert!(data_entry.definition().is_none());
        assert!(data_entry.drop_statement().is_none());
        assert!(data_entry.copy_statement().is_some());
        assert!(data_entry.tablespace().is_none());
        assert!(data_entry.table_access_method().is_none());
        assert_eq!(data_entry.relation_kind(), version_relation_kind(version));
        assert_eq!(data_entry.dependencies(), &[table_id]);
        assert!(matches!(data_entry.data_location(), DataLocation::Offset(_)));
    }
}

fn version_relation_kind(version: ArchiveVersion) -> Option<i32> {
    (version >= ArchiveVersion::new(1, 16, 0)).then_some(0)
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}
