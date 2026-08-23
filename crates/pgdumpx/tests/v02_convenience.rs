use pgdumpx::{
    Archive, EntryReadLimits, ExtractionPlan, ExtractionPlanError, Limits, PgDumpError,
    TableSelector,
};
use std::{
    cell::Cell,
    error::Error as _,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    rc::Rc,
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;

#[test]
fn opens_official_fixture_from_path_and_honors_explicit_limits() {
    let path = fixture_path("pg18-none-copy-basic.dump");

    let archive = Archive::open_path(&path).expect("path convenience must open official fixture");
    assert!(archive.table(b"public", b"orders").is_some());

    let error =
        Archive::open_path_with_limits(&path, Limits::default_compatible().with_max_toc_entries(0))
            .unwrap_err();
    assert!(matches!(
        error,
        PgDumpError::TocEntryLimitExceeded { limit: 0, .. }
    ));
}

#[test]
fn path_open_preserves_typed_file_io_source() {
    let missing = fixture_path("definitely-missing-v02.dump");
    let error = Archive::open_path(&missing).unwrap_err();

    assert!(matches!(error, PgDumpError::Io { offset: 0, .. }));
    let source = error.source().expect("file-open source must be preserved");
    let io_error = source
        .downcast_ref::<io::Error>()
        .expect("source must remain std::io::Error");
    assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn owned_table_selector_round_trips_exact_non_utf8_bytes() {
    let schema = [0xfe, b's'];
    let name = [0xff, b't'];
    let selector = TableSelector::new(schema, name);
    let stored = selector.clone();

    assert_eq!(stored.schema(), schema.as_slice());
    assert_eq!(stored.name(), name.as_slice());
    assert_eq!(stored, TableSelector::new(schema, name));
    assert_ne!(stored, TableSelector::new(b"public", b"orders"));
}

#[test]
fn selector_resolution_matches_existing_exact_table_lookup() {
    let archive = Archive::open(Cursor::new(build_archive(&two_table_entries()))).unwrap();
    let selector = TableSelector::new(b"public", b"orders");

    let direct = archive.table(b"public", b"orders").unwrap();
    let selected = archive.resolve_table(&selector).unwrap();

    assert_eq!(selected.table_entry_id(), direct.table_entry_id());
    assert_eq!(selected.data_entry_id(), direct.data_entry_id());
    assert!(
        archive
            .resolve_table(&TableSelector::new(b"PUBLIC", b"orders"))
            .is_none()
    );
}

#[test]
fn extraction_plan_preserves_order_limits_and_rejects_duplicates() {
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let limits = EntryReadLimits::unlimited().with_max_decompressed_bytes(4096);
    let plan =
        ExtractionPlan::with_entry_read_limits(vec![orders.clone(), inventory.clone()], limits)
            .unwrap();

    assert_eq!(plan.selectors(), &[orders.clone(), inventory.clone()]);
    assert_eq!(plan.entry_read_limits(), limits);
    assert_eq!(plan.clone().selectors(), &[orders.clone(), inventory]);

    let error = ExtractionPlan::new(vec![orders.clone(), orders]).unwrap_err();
    assert!(matches!(
        error,
        ExtractionPlanError::DuplicateSelector { .. }
    ));
}

#[test]
fn preflight_resolves_all_targets_in_order_without_payload_io() {
    let bytes_read = Rc::new(Cell::new(0_u64));
    let reader = TrackingReader::new(build_archive(&two_table_entries()), Rc::clone(&bytes_read));
    let archive = Archive::open(reader).unwrap();
    let after_open = bytes_read.get();
    let plan = ExtractionPlan::new(vec![
        TableSelector::new(b"warehouse", b"inventory"),
        TableSelector::new(b"public", b"orders"),
    ])
    .unwrap();

    let resolved = plan.preflight(&archive).unwrap();

    assert_eq!(bytes_read.get(), after_open);
    assert_eq!(resolved.tables().len(), 2);
    assert_eq!(resolved.tables()[0].name(), b"inventory");
    assert_eq!(resolved.tables()[1].name(), b"orders");
    assert_eq!(resolved.entry_read_limits(), EntryReadLimits::unlimited());
}

#[test]
fn preflight_fails_before_payload_io_for_missing_table_or_table_data() {
    let archive = Archive::open(Cursor::new(build_archive(&two_table_entries()))).unwrap();
    let missing = ExtractionPlan::new(vec![TableSelector::new(b"public", b"missing")]).unwrap();
    assert!(matches!(
        missing.preflight(&archive),
        Err(PgDumpError::TableNotFound)
    ));

    let entries = vec![EntrySpec::table(1, b"public", b"empty", b"41")];
    let archive = Archive::open(Cursor::new(build_archive(&entries))).unwrap();
    let no_data = ExtractionPlan::new(vec![TableSelector::new(b"public", b"empty")]).unwrap();
    assert!(matches!(
        no_data.preflight(&archive),
        Err(PgDumpError::TableDataEntryUnavailable { table_id: 1 })
    ));
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
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
        self.bytes_read.set(
            self.bytes_read
                .get()
                .checked_add(u64::try_from(read).expect("test read length fits u64"))
                .expect("test read count does not overflow"),
        );
        Ok(read)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

fn two_table_entries() -> Vec<EntrySpec> {
    vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table_data(2, b"public", b"orders", b"41", vec![b"1".to_vec()]),
        EntrySpec::table(3, b"warehouse", b"inventory", b"42"),
        EntrySpec::table_data(4, b"warehouse", b"inventory", b"42", vec![b"3".to_vec()]),
    ]
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
