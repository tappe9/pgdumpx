#[cfg(any(feature = "lz4", feature = "zstd"))]
use pgdumpx::{Archive, EntryReadLimits};
#[cfg(any(feature = "lz4", feature = "zstd"))]
use std::{
    io::{Cursor, Read},
    path::PathBuf,
};

#[cfg(feature = "lz4")]
#[test]
fn lz4_raw_payload_below_limit_streams_completely() {
    assert_payload_below_limit("pg18-lz4-copy-basic.dump");
}

#[cfg(feature = "zstd")]
#[test]
fn zstd_raw_payload_below_limit_streams_completely() {
    assert_payload_below_limit("pg18-zstd-copy-basic.dump");
}

#[cfg(any(feature = "lz4", feature = "zstd"))]
fn assert_payload_below_limit(fixture_name: &str) {
    let bytes = fixture(fixture_name);

    let mut archive = Archive::open(Cursor::new(bytes.clone())).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut unbounded = archive.entry_reader(id).unwrap().unwrap();
    let mut expected = Vec::new();
    unbounded.read_to_end(&mut expected).unwrap();

    let max_decompressed_bytes = u64::try_from(expected.len()).unwrap() + 1;
    let mut archive = Archive::open(Cursor::new(bytes)).unwrap();
    let id = archive
        .table(b"public", b"orders")
        .unwrap()
        .data_entry_id()
        .unwrap();
    let mut bounded = archive
        .entry_reader_with_limits(
            id,
            EntryReadLimits::unlimited().with_max_decompressed_bytes(max_decompressed_bytes),
        )
        .unwrap()
        .unwrap();
    let mut actual = Vec::new();
    bounded.read_to_end(&mut actual).unwrap();

    assert_eq!(actual, expected);
}

#[cfg(any(feature = "lz4", feature = "zstd"))]
fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("official compression fixture must be readable")
}
