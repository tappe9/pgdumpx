use pgdumpx::{Archive, PgDumpError};
use std::io::Cursor;

const SECTION_PRE_DATA: i32 = 2;
const NO_DATA: u8 = 3;

#[test]
fn optional_metadata_preserves_null_empty_and_non_utf8_bytes() {
    let bytes = build_archive([1, 16, 0], 2, |output| {
        write_metadata_entry(output, 1, &[0xff], None, true);
        write_metadata_entry(output, 2, b"empty-owner", Some(b""), true);
    });
    let archive = Archive::open(Cursor::new(bytes)).expect("archive must open");

    let non_utf8 = &archive.entries()[0];
    assert_eq!(non_utf8.name().as_bytes(), &[0xff]);
    assert!(non_utf8.name().to_str().is_err());
    assert!(non_utf8.owner().is_none());
    assert!(non_utf8.owner_bytes().is_none());

    let empty_owner = &archive.entries()[1];
    assert_eq!(
        empty_owner.owner().map(|owner| owner.as_bytes()),
        Some(b"".as_slice())
    );
    assert_eq!(empty_owner.owner_bytes(), Some(b"".as_slice()));
}

#[test]
fn archive_1_15_does_not_consume_a_relkind_slot() {
    let bytes = build_archive([1, 15, 0], 1, |output| {
        write_metadata_entry(output, 1, b"entry", Some(b"owner-after-tableam"), false);
    });
    let archive = Archive::open(Cursor::new(bytes)).expect("1.15 archive must open");
    let entry = &archive.entries()[0];

    assert!(entry.relation_kind().is_none());
    assert_eq!(
        entry.owner().map(|owner| owner.as_bytes()),
        Some(b"owner-after-tableam".as_slice())
    );
}

#[test]
fn truncated_1_16_relkind_returns_a_typed_eof_error() {
    let mut bytes = complete_header([1, 16, 0]);
    write_int(&mut bytes, 1);
    write_toc_prefix_through_tableam(&mut bytes, 1, b"entry");

    let error = Archive::open(Cursor::new(bytes)).expect_err("relkind is required in 1.16");
    assert!(matches!(error, PgDumpError::UnexpectedEof { .. }));
}

fn build_archive(
    version: [u8; 3],
    entry_count: i32,
    write_entries: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut output = complete_header(version);
    write_int(&mut output, entry_count);
    write_entries(&mut output);
    output
}

fn complete_header(version: [u8; 3]) -> Vec<u8> {
    let mut output = b"PGDMP".to_vec();
    output.extend_from_slice(&version);
    output.push(4);
    output.push(8);
    output.push(1);
    output.push(0);
    for value in [0, 0, 0, 1, 0, 126, 0] {
        write_int(&mut output, value);
    }
    write_string(&mut output, Some(b"database"));
    write_string(&mut output, Some(b"server"));
    write_string(&mut output, Some(b"pg_dump"));
    output
}

fn write_metadata_entry(
    output: &mut Vec<u8>,
    id: i32,
    tag: &[u8],
    owner: Option<&[u8]>,
    include_relkind: bool,
) {
    write_toc_prefix_through_tableam(output, id, tag);
    if include_relkind {
        write_int(output, 0);
    }
    write_string(output, owner);
    write_string(output, Some(b"false"));
    write_string(output, None);
    output.push(NO_DATA);
    output.extend_from_slice(&0_u64.to_le_bytes());
}

fn write_toc_prefix_through_tableam(output: &mut Vec<u8>, id: i32, tag: &[u8]) {
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
