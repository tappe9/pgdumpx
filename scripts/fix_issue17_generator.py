from pathlib import Path

path = Path(__file__).with_name("apply_issue17.py")
content = path.read_text(encoding="utf-8")
start_marker = '''regex_once(
    "crates/pgdumpx/src/copy.rs",
'''
end_marker = '''replace_once(
    "crates/pgdumpx/src/copy.rs",
    "    limits: CopyParserLimits,\\n",
'''
start = content.find(start_marker)
end = content.find(end_marker, start)
if start < 0 or end < 0:
    raise RuntimeError("could not locate the copy.rs regex replacement block")

replacement = '''replace_once(
    "crates/pgdumpx/src/copy.rs",
    '''\''const PROVISIONAL_MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;
const PROVISIONAL_MAX_FIELDS: u64 = 4 * 1024;
const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;
const COPY_TERMINATOR: &[u8] = b"\\\\.";

const ALPHA1_COPY_LIMITS: CopyParserLimits =
    CopyParserLimits::new(PROVISIONAL_MAX_ROW_BYTES, PROVISIONAL_MAX_FIELDS);

#[derive(Clone, Copy, Debug)]
pub(crate) struct CopyParserLimits {
    max_row_bytes: u64,
    max_fields: u64,
}

impl CopyParserLimits {
    pub(crate) const fn new(max_row_bytes: u64, max_fields: u64) -> Self {
        Self {
            max_row_bytes,
            max_fields,
        }
    }
}
'''\'',
    '''\''const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;
const COPY_TERMINATOR: &[u8] = b"\\\\.";
'''\'',
)
'''
path.write_text(content[:start] + replacement + content[end:], encoding="utf-8")
