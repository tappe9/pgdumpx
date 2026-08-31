use pgdumpx::{Archive, EntryReadLimits, ExtractionPlan, PgDumpError, TableSelector};
use std::{
    cell::{Cell, RefCell},
    io::{self, Cursor, Write},
    rc::Rc,
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const BLK_DATA: u8 = 1;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
const ORDERS_PAYLOAD: &[u8] = b"orders-data\n";
const INVENTORY_PAYLOAD: &[u8] = b"inventory-data\n";
const ORDERS_ZLIB: &[u8] = &[
    0x78, 0x9c, 0xcb, 0x2f, 0x4a, 0x49, 0x2d, 0x2a, 0xd6, 0x4d, 0x49, 0x2c, 0x49, 0xe4, 0x02, 0x00,
    0x1e, 0xfe, 0x04, 0x61,
];
const INVENTORY_ZLIB: &[u8] = &[
    0x78, 0x9c, 0xcb, 0xcc, 0x2b, 0x4b, 0xcd, 0x2b, 0xc9, 0x2f, 0xaa, 0xd4, 0x4d, 0x49, 0x2c, 0x49,
    0xe4, 0x02, 0x00, 0x31, 0xaa, 0x05, 0xc0,
];

#[test]
fn executes_multiple_targets_in_plan_order_for_none_and_gzip() {
    for compression in [0_u8, 1_u8] {
        let mut archive =
            Archive::open(Cursor::new(two_table_archive(compression))).expect("archive opens");
        let inventory = TableSelector::new(b"warehouse", b"inventory");
        let orders = TableSelector::new(b"public", b"orders");
        let plan = ExtractionPlan::new(vec![inventory.clone(), orders.clone()]).unwrap();
        let started = Rc::new(RefCell::new(Vec::<TableSelector>::new()));
        let orders_output = Rc::new(RefCell::new(Vec::new()));
        let inventory_output = Rc::new(RefCell::new(Vec::new()));

        let outcomes = plan
            .execute(&mut archive, {
                let started = Rc::clone(&started);
                let orders_output = Rc::clone(&orders_output);
                let inventory_output = Rc::clone(&inventory_output);
                let orders = orders.clone();
                move |target| {
                    started.borrow_mut().push(target.selector().clone());
                    let output = if target.selector() == &orders {
                        Rc::clone(&orders_output)
                    } else {
                        Rc::clone(&inventory_output)
                    };
                    Ok(SharedWriter(output))
                }
            })
            .expect("plan execution succeeds");

        assert_eq!(
            started.borrow().as_slice(),
            &[inventory.clone(), orders.clone()]
        );
        assert_eq!(inventory_output.borrow().as_slice(), INVENTORY_PAYLOAD);
        assert_eq!(orders_output.borrow().as_slice(), ORDERS_PAYLOAD);
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].target().selector(), &inventory);
        assert_eq!(outcomes[0].target().table_entry_id().as_i32(), 3);
        assert_eq!(outcomes[0].target().data_entry_id().as_i32(), 4);
        assert_eq!(
            outcomes[0].copied_bytes(),
            u64::try_from(INVENTORY_PAYLOAD.len()).unwrap()
        );
        assert_eq!(outcomes[1].target().selector(), &orders);
        assert_eq!(outcomes[1].target().table_entry_id().as_i32(), 1);
        assert_eq!(outcomes[1].target().data_entry_id().as_i32(), 2);
        assert_eq!(
            outcomes[1].copied_bytes(),
            u64::try_from(ORDERS_PAYLOAD.len()).unwrap()
        );
    }
}

