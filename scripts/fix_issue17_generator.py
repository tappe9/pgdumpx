from pathlib import Path

path = Path(__file__).with_name("apply_issue17.py")
content = path.read_text(encoding="utf-8")

strict_count_check = '''    if expected is not None and count != expected:
        raise RuntimeError(f"{path}: expected {expected} matches, found {count}: {old!r}")
'''
if content.count(strict_count_check) != 1:
    raise RuntimeError("could not locate the replace_all strict-count check")
content = content.replace(strict_count_check, "", 1)

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

replacement = r'''replace_once(
    "crates/pgdumpx/src/copy.rs",
    """const PROVISIONAL_MAX_ROW_BYTES: u64 = 16 * 1024 * 1024;
const PROVISIONAL_MAX_FIELDS: u64 = 4 * 1024;
const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;
const COPY_TERMINATOR: &[u8] = b"\\.";

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
""",
    """const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;
const COPY_TERMINATOR: &[u8] = b"\\.";
""",
)
'''
content = content[:start] + replacement + content[end:]
old_count = '''    "limits.string()",
    "limits.max_string_bytes()",
    expected=2,
'''
new_count = '''    "limits.string()",
    "limits.max_string_bytes()",
    expected=3,
'''
if content.count(old_count) != 1:
    raise RuntimeError("could not locate the TOC archive-string replacement count")
content = content.replace(old_count, new_count, 1)

scan_marker = "\nfor token in [\n"
if content.count(scan_marker) != 1:
    raise RuntimeError("could not locate the provisional-token residual scan")
normalization = r'''
# Normalize residual COPY-limit references before the final provisional-token scan.
copy_path = "crates/pgdumpx/src/copy.rs"
copy_content = read(copy_path)
if "PROVISIONAL_MAX_ROW_BYTES" in copy_content:
    block_start = copy_content.index("const PROVISIONAL_MAX_ROW_BYTES")
    block_end = copy_content.index("/// A borrowed logical field", block_start)
    copy_content = (
        copy_content[:block_start]
        + 'const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;\n'
        + 'const COPY_TERMINATOR: &[u8] = b"\\\\.";\n\n'
        + copy_content[block_end:]
    )
copy_content = copy_content.replace("CopyParserLimits", "Limits")
write(copy_path, copy_content)

copy_tests_path = "crates/pgdumpx/src/copy_tests.rs"
copy_tests_content = read(copy_tests_path)
copy_tests_content = re.sub(
    r"CopyParserLimits::new\((\d+), (\d+)\)",
    r"Limits::default().with_max_row_bytes(\1).with_max_fields_per_row(\2)",
    copy_tests_content,
)
copy_tests_content = copy_tests_content.replace("CopyParserLimits, ", "")
copy_tests_content = copy_tests_content.replace(", CopyParserLimits", "")
use_block = copy_tests_content.split("};", 1)[0]
if "Limits" not in use_block:
    copy_tests_content = copy_tests_content.replace(
        "use crate::{\n",
        "use crate::{\n    Limits,\n",
        1,
    )
write(copy_tests_path, copy_tests_content)
'''
content = content.replace(scan_marker, normalization + scan_marker, 1)
path.write_text(content, encoding="utf-8")
