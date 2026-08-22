use pgdumpx::Limits;

pub const MAX_INPUT_BYTES: usize = 64 * 1024;

const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;
const SECTION_DATA: i32 = 3;

pub fn fuzz_limits() -> Limits {
    Limits::default()
        .with_max_toc_entries(128)
        .with_max_string_bytes(MAX_INPUT_BYTES)
        .with_max_dependencies_per_entry(128)
        .with_max_row_bytes(8 * 1024)
        .with_max_fields_per_row(128)
}

pub fn build_toc_archive(toc_tail: &[u8]) -> Vec<u8> {
    let mut archive = complete_header();
    archive.extend_from_slice(toc_tail);
    archive
}

pub fn build_copy_metadata_archive(statement: &[u8]) -> Vec<u8> {
    build_single_entry_archive(b"TABLE DATA", Some(statement), NO_DATA, 0)
}

pub fn build_data_block_archive(block: &[u8]) -> Vec<u8> {
    let provisional = build_single_entry_archive(b"COMMENT", None, POSITION_SET, 0);
    let offset = u64::try_from(provisional.len()).unwrap_or(u64::MAX);
    let mut archive = build_single_entry_archive(b"COMMENT", None, POSITION_SET, offset);
    archive.extend_from_slice(block);
    archive
}

pub fn build_raw_payload_archive(payload: &[u8]) -> Vec<u8> {
    let mut block = Vec::with_capacity(payload.len().saturating_add(16));
    block.push(1);
    write_int(&mut block, 1);
    write_int(
        &mut block,
        i32::try_from(payload.len()).unwrap_or(i32::MAX),
    );
    block.extend_from_slice(payload);
    write_int(&mut block, 0);
    build_data_block_archive(&block)
}

fn build_single_entry_archive(
    description: &[u8],
    copy_statement: Option<&[u8]>,
    offset_state: u8,
    offset: u64,
) -> Vec<u8> {
    let mut bytes = complete_header();
    write_int(&mut bytes, 1);
    write_int(&mut bytes, 1);
    write_int(&mut bytes, 1);
    write_string(&mut bytes, Some(b"0"));
    write_string(&mut bytes, Some(b"0"));
    write_string(&mut bytes, Some(b"fuzz"));
    write_string(&mut bytes, Some(description));
    write_int(&mut bytes, SECTION_DATA);
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_string(&mut bytes, copy_statement);
    write_string(&mut bytes, Some(b"public"));
    write_string(&mut bytes, None);
    write_string(&mut bytes, None);
    write_int(&mut bytes, 0);
    write_string(&mut bytes, Some(b"postgres"));
    write_string(&mut bytes, Some(b"false"));
    write_string(&mut bytes, None);
    bytes.push(offset_state);
    bytes.extend_from_slice(&offset.to_le_bytes());
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
                i32::try_from(bytes.len()).unwrap_or(i32::MAX),
            );
            output.extend_from_slice(bytes);
        }
        None => write_int(output, -1),
    }
}
