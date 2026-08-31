use pgdumpx::{Archive, ErrorCategory, Limits, PgDumpError, ResourceLimit};
use std::io::Cursor;

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
const HEADER_RETAINED_STRING_BYTES: usize = b"database".len() + b"18.4".len() * 2;
const WITH_OIDS_BYTES: usize = b"false".len();

#[test]
fn aggregate_metadata_defaults_are_finite_and_nonzero() {
    let limits = Limits::default_compatible();

    assert!(limits.max_metadata_string_bytes() > 0);
    assert!(limits.max_metadata_string_bytes() < usize::MAX);
    assert!(limits.max_metadata_dependencies() > 0);
    assert!(limits.max_metadata_dependencies() < usize::MAX);
    assert!(limits.max_metadata_index_bytes() > 0);
    assert!(limits.max_metadata_index_bytes() < usize::MAX);
}

#[test]
fn cumulative_retained_strings_accept_exact_limit_and_reject_one_over() {
    let entries = vec![
        EntrySpec::metadata(1, b"first", Vec::new()),
        EntrySpec::metadata(2, b"second", Vec::new()),
    ];
    let exact = HEADER_RETAINED_STRING_BYTES
        + entries
            .iter()
            .map(EntrySpec::retained_string_bytes)
            .sum::<usize>();

    let archive = open_with(
        &entries,
        Limits::default().with_max_metadata_string_bytes(exact),
    )
    .expect("the exact aggregate string-byte limit is inclusive");
    assert_eq!(archive.entries().len(), 2);

    let error = open_with(
        &entries,
        Limits::default().with_max_metadata_string_bytes(exact - 1),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::MetadataStringByteLimitExceeded {
            limit,
            attempted,
            ..
        } if limit == u64::try_from(exact - 1).unwrap()
            && attempted == u64::try_from(exact).unwrap()
    ));
    assert_eq!(error.category(), ErrorCategory::Resource);
    let context = error.limit_context().expect("aggregate limit context");
    assert_eq!(context.resource(), ResourceLimit::MetadataStringBytes);
    assert_eq!(context.limit(), u64::try_from(exact - 1).unwrap());
    assert_eq!(context.consumed(), u64::try_from(exact).unwrap());
    assert!(error.byte_offset().is_some());
}

#[test]
fn cumulative_dependencies_accept_exact_limit_and_reject_first_excess() {
    let entries = vec![
        EntrySpec::metadata(1, b"first", vec![b"1".to_vec(), b"2".to_vec()]),
        EntrySpec::metadata(2, b"second", vec![b"1".to_vec(), b"2".to_vec()]),
    ];

    let archive = open_with(
        &entries,
        Limits::default().with_max_metadata_dependencies(4),
    )
    .expect("the exact aggregate dependency limit is inclusive");
    assert_eq!(archive.entries()[0].dependencies().len(), 2);
    assert_eq!(archive.entries()[1].dependencies().len(), 2);

    let error = open_with(
        &entries,
        Limits::default().with_max_metadata_dependencies(3),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::MetadataDependencyLimitExceeded {
            entry_id: 2,
            limit: 3,
            attempted: 4,
            ..
        }
    ));
    assert_eq!(error.dump_id().map(|id| id.as_i32()), Some(2));
    let context = error.limit_context().expect("aggregate limit context");
    assert_eq!(context.resource(), ResourceLimit::MetadataDependencies);
    assert_eq!(context.limit(), 3);
    assert_eq!(context.consumed(), 4);
}

#[test]
fn auxiliary_index_names_accept_exact_limit_and_reject_one_over() {
    let entries = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table_data(
            2,
            b"public",
            b"orders",
            b"41",
            vec![b"1".to_vec()],
            b"COPY public.orders (id, code) FROM stdin;\n",
        ),
    ];
    let table_index_bytes = b"public".len() + b"orders".len();
    let copy_column_metadata_and_lookup_bytes = 2 * (b"id".len() + b"code".len());
    let exact = table_index_bytes + copy_column_metadata_and_lookup_bytes;

    let archive = open_with(
        &entries,
        Limits::default().with_max_metadata_index_bytes(exact),
    )
    .expect("the exact aggregate index-byte limit is inclusive");
    let table = archive.table(b"public", b"orders").expect("table is indexed");
    assert_eq!(table.columns().unwrap().len(), 2);

    let error = open_with(
        &entries,
        Limits::default().with_max_metadata_index_bytes(exact - 1),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::MetadataIndexByteLimitExceeded {
            limit,
            attempted,
            ..
        } if limit == u64::try_from(exact - 1).unwrap()
            && attempted == u64::try_from(exact).unwrap()
    ));
    let context = error.limit_context().expect("aggregate limit context");
    assert_eq!(context.resource(), ResourceLimit::MetadataIndexBytes);
    assert_eq!(context.limit(), u64::try_from(exact - 1).unwrap());
    assert_eq!(context.consumed(), u64::try_from(exact).unwrap());
}

#[test]
fn existing_per_item_string_limit_remains_distinguishable() {
    let entries = vec![EntrySpec::metadata(1, b"entry", Vec::new())];
    let limits = Limits::default()
        .with_max_string_bytes(7)
        .with_max_metadata_string_bytes(usize::MAX);

    let error = open_with(&entries, limits).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ArchiveStringLimitExceeded {
            length: 8,
            limit: 7,
            ..
        }
    ));
    let context = error.limit_context().expect("per-item limit context");
    assert_eq!(context.resource(), ResourceLimit::ArchiveStringBytes);
}

fn open_with(
    entries: &[EntrySpec],
    limits: Limits,
) -> Result<Archive<Cursor<Vec<u8>>>, PgDumpError> {
    Archive::open_with_limits(Cursor::new(build_archive(entries)), limits)
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
    fn metadata(id: i32, tag: &[u8], dependencies: Vec<Vec<u8>>) -> Self {
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
            dependencies,
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
        copy_statement: &[u8],
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
            copy_statement: Some(copy_statement.to_vec()),
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

    fn retained_string_bytes(&self) -> usize {
        self.catalog_table_oid.len()
            + self.catalog_oid.len()
            + self.tag.len()
            + self.description.len()
            + option_len(&self.definition)
            + option_len(&self.drop_statement)
            + option_len(&self.copy_statement)
            + option_len(&self.namespace)
            + option_len(&self.tablespace)
            + option_len(&self.table_access_method)
            + option_len(&self.owner)
            + WITH_OIDS_BYTES
    }
}

fn option_len(value: &Option<Vec<u8>>) -> usize {
    value.as_ref().map_or(0, Vec::len)
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
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 16, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
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
