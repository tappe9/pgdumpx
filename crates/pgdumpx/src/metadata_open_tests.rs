use crate::{
    Archive, Compression, Limits, PgDumpError, TocEntry,
    custom::{
        primitives::{ArchiveIntegerSize, ArchiveOffsetSize},
        toc::read_toc,
    },
    io::archive_reader::ArchiveReader,
};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    rc::Rc,
};

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_PRE_DATA: i32 = 2;
const SECTION_DATA: i32 = 3;
const DEFAULT_MAX_ARCHIVE_STRING_BYTES: usize = 16 * 1024 * 1024;

#[test]
fn opens_zero_entry_archives_and_decodes_known_compression_metadata() {
    let cases = [
        (0, Compression::None),
        (1, Compression::Gzip),
        (2, Compression::Lz4),
        (3, Compression::Zstd),
    ];

    for (algorithm, expected) in cases {
        let archive = Archive::open(Cursor::new(build_archive_with_compression(
            algorithm,
            0,
            |_| {},
        )))
        .unwrap();

        assert_eq!(archive.header().compression(), expected);
        assert!(archive.entries().is_empty());
    }
}

#[test]
fn missing_entry_and_table_lookups_return_none() {
    let archive = Archive::open(Cursor::new(build_archive(1, |output| {
        write_table_entry(output, 1, b"public", b"orders", b"41");
    })))
    .unwrap();
    let other_archive = Archive::open(Cursor::new(build_archive(1, |output| {
        write_metadata_entry(output, 99, b"foreign", &[]);
    })))
    .unwrap();
    let missing_id = other_archive.entries()[0].id();

    assert!(archive.entry(missing_id).is_none());
    assert!(archive.table(b"public", b"missing").is_none());
}

#[test]
fn rejects_truncated_fixed_header_fields() {
    let fixed_header = header_with([1, 16, 0], 1, 0);

    for length in 5..=fixed_header.len() {
        let error = Archive::open(Cursor::new(fixed_header[..length].to_vec())).unwrap_err();
        assert!(matches!(
            error,
            PgDumpError::UnexpectedEof { offset } if offset == u64::try_from(length).unwrap()
        ));
    }
}

#[test]
fn public_open_rejects_oversized_header_string_before_payload_read() {
    let mut bytes = header_with([1, 16, 0], 1, 0);
    write_timestamp(&mut bytes);
    write_int(
        &mut bytes,
        i32::try_from(DEFAULT_MAX_ARCHIVE_STRING_BYTES + 1).unwrap(),
    );
    bytes.extend_from_slice(b"payload-must-not-be-read");

    let bytes_read = Rc::new(Cell::new(0));
    let reader = TrackingReader::new(bytes, Rc::clone(&bytes_read));
    let error = Archive::open(reader).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ArchiveStringLimitExceeded {
            length,
            limit,
            offset: 47,
        } if length == u64::try_from(DEFAULT_MAX_ARCHIVE_STRING_BYTES + 1).unwrap()
            && limit == u64::try_from(DEFAULT_MAX_ARCHIVE_STRING_BYTES).unwrap()
    ));
    assert_eq!(bytes_read.get(), 52);
}

#[test]
fn rejects_catalog_identity_conflicts_in_table_data_relationships() {
    let bytes = build_archive(2, |output| {
        write_table_entry(output, 1, b"public", b"orders", b"41");
        write_table_data_entry(output, 2, b"public", b"orders", b"42", 1);
    });
    let error = Archive::open(Cursor::new(bytes)).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::ConflictingTableDataRelationship {
            table_id: 1,
            data_id: 2,
        }
    ));
}

#[test]
fn toc_entry_limit_accepts_below_and_exact_and_rejects_above() {
    for count in [0_i32, 1] {
        let mut bytes = Vec::new();
        write_int(&mut bytes, count);
        if count == 1 {
            write_metadata_entry(&mut bytes, 1, b"entry", &[]);
        }
        let entries = parse_toc(bytes, metadata_limits(1, 1)).unwrap();

        assert_eq!(entries.len(), usize::try_from(count).unwrap());
    }

    let mut bytes = Vec::new();
    write_int(&mut bytes, 2);
    let error = parse_toc(bytes, metadata_limits(1, 1)).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::TocEntryLimitExceeded {
            count: 2,
            limit: 1,
            offset: 0,
        }
    ));
}

#[test]
fn dependency_limit_accepts_below_and_exact_and_rejects_above() {
    let cases: [&[i32]; 2] = [&[], &[2]];
    for dependencies in cases {
        let mut bytes = Vec::new();
        write_int(&mut bytes, 1);
        write_metadata_entry(&mut bytes, 1, b"entry", dependencies);

        let entries = parse_toc(bytes, metadata_limits(1, 1)).unwrap();

        assert_eq!(entries[0].dependencies().len(), dependencies.len());
    }

    let mut bytes = Vec::new();
    write_int(&mut bytes, 1);
    write_metadata_entry(&mut bytes, 1, b"entry", &[2, 3]);
    let error = parse_toc(bytes, metadata_limits(1, 1)).unwrap_err();

    assert!(matches!(
        error,
        PgDumpError::DependencyLimitExceeded {
            entry_id: 1,
            count: 2,
            limit: 1,
            ..
        }
    ));
}

