use pgdumpx::{Archive, ArchiveVersion, Compression, DataLocation, PgDumpError, Section};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

const OFFICIAL_METADATA_END: u64 = 1_839;
const POSITION_NOT_SET: u8 = 1;
const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
const MAX_PROVISIONAL_DEPENDENCIES: usize = 100_000;

#[test]
fn opens_official_none_fixture_through_the_public_production_path() {
    let bytes = fixture("pg18-none-copy-basic.dump");
    let bytes_read = Rc::new(Cell::new(0));
    let reader = TrackingReader::new(bytes, Rc::clone(&bytes_read));

    let archive = Archive::open(reader).expect("official 1.16 fixture must open");

    assert_eq!(bytes_read.get(), OFFICIAL_METADATA_END);
    assert_eq!(archive.header().version(), ArchiveVersion::new(1, 16, 0));
    assert_eq!(archive.header().integer_size(), 4);
    assert_eq!(archive.header().offset_size(), 8);
    assert_eq!(archive.header().compression(), Compression::None);
    assert_eq!(
        archive.header().database_name().as_bytes(),
        b"pgdumpx_fixture"
    );
    assert_eq!(
        archive.header().server_version().as_bytes(),
        b"18.4 (Debian 18.4-1.pgdg12+1)"
    );
    assert_eq!(
        archive.header().dump_version().as_bytes(),
        b"18.4 (Debian 18.4-1.pgdg12+1)"
    );
    assert_eq!(
        archive.header().database_name().to_str().unwrap(),
        "pgdumpx_fixture"
    );

    let created_at = archive.header().created_at();
    assert_eq!(created_at.second(), 15);
    assert_eq!(created_at.minute(), 4);
    assert_eq!(created_at.hour(), 11);
    assert_eq!(created_at.day_of_month(), 19);
    assert_eq!(created_at.month_zero_based(), 7);
    assert_eq!(created_at.year_since_1900(), 126);
    assert_eq!(created_at.is_dst(), 0);

    assert_eq!(archive.entries().len(), 7);
    let table = archive
        .table(b"public", b"orders")
        .expect("orders table must be indexed");
    assert_eq!(table.schema(), Some(b"public".as_slice()));
    assert_eq!(table.name(), b"orders");
    assert_eq!(table.table_entry_id().as_i32(), 219);
    assert_eq!(table.data_entry_id().map(|id| id.as_i32()), Some(3372));

    let table_entry = archive
        .entry(table.table_entry_id())
        .expect("table dump ID must resolve");
    assert_eq!(table_entry.description_bytes(), b"TABLE");
    assert_eq!(table_entry.section(), Section::PreData);
    assert_eq!(table_entry.data_location(), DataLocation::NoData);

    let data_entry = archive
        .entry(table.data_entry_id().unwrap())
        .expect("table-data dump ID must resolve");
    assert!(data_entry.has_data());
    assert_eq!(data_entry.description_bytes(), b"TABLE DATA");
    assert_eq!(data_entry.section(), Section::Data);
    assert_eq!(data_entry.dependencies().len(), 1);
    assert_eq!(data_entry.dependencies()[0].as_i32(), 219);
    assert_eq!(
        data_entry.data_location(),
        DataLocation::Offset(OFFICIAL_METADATA_END)
    );
}

#[test]
fn opens_official_gzip_fixture_without_reading_payload() {
    let bytes = fixture("pg18-gzip-copy-basic.dump");
    let bytes_read = Rc::new(Cell::new(0));
    let reader = TrackingReader::new(bytes, Rc::clone(&bytes_read));

    let archive = Archive::open(reader).expect("official gzip fixture must open");

    assert_eq!(bytes_read.get(), OFFICIAL_METADATA_END);
    assert_eq!(archive.header().compression(), Compression::Gzip);
    let table = archive.table(b"public", b"orders").unwrap();
    let data_entry = archive.entry(table.data_entry_id().unwrap()).unwrap();
    assert_eq!(
        data_entry.data_location(),
        DataLocation::Offset(OFFICIAL_METADATA_END)
    );
}

#[test]
fn preserves_all_custom_data_location_states() {
    let entries = vec![
        EntrySpec::metadata(1, b"unknown").with_location(POSITION_NOT_SET, 0),
        EntrySpec::metadata(2, b"offset").with_location(POSITION_SET, 99),
        EntrySpec::metadata(3, b"none").with_location(NO_DATA, 0),
    ];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();

    assert_eq!(archive.entries()[0].data_location(), DataLocation::Unknown);
    assert_eq!(
        archive.entries()[1].data_location(),
        DataLocation::Offset(99)
    );
    assert_eq!(archive.entries()[2].data_location(), DataLocation::NoData);
}

