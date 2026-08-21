from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


limits_path = "crates/pgdumpx/src/limits.rs"
limits = read(limits_path)
limits = limits.replace("max_toc_entries: usize,", "max_toc_entries: u64,")
limits = limits.replace(
    "max_dependencies_per_entry: usize,",
    "max_dependencies_per_entry: u64,",
)
limits = limits.replace(
    "const DEFAULT_MAX_TOC_ENTRIES: usize = 100_000;",
    "const DEFAULT_MAX_TOC_ENTRIES: u64 = 100_000;",
)
limits = limits.replace(
    "const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: usize = 100_000;",
    "const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: u64 = 100_000;",
)
limits = limits.replace(
    "pub const fn max_toc_entries(self) -> usize",
    "pub const fn max_toc_entries(self) -> u64",
)
limits = limits.replace(
    "pub const fn max_dependencies_per_entry(self) -> usize",
    "pub const fn max_dependencies_per_entry(self) -> u64",
)
limits = limits.replace(
    "pub const fn with_max_toc_entries(mut self, value: usize)",
    "pub const fn with_max_toc_entries(mut self, value: u64)",
)
limits = limits.replace(
    "pub const fn with_max_dependencies_per_entry(mut self, value: usize)",
    "pub const fn with_max_dependencies_per_entry(mut self, value: u64)",
)
write(limits_path, limits)

archive_path = "crates/pgdumpx/src/archive.rs"
archive = read(archive_path)
metadata_path = "crates/pgdumpx/src/copy_metadata.rs"
metadata = read(metadata_path)

if "fn parse_table_data_metadata_with_limits" not in metadata:
    old = "pub(crate) fn parse_table_data_metadata(\n"
    if old not in metadata:
        raise RuntimeError("COPY metadata parser function was not found")
    metadata = metadata.replace(
        old,
        "pub(crate) fn parse_table_data_metadata_with_limits(\n",
        1,
    )
    wrapper = '''#[cfg(test)]
pub(crate) fn parse_table_data_metadata(
    dump_id: DumpId,
    copy_statement: Option<&[u8]>,
) -> Result<TableDataMetadata, PgDumpError> {
    parse_table_data_metadata_with_limits(dump_id, copy_statement, Limits::default())
}

'''
    metadata = metadata.replace(
        "pub(crate) fn parse_table_data_metadata_with_limits(\n",
        wrapper + "pub(crate) fn parse_table_data_metadata_with_limits(\n",
        1,
    )

archive = archive.replace(
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata}",
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata_with_limits}",
)
archive = archive.replace(
    "parse_table_data_metadata(data.id(), data.copy_statement_bytes(), limits)?",
    "parse_table_data_metadata_with_limits(data.id(), data.copy_statement_bytes(), limits)?",
)

# Move the field-count check before parsing the next identifier when the
# generated implementation still performs it afterwards.
column_block = re.compile(
    r"(?P<i>\s*)let column = self\.parse_identifier\((?P<label>[^\n]+)\)\?;\n"
    r"(?P=i)let actual = columns\n"
    r"(?P=i)    \.len\(\)\n"
    r"(?P=i)    \.checked_add\(1\)\n"
    r"(?P=i)    \.ok_or\(PgDumpError::ArithmeticOverflow \{ offset: 0 \}\)\?;\n"
    r"(?P=i)let actual_u64 = to_u64\(actual\)\?;\n"
    r"(?P=i)if actual > self\.max_columns \{\n"
    r"(?P=i)    return Err\(PgDumpError::CopyColumnCountLimitExceeded \{\n"
    r"(?P=i)        dump_id: self\.dump_id\.as_i32\(\),\n"
    r"(?P=i)        limit: to_u64\(self\.max_columns\)\?,\n"
    r"(?P=i)        actual: actual_u64,\n"
    r"(?P=i)    \}\n"
    r"(?P=i)    \.into\(\)\);\n"
    r"(?P=i)\}\n"
)
match = column_block.search(metadata)
if match:
    i = match.group("i")
    label = match.group("label")
    replacement = (
        f"{i}let actual = columns\n"
        f"{i}    .len()\n"
        f"{i}    .checked_add(1)\n"
        f"{i}    .ok_or(PgDumpError::ArithmeticOverflow {{ offset: 0 }})?;\n"
        f"{i}let actual_u64 = to_u64(actual)?;\n"
        f"{i}if actual > self.max_columns {{\n"
        f"{i}    return Err(PgDumpError::CopyColumnCountLimitExceeded {{\n"
        f"{i}        dump_id: self.dump_id.as_i32(),\n"
        f"{i}        limit: to_u64(self.max_columns)?,\n"
        f"{i}        actual: actual_u64,\n"
        f"{i}    }}\n"
        f"{i}    .into());\n"
        f"{i}}}\n"
        f"{i}let column = self.parse_identifier({label})?;\n"
    )
    metadata = metadata[: match.start()] + replacement + metadata[match.end() :]

write(metadata_path, metadata)
write(archive_path, archive)

# Keep direct internal constructor tests on the finite default path while the
# archive uses its configured limits.
table_rows_path = "crates/pgdumpx/src/table_rows.rs"
table_rows = read(table_rows_path)
if "fn new_with_limits(" not in table_rows:
    pattern = re.compile(
        r"    pub\(crate\) fn new\(\n"
        r"        data_id: DumpId,\n"
        r"        metadata: &'a TableDataMetadata,\n"
        r"        entry: EntryDataReader<'a, R>,\n"
        r"        limits: Limits,\n"
        r"    \) -> Self \{\n"
    )
    if pattern.search(table_rows):
        replacement = '''    #[cfg(test)]
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
        table_rows = pattern.sub(replacement, table_rows, count=1)
        archive = read(archive_path).replace(
            "Ok(TableRowReader::new(\n",
            "Ok(TableRowReader::new_with_limits(\n",
        )
        write(archive_path, archive)
write(table_rows_path, table_rows)

# Preserve direct ArchiveIndex unit tests without weakening the production path.
archive = read(archive_path)
if "fn build_with_limits(entries:" not in archive:
    old = "    fn build(entries: &[TocEntry], limits: Limits) -> Result<Self, PgDumpError> {\n"
    if old in archive:
        replacement = '''    #[cfg(test)]
    fn build(entries: &[TocEntry]) -> Result<Self, PgDumpError> {
        Self::build_with_limits(entries, Limits::default())
    }

    fn build_with_limits(entries: &[TocEntry], limits: Limits) -> Result<Self, PgDumpError> {
'''
        archive = archive.replace(old, replacement, 1)
        archive = archive.replace(
            "let index = ArchiveIndex::build(&entries, limits)?;",
            "let index = ArchiveIndex::build_with_limits(&entries, limits)?;",
        )
write(archive_path, archive)

# Ensure test modules call their default-limits helper rather than the production
# explicit-limits helper.
for path in (ROOT / "crates/pgdumpx/src").glob("*_tests.rs"):
    content = path.read_text(encoding="utf-8")
    content = content.replace(
        "parse_table_data_metadata_with_limits(",
        "parse_table_data_metadata(",
    )
    path.write_text(content, encoding="utf-8")