#[test]
fn complete_preflight_happens_before_any_destination_is_started() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let plan = ExtractionPlan::new(vec![
        TableSelector::new(b"public", b"orders"),
        TableSelector::new(b"public", b"missing"),
    ])
    .unwrap();
    let starts = Cell::new(0_u32);
    let output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, |_| {
            starts.set(starts.get() + 1);
            Ok(SharedWriter(Rc::clone(&output)))
        })
        .unwrap_err();

    assert_eq!(starts.get(), 0);
    assert!(output.borrow().is_empty());
    assert!(error.completed().is_empty());
    assert!(error.failed_target().is_none());
    assert!(matches!(error.pgdump_error(), PgDumpError::TableNotFound));
}

#[test]
fn mid_target_output_failure_stops_before_later_target() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let plan = ExtractionPlan::new(vec![orders.clone(), inventory.clone()]).unwrap();
    let started = Rc::new(RefCell::new(Vec::<TableSelector>::new()));
    let inventory_output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, {
            let started = Rc::clone(&started);
            let inventory_output = Rc::clone(&inventory_output);
            let orders = orders.clone();
            move |target| {
                started.borrow_mut().push(target.selector().clone());
                if target.selector() == &orders {
                    Ok(TestWriter::Fail(FailAfter::new(4)))
                } else {
                    Ok(TestWriter::Shared(SharedWriter(Rc::clone(
                        &inventory_output,
                    ))))
                }
            }
        })
        .unwrap_err();

    assert_eq!(started.borrow().as_slice(), std::slice::from_ref(&orders));
    assert!(inventory_output.borrow().is_empty());
    assert!(error.completed().is_empty());
    assert_eq!(error.failed_target().unwrap().selector(), &orders);
    assert!(matches!(
        error.pgdump_error(),
        PgDumpError::EntryOutputIo {
            dump_id: 2,
            written: 4,
            ..
        }
    ));
}

#[test]
fn failure_after_completed_target_reports_completed_outcome() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let plan = ExtractionPlan::new(vec![orders.clone(), inventory.clone()]).unwrap();
    let orders_output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, {
            let orders = orders.clone();
            let orders_output = Rc::clone(&orders_output);
            move |target| {
                if target.selector() == &orders {
                    Ok(TestWriter::Shared(SharedWriter(Rc::clone(&orders_output))))
                } else {
                    Ok(TestWriter::Fail(FailAfter::new(3)))
                }
            }
        })
        .unwrap_err();

    assert_eq!(orders_output.borrow().as_slice(), ORDERS_PAYLOAD);
    assert_eq!(error.completed().len(), 1);
    assert_eq!(error.completed()[0].target().selector(), &orders);
    assert_eq!(
        error.completed()[0].copied_bytes(),
        u64::try_from(ORDERS_PAYLOAD.len()).unwrap()
    );
    assert_eq!(error.failed_target().unwrap().selector(), &inventory);
    assert!(matches!(
        error.pgdump_error(),
        PgDumpError::EntryOutputIo {
            dump_id: 4,
            written: 3,
            ..
        }
    ));
}

#[test]
fn per_target_raw_limit_exhaustion_is_error_and_stops_later_target() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let plan = ExtractionPlan::with_entry_read_limits(
        vec![orders.clone(), inventory],
        EntryReadLimits::unlimited().with_max_decompressed_bytes(4),
    )
    .unwrap();
    let started = Rc::new(RefCell::new(Vec::<TableSelector>::new()));
    let output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, {
            let started = Rc::clone(&started);
            let output = Rc::clone(&output);
            move |target| {
                started.borrow_mut().push(target.selector().clone());
                Ok(SharedWriter(Rc::clone(&output)))
            }
        })
        .unwrap_err();

    assert_eq!(started.borrow().as_slice(), std::slice::from_ref(&orders));
    assert_eq!(output.borrow().as_slice(), &ORDERS_PAYLOAD[..4]);
    assert!(error.completed().is_empty());
    assert_eq!(error.failed_target().unwrap().selector(), &orders);
    assert!(matches!(
        error.pgdump_error(),
        PgDumpError::EntryDecompressedByteLimitExceeded {
            dump_id: 2,
            limit: 4,
            ..
        }
    ));
}