#[test]
fn indexes_same_table_name_in_different_schemas() {
    let entries = vec![
        EntrySpec::table(1, b"alpha", b"orders", b"41"),
        EntrySpec::table(2, b"beta", b"orders", b"42"),
    ];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();

    assert_eq!(
        archive
            .table(b"alpha", b"orders")
            .unwrap()
            .table_entry_id()
            .as_i32(),
        1
    );
    assert_eq!(
        archive
            .table(b"beta", b"orders")
            .unwrap()
            .table_entry_id()
            .as_i32(),
        2
    );
}

#[test]
fn table_lookup_and_metadata_remain_byte_oriented() {
    let schema = [0xfe];
    let name = [0xff];
    let entries = vec![
        EntrySpec::table(1, &schema, &name, b"41"),
        EntrySpec::table_data(2, &schema, &name, b"41", vec![b"1".to_vec()]),
    ];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();

    let table = archive.table(&schema, &name).unwrap();
    assert_eq!(table.schema(), Some(schema.as_slice()));
    assert_eq!(table.name(), name.as_slice());
    assert_eq!(table.data_entry_id().map(|id| id.as_i32()), Some(2));

    let entry = archive.entry(table.table_entry_id()).unwrap();
    assert!(entry.name().to_str().is_err());
}

#[test]
fn a_table_without_table_data_is_still_indexed() {
    let entries = vec![EntrySpec::table(1, b"public", b"empty_table", b"41")];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();

    let table = archive.table(b"public", b"empty_table").unwrap();
    assert_eq!(table.table_entry_id().as_i32(), 1);
    assert_eq!(table.data_entry_id(), None);
}

#[test]
fn standalone_table_data_is_not_synthesized_into_a_table() {
    let entries = vec![EntrySpec::table_data(
        2,
        b"public",
        b"orders",
        b"41",
        Vec::new(),
    )];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();

    assert!(archive.table(b"public", b"orders").is_none());
    assert_eq!(archive.entries().len(), 1);
}

#[test]
fn rejects_bad_and_truncated_magic() {
    let bad = Archive::open(Cursor::new(b"NOTMP".to_vec())).unwrap_err();
    assert!(matches!(
        bad,
        PgDumpError::InvalidArchiveMagic { offset: 0 }
    ));

    let truncated = Archive::open(Cursor::new(b"PGD".to_vec())).unwrap_err();
    assert!(matches!(
        truncated,
        PgDumpError::UnexpectedEof { offset: 3 }
    ));
}

#[test]
fn rejects_versions_outside_the_exact_1_16_0_scope() {
    for version in [[1, 15, 0], [1, 16, 1], [1, 17, 0]] {
        let bytes = header_with(version, 1, 0);
        let error = Archive::open(Cursor::new(bytes)).unwrap_err();
        assert!(matches!(
            error,
            PgDumpError::UnsupportedArchiveVersion {
                major,
                minor,
                revision,
                offset: 5,
            } if [major, minor, revision] == version
        ));
    }
}

#[test]
fn rejects_non_custom_format_and_unknown_compression() {
    let wrong_format = Archive::open(Cursor::new(header_with([1, 16, 0], 3, 0))).unwrap_err();
    assert!(matches!(
        wrong_format,
        PgDumpError::UnexpectedArchiveFormat {
            format: 3,
            offset: 10,
        }
    ));

    let compression = Archive::open(Cursor::new(header_with([1, 16, 0], 1, 9))).unwrap_err();
    assert!(matches!(
        compression,
        PgDumpError::UnsupportedCompressionAlgorithm {
            algorithm: 9,
            offset: 11,
        }
    ));
}

#[test]
fn rejects_impossible_dump_ids_and_malformed_dependencies() {
    let impossible = vec![EntrySpec::metadata(0, b"bad")];
    let error = Archive::open(Cursor::new(build_archive(&impossible))).unwrap_err();
    assert!(matches!(error, PgDumpError::InvalidDumpId { value: 0, .. }));

    let malformed = vec![EntrySpec::table_data(
        2,
        b"public",
        b"orders",
        b"41",
        vec![b"not-an-id".to_vec()],
    )];
    let error = Archive::open(Cursor::new(build_archive(&malformed))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::InvalidDependencyEncoding { entry_id: 2, .. }
    ));
}