fn parse_toc(bytes: Vec<u8>, limits: Limits) -> Result<Vec<TocEntry>, PgDumpError> {
    let mut reader = ArchiveReader::new(Cursor::new(bytes));
    read_toc(
        &mut reader,
        ArchiveIntegerSize::new(4, 0).unwrap(),
        ArchiveOffsetSize::new(8, 0).unwrap(),
        limits,
    )
}

fn metadata_limits(max_toc_entries: usize, max_dependencies: usize) -> Limits {
    Limits::default()
        .with_max_string_bytes(1_024)
        .with_max_toc_entries(max_toc_entries)
        .with_max_dependencies_per_entry(max_dependencies)
}

fn build_archive(entry_count: i32, write_entries: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    build_archive_with_compression(0, entry_count, write_entries)
}

fn build_archive_with_compression(
    compression: u8,
    entry_count: i32,
    write_entries: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut output = complete_header(compression);
    write_int(&mut output, entry_count);
    write_entries(&mut output);
    output
}

fn complete_header(compression: u8) -> Vec<u8> {
    let mut output = header_with([1, 16, 0], 1, compression);
    write_timestamp(&mut output);
    write_string(&mut output, Some(b"database"));
    write_string(&mut output, Some(b"18.4"));
    write_string(&mut output, Some(b"18.4"));
    output
}

fn write_timestamp(output: &mut Vec<u8>) {
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(output, value);
    }
}

fn write_metadata_entry(output: &mut Vec<u8>, id: i32, tag: &[u8], dependencies: &[i32]) {
    write_int(output, id);
    write_int(output, 0);
    write_string(output, Some(b"0"));
    write_string(output, Some(b"0"));
    write_string(output, Some(tag));
    write_string(output, Some(b"COMMENT"));
    write_int(output, SECTION_PRE_DATA);
    for _ in 0..6 {
        write_string(output, None);
    }
    write_int(output, 0);
    write_string(output, None);
    write_string(output, Some(b"false"));
    write_dependencies(output, dependencies);
    output.push(NO_DATA);
    output.extend_from_slice(&0_u64.to_le_bytes());
}

fn write_table_entry(output: &mut Vec<u8>, id: i32, schema: &[u8], name: &[u8], oid: &[u8]) {
    write_int(output, id);
    write_int(output, 0);
    write_string(output, Some(b"1259"));
    write_string(output, Some(oid));
    write_string(output, Some(name));
    write_string(output, Some(b"TABLE"));
    write_int(output, SECTION_PRE_DATA);
    write_string(output, Some(b"CREATE TABLE"));
    write_string(output, Some(b"DROP TABLE"));
    write_string(output, None);
    write_string(output, Some(schema));
    write_string(output, Some(b""));
    write_string(output, Some(b"heap"));
    write_int(output, i32::from(b'r'));
    write_string(output, Some(b"postgres"));
    write_string(output, Some(b"false"));
    write_dependencies(output, &[]);
    output.push(NO_DATA);
    output.extend_from_slice(&0_u64.to_le_bytes());
}

fn write_table_data_entry(
    output: &mut Vec<u8>,
    id: i32,
    schema: &[u8],
    name: &[u8],
    oid: &[u8],
    table_id: i32,
) {
    write_int(output, id);
    write_int(output, 1);
    write_string(output, Some(b"0"));
    write_string(output, Some(oid));
    write_string(output, Some(name));
    write_string(output, Some(b"TABLE DATA"));
    write_int(output, SECTION_DATA);
    write_string(output, None);
    write_string(output, None);
    write_string(output, Some(b"COPY table FROM stdin;\n"));
    write_string(output, Some(schema));
    write_string(output, None);
    write_string(output, None);
    write_int(output, 0);
    write_string(output, Some(b"postgres"));
    write_string(output, Some(b"false"));
    write_dependencies(output, &[table_id]);
    output.push(POSITION_SET);
    output.extend_from_slice(&2_048_u64.to_le_bytes());
}

fn write_dependencies(output: &mut Vec<u8>, dependencies: &[i32]) {
    for dependency in dependencies {
        let dependency = dependency.to_string();
        write_string(output, Some(dependency.as_bytes()));
    }
    write_string(output, None);
}

fn header_with(version: [u8; 3], format: u8, compression: u8) -> Vec<u8> {
    let mut output = b"PGDMP".to_vec();
    output.extend_from_slice(&version);
    output.push(4);
    output.push(8);
    output.push(format);
    output.push(compression);
    output
}

fn write_int(output: &mut Vec<u8>, value: i32) {
    output.push(u8::from(value.is_negative()));
    output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
}

fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            write_int(output, i32::try_from(bytes.len()).unwrap());
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
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
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(output)?;
        let read = u64::try_from(read).unwrap();
        self.bytes_read.set(self.bytes_read.get() + read);
        usize::try_from(read).map_err(io::Error::other)
    }
}

impl Seek for TrackingReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
