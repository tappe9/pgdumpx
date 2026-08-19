use crate::{Archive, FieldRef};
use std::{fs::File, io::BufReader, path::PathBuf};

#[test]
fn integrated_none_and_gzip_paths_preserve_parser_consumed_byte_accounting() {
    let first_physical_row = b"1\tEARLY-100\tcustomer-a\tplain\t\n";

    for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
        let file = File::open(fixture_path(fixture_name)).unwrap();
        let mut archive = Archive::open(BufReader::new(file)).unwrap();
        let mut rows = archive.table_rows(b"public", b"orders").unwrap();

        {
            let row = rows.next_row().unwrap().unwrap();
            assert_eq!(row.field(1), Some(FieldRef::Bytes(b"EARLY-100")));
        }
        assert_eq!(
            rows.consumed_input_bytes(),
            u64::try_from(first_physical_row.len()).unwrap(),
            "decoder or BufRead read-ahead must not affect parser-consumed accounting for {fixture_name}"
        );
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/archives")
        .join(name)
}
