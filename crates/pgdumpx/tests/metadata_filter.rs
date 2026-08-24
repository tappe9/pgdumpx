use pgdumpx::{Archive, ExtractionPlan, MetadataFilter};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    rc::Rc,
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
const SECTION_POST_DATA: i32 = 4;

#[test]
fn filters_schema_object_type_and_name_with_and_semantics_in_toc_order() {
    let archive = Archive::open(Cursor::new(build_archive(&metadata_entries()))).unwrap();

    let public = archive.filter_metadata(&MetadataFilter::new().with_schema(b"public"));
    assert_eq!(dump_ids(&public), vec![1, 2, 3]);
    assert_eq!(
        public
            .iter()
            .map(|matched| matched.entry().description_bytes())
            .collect::<Vec<_>>(),
        vec![b"TABLE".as_slice(), b"TABLE DATA".as_slice(), b"INDEX".as_slice()]
    );

    let exact = MetadataFilter::new()
        .with_schema(b"public")
        .with_object_type(b"TABLE")
        .with_name(b"orders");
    assert_eq!(dump_ids(&archive.filter_metadata(&exact)), vec![1]);

    let table_order = MetadataFilter::new().with_object_type(b"TABLE");
    assert_eq!(
        dump_ids(&archive.filter_metadata(&table_order)),
        vec![1, 4, 5, 6, 7]
    );

    let missing = MetadataFilter::new().with_name(b"missing");
    assert!(archive.filter_metadata(&missing).is_empty());
}

#[test]
fn distinguishes_absent_namespace_from_exact_empty_namespace() {
    let archive = Archive::open(Cursor::new(build_archive(&metadata_entries()))).unwrap();

    let absent = MetadataFilter::new().with_absent_schema();
    assert_eq!(dump_ids(&archive.filter_metadata(&absent)), vec![5]);

    let empty = MetadataFilter::new().with_schema(b"");
    assert_eq!(dump_ids(&archive.filter_metadata(&empty)), vec![6]);

    let public = MetadataFilter::new().with_schema(b"public");
    assert!(!dump_ids(&archive.filter_metadata(&public)).contains(&5));
}

#[test]
fn matches_non_utf8_schema_and_name_as_exact_bytes() {
    let archive = Archive::open(Cursor::new(build_archive(&metadata_entries()))).unwrap();
    let schema = [0xfe_u8, b's'];
    let name = [0xff_u8, b't'];

    let filter = MetadataFilter::new()
        .with_schema(schema)
        .with_object_type(b"TABLE")
        .with_name(name);
    let matches = archive.filter_metadata(&filter);

    assert_eq!(dump_ids(&matches), vec![7]);
    assert_eq!(matches[0].entry().namespace_bytes(), Some(schema.as_slice()));
    assert_eq!(matches[0].entry().name_bytes(), name.as_slice());

    let lossy_lookalike = MetadataFilter::new()
        .with_schema("�s".as_bytes())
        .with_name("�t".as_bytes());
    assert!(archive.filter_metadata(&lossy_lookalike).is_empty());
}

#[test]
fn filtering_is_metadata_only_and_does_not_seek_or_read_payloads() {
    let bytes_read = Rc::new(Cell::new(0_u64));
    let seek_count = Rc::new(Cell::new(0_u64));
    let reader = TrackingReader::new(
        build_archive(&metadata_entries()),
        Rc::clone(&bytes_read),
        Rc::clone(&seek_count),
    );
    let archive = Archive::open(reader).unwrap();
    let after_open_bytes = bytes_read.get();
    let after_open_seeks = seek_count.get();

    let matches = archive.filter_metadata(
        &MetadataFilter::new()
            .with_schema(b"public")
            .with_object_type(b"TABLE DATA"),
    );

    assert_eq!(dump_ids(&matches), vec![2]);
    assert_eq!(bytes_read.get(), after_open_bytes);
    assert_eq!(seek_count.get(), after_open_seeks);
}

