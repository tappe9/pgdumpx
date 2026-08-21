from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


archive_path = "crates/pgdumpx/src/archive.rs"
metadata_path = "crates/pgdumpx/src/copy_metadata.rs"
table_rows_path = "crates/pgdumpx/src/table_rows.rs"

archive = read(archive_path)
metadata = read(metadata_path)

# Preserve the historical two-argument parser helper for internal tests while
# making the production archive path explicitly carry caller-supplied limits.
if "fn parse_table_data_metadata_with_limits" not in metadata:
    signature = "pub(crate) fn parse_table_data_metadata(\n"
    if signature not in metadata:
        raise RuntimeError("COPY metadata parser signature was not found")
    metadata = metadata.replace(
        signature,
        "pub(crate) fn parse_table_data_metadata_with_limits(\n",
        1,
    )
    insertion = '''#[cfg(test)]
pub(crate) fn parse_table_data_metadata(
    dump_id: DumpId,
    copy_statement: Option<&[u8]>,
) -> Result<TableDataMetadata, PgDumpError> {
    parse_table_data_metadata_with_limits(dump_id, copy_statement, Limits::default())
}

'''
    marker = "pub(crate) fn parse_table_data_metadata_with_limits(\n"
    metadata = metadata.replace(marker, insertion + marker, 1)

archive = archive.replace(
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata}",
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata_with_limits}",
)
archive = archive.replace(
    "parse_table_data_metadata(data.id(), data.copy_statement_bytes(), limits)?",
    "parse_table_data_metadata_with_limits(data.id(), data.copy_statement_bytes(), limits)?",
)

# Reject the next COPY metadata column before parsing/allocating its identifier.
loop_pattern = re.compile(
    r"(?P<indent>\s*)let column = self\.parse_identifier\((?P<label>[^\n]+)\)\?;\n"
    r"(?P=indent)let actual = columns\n"
    r"(?P=indent)    \.len\(\)\n"
    r"(?P=indent)    \.checked_add\(1\)\n"
    r"(?P=indent)    \.ok_or\(PgDumpError::ArithmeticOverflow \{ offset: 0 \}\)\?;\n"
    r"(?P=indent)let actual_u64 = to_u64\(actual\)\?;\n"
    r"(?P=indent)if actual > self\.max_columns \{\n"
    r"(?P=indent)    return Err\(PgDumpError::CopyColumnCountLimitExceeded \{\n"
    r"(?P=indent)        dump_id: self\.dump_id\.as_i32\(\),\n"
    r"(?P=indent)        limit: to_u64\(self\.max_columns\)\?,\n"
    r"(?P=indent)        actual: actual_u64,\n"
    r"(?P=indent)    \}\n"
    r"(?P=indent)    \.into\(\)\);\n"
    r"(?P=indent)\}\n"
)
match = loop_pattern.search(metadata)
if match:
    indent = match.group("indent")
    label = match.group("label")
    replacement = (
        f"{indent}let actual = columns\n"
        f"{indent}    .len()\n"
        f"{indent}    .checked_add(1)\n"
        f"{indent}    .ok_or(PgDumpError::ArithmeticOverflow {{ offset: 0 }})?;\n"
        f"{indent}let actual_u64 = to_u64(actual)?;\n"
        f"{indent}if actual > self.max_columns {{\n"
        f"{indent}    return Err(PgDumpError::CopyColumnCountLimitExceeded {{\n"
        f"{indent}        dump_id: self.dump_id.as_i32(),\n"
        f"{indent}        limit: to_u64(self.max_columns)?,\n"
        f"{indent}        actual: actual_u64,\n"
        f"{indent}    }}\n"
        f"{indent}    .into());\n"
        f"{indent}}}\n"
        f"{indent}let column = self.parse_identifier({label})?;\n"
    )
    metadata = metadata[: match.start()] + replacement + metadata[match.end() :]

write(metadata_path, metadata)
write(archive_path, archive)

# Retain a default-limits constructor only for direct internal tests; the archive
# production path uses the explicit-limits constructor.
table_rows = read(table_rows_path)
if "fn new_with_limits(" not in table_rows:
    old = '''    pub(crate) fn new(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
        limits: Limits,
    ) -> Self {
'''
    if old in table_rows:
        new = '''    #[cfg(test)]
    pub(crate) fn new(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
    ) -> Self {
        Self::new_with_limits(data_id, metadata, entry, Limits::default())
    }

    pub(crate) fn new_with_limits(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
        limits: Limits,
    ) -> Self {
'''
        table_rows = table_rows.replace(old, new, 1)
        archive = read(archive_path).replace(
            "Ok(TableRowReader::new(\n",
            "Ok(TableRowReader::new_with_limits(\n",
        )
        write(archive_path, archive)
write(table_rows_path, table_rows)

# Update any direct test-only calls that gained an explicit Limits argument.
for path in (ROOT / "crates/pgdumpx/src").glob("*_tests.rs"):
    content = path.read_text(encoding="utf-8")
    if "parse_table_data_metadata_with_limits(" in content:
        content = content.replace(
            "parse_table_data_metadata_with_limits(",
            "parse_table_data_metadata(",
        )
    path.write_text(content, encoding="utf-8")
