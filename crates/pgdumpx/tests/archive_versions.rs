use pgdumpx::{Archive, ArchiveVersion, Compression, Limits, PgDumpError};
use std::{
    io::{Cursor, Read},
    path::PathBuf,
};

const EXPECTED_COPY_STREAM: &[u8] = b"1\tEARLY-100\tcustomer-a\tplain\t\n\
2\tSECOND-200\trepeat\ttab\\tvalue\tfilled\n\
3\tTHIRD-300\tcustomer-c\tline1\\nline2\tfilled\n\
4\tMIDDLE-400\tcustomer-d\t\\N\tfilled\n\
5\tFIFTH-500\tcustomer-e\t\tfilled\n\
6\tSIXTH-600\trepeat\tcarriage\\rreturn\tfilled\n\
7\tLATE-700\tcustomer-g\tbackslash\\\\value\tfilled\n\
\\.\n\n\n";

#[test]
fn official_archive_1_15_none_and_gzip_open_and_stream_selected_entry() {
    for (fixture_name, compression) in [
        ("pg16-none-copy-basic.dump", Compression::None),
        ("pg16-gzip-copy-basic.dump", Compression::Gzip),
    ] {
        assert_version_fixture(fixture_name, ArchiveVersion::new(1, 15, 0), compression);
    }
}

#[test]
fn official_archive_1_14_legacy_none_and_gzip_open_and_stream_selected_entry() {
    for (fixture_name, compression) in [
        ("pg15-none-copy-basic.dump", Compression::None),
        ("pg15-gzip-copy-basic.dump", Compression::Gzip),
    ] {
        assert_version_fixture(fixture_name, ArchiveVersion::new(1, 14, 0), compression);
    }
}

#[test]
fn structural_toc_limit_is_consistent_for_archive_1_14_and_1_15() {
    let limits = Limits::default().with_max_toc_entries(6);

    for fixture_name in ["pg16-none-copy-basic.dump", "pg15-none-copy-basic.dump"] {
        let error = Archive::open_with_limits(Cursor::new(fixture(fixture_name)), limits)
            .expect_err("seven-entry official fixture must exceed a six-entry limit");
        assert!(matches!(
            error,
            PgDumpError::TocEntryLimitExceeded {
                count: 7,
                limit: 6,
                ..
            }
        ));
    }
}

#[test]
fn truncated_archive_1_15_compression_algorithm_is_typed_eof() {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 15, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1); // custom format; compression algorithm byte is missing

    let error = Archive::open(Cursor::new(bytes)).expect_err("truncated algorithm must fail");
    assert!(matches!(error, PgDumpError::UnexpectedEof { offset: 11 }));
}

#[test]
fn truncated_archive_1_14_legacy_compression_integer_is_typed_eof() {
    let mut bytes = b"PGDMP".to_vec();
    bytes.extend_from_slice(&[1, 14, 0]);
    bytes.push(4);
    bytes.push(8);
    bytes.push(1);
    bytes.push(0); // legacy compression integer sign without its four value bytes

    let error = Archive::open(Cursor::new(bytes)).expect_err("truncated legacy level must fail");
    assert!(matches!(error, PgDumpError::UnexpectedEof { .. }));
}

#[test]
fn unsupported_versions_and_revisions_remain_explicit_errors() {
    for version in [[1, 13, 0], [1, 14, 1], [1, 15, 1], [1, 16, 1], [1, 17, 0]] {
        let mut bytes = b"PGDMP".to_vec();
        bytes.extend_from_slice(&version);
        let error = Archive::open(Cursor::new(bytes)).expect_err("version must be rejected");
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

fn assert_version_fixture(
    fixture_name: &str,
    expected_version: ArchiveVersion,
    expected_compression: Compression,
) {
    let mut archive = Archive::open(Cursor::new(fixture(fixture_name)))
        .unwrap_or_else(|error| panic!("{fixture_name} failed to open: {error}"));

    assert_eq!(archive.header().version(), expected_version);
    assert_eq!(archive.header().compression(), expected_compression);
    assert_eq!(archive.header().integer_size(), 4);
    assert_eq!(archive.header().offset_size(), 8);

    let data_id = archive
        .table(b"public", b"orders")
        .expect("orders table must be indexed")
        .data_entry_id()
        .expect("orders table must have TABLE DATA");
    let data_entry = archive
        .entry(data_id)
        .expect("TABLE DATA entry must resolve");
    assert_eq!(data_entry.description_bytes(), b"TABLE DATA");

    let mut reader = archive
        .entry_reader(data_id)
        .expect("selected-entry validation must succeed")
        .expect("selected entry must exist");
    let mut output = Vec::new();
    reader
        .read_to_end(&mut output)
        .expect("selected entry must decompress");
    assert_eq!(output, EXPECTED_COPY_STREAM, "fixture={fixture_name}");
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}