#[test]
fn only_normal_tables_with_concrete_namespaces_convert_to_table_selectors() {
    let archive = Archive::open(Cursor::new(build_archive(&metadata_entries()))).unwrap();

    let public_tables = archive.filter_metadata(
        &MetadataFilter::new()
            .with_schema(b"public")
            .with_object_type(b"TABLE"),
    );
    let selectors = public_tables
        .iter()
        .filter_map(|matched| matched.table_selector())
        .collect::<Vec<_>>();
    assert_eq!(selectors.len(), 1);
    assert_eq!(selectors[0].schema(), b"public");
    assert_eq!(selectors[0].name(), b"orders");

    let plan = ExtractionPlan::new(selectors).unwrap();
    let resolved = plan.preflight(&archive).unwrap();
    assert_eq!(resolved.tables()[0].table_entry_id().as_i32(), 1);
    assert_eq!(resolved.tables()[0].data_entry_id().unwrap().as_i32(), 2);

    let schema_less = archive.filter_metadata(
        &MetadataFilter::new()
            .with_absent_schema()
            .with_object_type(b"TABLE"),
    );
    assert_eq!(dump_ids(&schema_less), vec![5]);
    assert!(schema_less[0].table_selector().is_none());

    let table_data = archive.filter_metadata(&MetadataFilter::new().with_object_type(b"TABLE DATA"));
    assert_eq!(dump_ids(&table_data), vec![2, 8]);
    assert!(table_data.iter().all(|matched| matched.table_selector().is_none()));
}

fn dump_ids(matches: &[pgdumpx::MetadataMatch<'_>]) -> Vec<i32> {
    matches
        .iter()
        .map(|matched| matched.entry().id().as_i32())
        .collect()
}

#[derive(Debug)]
struct TrackingReader {
    inner: Cursor<Vec<u8>>,
    bytes_read: Rc<Cell<u64>>,
    seek_count: Rc<Cell<u64>>,
}

impl TrackingReader {
    fn new(bytes: Vec<u8>, bytes_read: Rc<Cell<u64>>, seek_count: Rc<Cell<u64>>) -> Self {
        Self {
            inner: Cursor::new(bytes),
            bytes_read,
            seek_count,
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
        self.seek_count.set(
            self.seek_count
                .get()
                .checked_add(1)
                .expect("test seek count does not overflow"),
        );
        self.inner.seek(position)
    }
}

fn metadata_entries() -> Vec<EntrySpec> {
    vec![
        EntrySpec::table(1, Some(b"public"), b"orders", b"41"),
        EntrySpec::table_data(
            2,
            Some(b"public"),
            b"orders",
            b"41",
            vec![b"1".to_vec()],
        ),
        EntrySpec::generic(3, Some(b"public"), b"orders_pkey", b"INDEX", SECTION_POST_DATA),
        EntrySpec::table(4, Some(b"warehouse"), b"orders", b"42"),
        EntrySpec::table(5, None, b"schema_less", b"43"),
        EntrySpec::table(6, Some(b""), b"empty_schema", b"44"),
        EntrySpec::table(7, Some(&[0xfe, b's']), &[0xff, b't'], b"45"),
        EntrySpec::table_data(8, Some(b"orphan"), b"orphan", b"46", Vec::new()),
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
    fn table(id: i32, schema: Option<&[u8]>, name: &[u8], catalog_oid: &[u8]) -> Self {
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
            namespace: schema.map(ToOwned::to_owned),
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
        schema: Option<&[u8]>,
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
            namespace: schema.map(ToOwned::to_owned),
            tablespace: None,
            table_access_method: None,
            relation_kind: 0,
            owner: Some(b"postgres".to_vec()),
            dependencies,
            offset_state: POSITION_SET,
            offset: 2_048,
        }
    }

    fn generic(
        id: i32,
        schema: Option<&[u8]>,
        name: &[u8],
        description: &[u8],
        section: i32,
    ) -> Self {
        Self {
            id,
            has_data: 0,
            catalog_table_oid: b"0".to_vec(),
            catalog_oid: b"0".to_vec(),
            tag: name.to_vec(),
            description: description.to_vec(),
            section,
            definition: Some(Vec::new()),
            drop_statement: None,
            copy_statement: None,
            namespace: schema.map(ToOwned::to_owned),
            tablespace: None,
            table_access_method: None,
            relation_kind: 0,
            owner: Some(b"postgres".to_vec()),
            dependencies: Vec::new(),
            offset_state: NO_DATA,
            offset: 0,
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
