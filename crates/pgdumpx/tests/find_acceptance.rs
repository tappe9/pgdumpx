use pgdumpx::{Archive, FieldRef, OwnedField};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
};

#[test]
fn official_none_and_gzip_rows_are_invariant_under_one_byte_source_reads() {
    let expected_ids = [
        b"1".as_slice(),
        b"2".as_slice(),
        b"3".as_slice(),
        b"4".as_slice(),
        b"5".as_slice(),
        b"6".as_slice(),
        b"7".as_slice(),
    ];

    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let reader = OneByteReader::new(fixture(fixture_name));
        let mut archive = Archive::open(reader).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        let mut actual_ids = Vec::new();

        while let Some(row) = rows.next_row().unwrap() {
            let Some(FieldRef::Bytes(id)) = row.field(0) else {
                panic!("official fixture row must have a non-NULL order_id");
            };
            actual_ids.push(id.to_vec());
        }

        assert_eq!(
            actual_ids,
            expected_ids.map(<[u8]>::to_vec),
            "fixture {fixture_name}"
        );
    }
}

#[test]
fn find_first_evaluates_each_row_once_and_never_after_the_first_match() {
    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let mut archive = Archive::open(Cursor::new(fixture(fixture_name))).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();
        let order_number = rows.column_index(b"order_number").unwrap().unwrap();
        let predicate_calls = Cell::new(0_u64);

        let found = rows
            .find_first(|row| {
                predicate_calls.set(predicate_calls.get() + 1);
                row.field(order_number) == Some(FieldRef::Bytes(b"MIDDLE-400"))
            })
            .unwrap()
            .unwrap();

        assert_eq!(found.field(0), Some(&OwnedField::Bytes(b"4".to_vec())));
        assert_eq!(
            predicate_calls.get(),
            4,
            "predicate must run once for rows 1-4 and never for later rows in {fixture_name}"
        );
    }
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name);
    std::fs::read(path).expect("committed official fixture must be readable")
}

#[derive(Debug)]
struct OneByteReader {
    inner: Cursor<Vec<u8>>,
}

impl OneByteReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            inner: Cursor::new(bytes),
        }
    }
}

impl Read for OneByteReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.inner.read(&mut output[..1])
    }
}

impl Seek for OneByteReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