#[test]
fn flush_failure_reports_copied_bytes_and_stops_before_later_target() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let plan = ExtractionPlan::new(vec![orders.clone(), inventory.clone()]).unwrap();
    let started = Rc::new(RefCell::new(Vec::<TableSelector>::new()));
    let orders_output = Rc::new(RefCell::new(Vec::new()));
    let inventory_output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, {
            let started = Rc::clone(&started);
            let orders_output = Rc::clone(&orders_output);
            let inventory_output = Rc::clone(&inventory_output);
            let orders = orders.clone();
            move |target| {
                started.borrow_mut().push(target.selector().clone());
                if target.selector() == &orders {
                    Ok(TestWriter::Flush(FailOnFlush(SharedWriter(Rc::clone(
                        &orders_output,
                    )))))
                } else {
                    Ok(TestWriter::Shared(SharedWriter(Rc::clone(
                        &inventory_output,
                    ))))
                }
            }
        })
        .unwrap_err();

    assert_eq!(started.borrow().as_slice(), std::slice::from_ref(&orders));
    assert_eq!(orders_output.borrow().as_slice(), ORDERS_PAYLOAD);
    assert!(inventory_output.borrow().is_empty());
    assert!(error.completed().is_empty());
    assert_eq!(error.failed_target().unwrap().selector(), &orders);
    match error.pgdump_error() {
        PgDumpError::EntryOutputIo {
            dump_id, written, ..
        } => {
            assert_eq!(*dump_id, 2);
            assert_eq!(*written, u64::try_from(ORDERS_PAYLOAD.len()).unwrap());
        }
        other => panic!("expected EntryOutputIo, got {other:?}"),
    }
}

#[test]
fn flush_failure_preserves_earlier_completed_outcomes() {
    let mut archive = Archive::open(Cursor::new(two_table_archive(0))).unwrap();
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"warehouse", b"inventory");
    let plan = ExtractionPlan::new(vec![orders.clone(), inventory.clone()]).unwrap();
    let orders_output = Rc::new(RefCell::new(Vec::new()));
    let inventory_output = Rc::new(RefCell::new(Vec::new()));

    let error = plan
        .execute(&mut archive, {
            let orders = orders.clone();
            let orders_output = Rc::clone(&orders_output);
            let inventory_output = Rc::clone(&inventory_output);
            move |target| {
                if target.selector() == &orders {
                    Ok(TestWriter::Shared(SharedWriter(Rc::clone(&orders_output))))
                } else {
                    Ok(TestWriter::Flush(FailOnFlush(SharedWriter(Rc::clone(
                        &inventory_output,
                    )))))
                }
            }
        })
        .unwrap_err();

    assert_eq!(orders_output.borrow().as_slice(), ORDERS_PAYLOAD);
    assert_eq!(inventory_output.borrow().as_slice(), INVENTORY_PAYLOAD);
    assert_eq!(error.completed().len(), 1);
    assert_eq!(error.completed()[0].target().selector(), &orders);
    assert_eq!(
        error.completed()[0].copied_bytes(),
        u64::try_from(ORDERS_PAYLOAD.len()).unwrap()
    );
    assert_eq!(error.failed_target().unwrap().selector(), &inventory);
    match error.pgdump_error() {
        PgDumpError::EntryOutputIo {
            dump_id, written, ..
        } => {
            assert_eq!(*dump_id, 4);
            assert_eq!(
                *written,
                u64::try_from(INVENTORY_PAYLOAD.len()).unwrap()
            );
        }
        other => panic!("expected EntryOutputIo, got {other:?}"),
    }
}

#[derive(Debug, Clone)]
struct SharedWriter(Rc<RefCell<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
enum TestWriter {
    Shared(SharedWriter),
    Fail(FailAfter),
    Flush(FailOnFlush),
}

impl Write for TestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Shared(writer) => writer.write(bytes),
            Self::Fail(writer) => writer.write(bytes),
            Self::Flush(writer) => writer.write(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Shared(writer) => writer.flush(),
            Self::Fail(writer) => writer.flush(),
            Self::Flush(writer) => writer.flush(),
        }
    }
}