#[test]
fn rejects_duplicate_dump_ids() {
    let entries = vec![
        EntrySpec::metadata(1, b"first"),
        EntrySpec::metadata(1, b"second"),
    ];
    let error = Archive::open(Cursor::new(build_archive(&entries))).unwrap_err();

    assert!(matches!(error, PgDumpError::DuplicateDumpId { dump_id: 1 }));
}

#[test]
fn rejects_ambiguous_or_conflicting_table_relationships() {
    let duplicate_tables = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table(2, b"public", b"orders", b"42"),
    ];
    let error = Archive::open(Cursor::new(build_archive(&duplicate_tables))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::DuplicateTableIdentity {
            first_table_id: 1,
            second_table_id: 2,
        }
    ));

    let conflicting = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table_data(2, b"other", b"orders", b"41", vec![b"1".to_vec()]),
    ];
    let error = Archive::open(Cursor::new(build_archive(&conflicting))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::ConflictingTableDataRelationship {
            table_id: 1,
            data_id: 2,
        }
    ));

    let duplicate_data = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table_data(2, b"public", b"orders", b"41", vec![b"1".to_vec()]),
        EntrySpec::table_data(3, b"public", b"orders", b"41", vec![b"1".to_vec()]),
    ];
    let error = Archive::open(Cursor::new(build_archive(&duplicate_data))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::DuplicateTableDataRelationship {
            table_id: 1,
            first_data_id: 2,
            second_data_id: 3,
        }
    ));

    let ambiguous = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table(2, b"other", b"orders", b"41"),
        EntrySpec::table_data(
            3,
            b"public",
            b"orders",
            b"41",
            vec![b"1".to_vec(), b"2".to_vec()],
        ),
    ];
    let error = Archive::open(Cursor::new(build_archive(&ambiguous))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::AmbiguousTableDataRelationship { data_id: 3 }
    ));
}

#[test]
fn rejects_truncated_toc_and_oversized_structural_counts() {
    let mut truncated = build_archive(&[EntrySpec::metadata(1, b"entry")]);
    truncated.pop();
    let error = Archive::open(Cursor::new(truncated)).unwrap_err();
    assert!(matches!(error, PgDumpError::UnexpectedEof { .. }));

    let mut oversized_toc = complete_header();
    write_int(&mut oversized_toc, i32::MAX);
    oversized_toc.extend_from_slice(b"payload-must-not-be-read");
    let error = Archive::open(Cursor::new(oversized_toc)).unwrap_err();
    assert!(matches!(error, PgDumpError::TocEntryLimitExceeded { .. }));

    let dependencies = (0..=MAX_PROVISIONAL_DEPENDENCIES)
        .map(|_| b"1".to_vec())
        .collect();
    let oversized_dependencies = vec![EntrySpec::table_data(
        2,
        b"public",
        b"orders",
        b"41",
        dependencies,
    )];
    let error = Archive::open(Cursor::new(build_archive(&oversized_dependencies))).unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::DependencyLimitExceeded { entry_id: 2, .. }
    ));
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}

#[derive(Debug)]
struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<u64>>,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>, bytes_read: Rc<Cell<u64>>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read,
        }
    }
}

impl Read for TrackingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        let read = u64::try_from(read).expect("test read length fits u64");
        self.bytes_read.set(
            self.bytes_read
                .get()
                .checked_add(read)
                .expect("test read count does not overflow"),
        );
        Ok(usize::try_from(read).expect("round trip test read length"))
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[derive(Debug, Clone)]
struct EntrySpec {
    id: i32,
    has_data: i32,
    catalog_table_oid: Vec<u8>,
    catalog_oid: Vec<u8>,
    tag: Vec<u8>,
    description: Vec<u8>,
    section: i32,
    definition: Option<Vec<u8>>,
    drop_statement: Option<Vec<u8>>,
    copy_statement: Option<Vec<u8>>,
    namespace: Option<Vec<u8>>,
    tablespace: Option<Vec<u8>>,
    table_access_method: Option<Vec<u8>>,
    relation_kind: i32,
    owner: Option<Vec<u8>>,
    dependencies: Vec<Vec<u8>>,
    offset_state: u8,
    offset: u64,
}