#[derive(Debug)]
struct FailOnFlush(SharedWriter);

impl Write for FailOnFlush {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("intentional flush failure"))
    }
}

#[derive(Debug)]
struct FailAfter {
    remaining: usize,
}

impl FailAfter {
    const fn new(remaining: usize) -> Self {
        Self { remaining }
    }
}

impl Write for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("intentional destination failure"));
        }
        let accepted = bytes.len().min(self.remaining);
        self.remaining -= accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn two_table_archive(compression: u8) -> Vec<u8> {
    let (orders_payload, inventory_payload) = match compression {
        0 => (ORDERS_PAYLOAD, INVENTORY_PAYLOAD),
        1 => (ORDERS_ZLIB, INVENTORY_ZLIB),
        other => panic!("unsupported test compression {other}"),
    };
    let entries = vec![
        EntrySpec::table(1, b"public", b"orders", b"41"),
        EntrySpec::table_data(
            2,
            b"public",
            b"orders",
            b"41",
            vec![b"1".to_vec()],
            orders_payload,
        ),
        EntrySpec::table(3, b"warehouse", b"inventory", b"42"),
        EntrySpec::table_data(
            4,
            b"warehouse",
            b"inventory",
            b"42",
            vec![b"3".to_vec()],
            inventory_payload,
        ),
    ];
    build_archive(compression, &entries)
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
    payload: Option<&'static [u8]>,
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
            payload: None,
        }
    }

    fn table_data(
        id: i32,
        schema: &[u8],
        name: &[u8],
        catalog_oid: &[u8],
        dependencies: Vec<Vec<u8>>,
        payload: &'static [u8],
    ) -> Self {
        let mut copy_statement = b"COPY ".to_vec();
        copy_statement.extend_from_slice(schema);
        copy_statement.push(b'.');
        copy_statement.extend_from_slice(name);
        copy_statement.extend_from_slice(b" (value) FROM stdin;\n");
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
            copy_statement: Some(copy_statement),
            namespace: Some(schema.to_vec()),
            tablespace: None,
            table_access_method: None,
            relation_kind: 0,
            owner: Some(b"postgres".to_vec()),
            dependencies,
            offset_state: POSITION_SET,
            payload: Some(payload),
        }
    }
}

fn build_archive(compression: u8, entries: &[EntrySpec]) -> Vec<u8> {
    let mut bytes = complete_header(compression);
    write_int(
        &mut bytes,
        i32::try_from(entries.len()).expect("test entry count fits i32"),
    );
    let mut payloads = Vec::new();

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
        let offset_start = bytes.len();
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        if let Some(payload) = entry.payload {
            payloads.push((offset_start, entry.id, payload));
        }
    }

    for (offset_start, dump_id, payload) in payloads {
        let offset = u64::try_from(bytes.len()).expect("test archive length fits u64");
        bytes[offset_start..offset_start + 8].copy_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&data_block(BLK_DATA, dump_id, &[payload]));
    }

    bytes
}

fn complete_header(compression: u8) -> Vec<u8> {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 16, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(compression);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut bytes, value);
    }
    write_string(&mut bytes, Some(b"database"));
    write_string(&mut bytes, Some(b"18.4"));
    write_string(&mut bytes, Some(b"18.4"));
    bytes
}

fn data_block(marker: u8, dump_id: i32, chunks: &[&[u8]]) -> Vec<u8> {
    let mut block = vec![marker];
    write_int(&mut block, dump_id);
    for chunk in chunks {
        write_int(
            &mut block,
            i32::try_from(chunk.len()).expect("test chunk length fits i32"),
        );
        block.extend_from_slice(chunk);
    }
    write_int(&mut block, 0);
    block
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