impl EntrySpec {
    fn metadata(id: i32, tag: &[u8]) -> Self {
        Self {
            id,
            has_data: 0,
            catalog_table_oid: b"0".to_vec(),
            catalog_oid: b"0".to_vec(),
            tag: tag.to_vec(),
            description: b"COMMENT".to_vec(),
            section: SECTION_PRE_DATA,
            definition: None,
            drop_statement: None,
            copy_statement: None,
            namespace: None,
            tablespace: None,
            table_access_method: None,
            relation_kind: 0,
            owner: None,
            dependencies: Vec::new(),
            offset_state: NO_DATA,
            offset: 0,
        }
    }

    fn table(id: i32, schema: &[u8], name: &[u8], catalog_oid: &[u8]) -> Self {
        Self {
            id,
            has_data: 0,
            catalog_table_oid: b"1259".to_vec(),
            catalog_oid: catalog_oid.to_vec(),
            tag: name.to_vec(),
            description: b"TABLE".to_vec(),
            section: SECTION_PRE_DATA,
            definition: Some(b"CREATE TABLE".to_vec()),
            drop_statement: Some(b"DROP TABLE".to_vec()),
            copy_statement: None,
            namespace: Some(schema.to_vec()),
            tablespace: Some(Vec::new()),
            table_access_method: Some(b"heap".to_vec()),
            relation_kind: i32::from(b'r'),
            owner: Some(b"postgres".to_vec()),
            dependencies: Vec::new(),
            offset_state: NO_DATA,
            offset: 0,
        }
    }

    fn table_data(
        id: i32,
        schema: &[u8],
        name: &[u8],
        catalog_oid: &[u8],
        dependencies: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            id,
            has_data: 1,
            catalog_table_oid: b"0".to_vec(),
            catalog_oid: catalog_oid.to_vec(),
            tag: name.to_vec(),
            description: b"TABLE DATA".to_vec(),
            section: SECTION_DATA,
            definition: None,
            drop_statement: None,
            copy_statement: Some(b"COPY table FROM stdin;\n".to_vec()),
            namespace: Some(schema.to_vec()),
            tablespace: None,
            table_access_method: None,
            relation_kind: 0,
            owner: Some(b"postgres".to_vec()),
            dependencies,
            offset_state: POSITION_SET,
            offset: 2_048,
        }
    }

    fn with_location(mut self, state: u8, offset: u64) -> Self {
        self.offset_state = state;
        self.offset = offset;
        self
    }
}

fn build_archive(entries: &[EntrySpec]) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(
        &mut bytes,
        i32::try_from(entries.len()).expect("test entry count fits i32"),
    );

    for entry in entries {
        write_int(&mut bytes, entry.id);
        write_int(&mut bytes, entry.has_data);
        write_string(&mut bytes, Some(&entry.catalog_table_oid));
        write_string(&mut bytes, Some(&entry.catalog_oid));
        write_string(&mut bytes, Some(&entry.tag));
        write_string(&mut bytes, Some(&entry.description));
        write_int(&mut bytes, entry.section);
        write_string(&mut bytes, entry.definition.as_deref());
        write_string(&mut bytes, entry.drop_statement.as_deref());
        write_string(&mut bytes, entry.copy_statement.as_deref());
        write_string(&mut bytes, entry.namespace.as_deref());
        write_string(&mut bytes, entry.tablespace.as_deref());
        write_string(&mut bytes, entry.table_access_method.as_deref());
        write_int(&mut bytes, entry.relation_kind);
        write_string(&mut bytes, entry.owner.as_deref());
        write_string(&mut bytes, Some(b"false"));
        for dependency in &entry.dependencies {
            write_string(&mut bytes, Some(dependency));
        }
        write_string(&mut bytes, None);
        bytes.push(entry.offset_state);
        bytes.extend_from_slice(&entry.offset.to_le_bytes());
    }

    bytes
}

fn complete_header() -> Vec<u8> {
    let mut bytes = header_with([1, 16, 0], 1, 0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
    bytes
}

fn header_with(version: [u8; 3], format: u8, compression: u8) -> Vec<u8> {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&version);
    bytes.push(4);
    bytes.push(8);
    bytes.push(format);
    bytes.push(compression);
    bytes
}

fn write_int(output: &mut Vec<u8>, value: i32) {
    output.push(u8::from(value.is_negative()));
    output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
}

fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_int(
                output,
                i32::try_from(bytes.len()).expect("test string length fits i32"),
            );
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}
