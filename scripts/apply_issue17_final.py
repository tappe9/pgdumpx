from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def replace(path: str, old: str, new: str, *, required: bool = True) -> None:
    content = read(path)
    count = content.count(old)
    if required and count == 0:
        raise RuntimeError(f"{path}: required text was not found: {old[:120]!r}")
    write(path, content.replace(old, new))


def sub(path: str, pattern: str, replacement: str, *, required: bool = True) -> None:
    content = read(path)
    updated, count = re.subn(pattern, replacement, content, flags=re.MULTILINE | re.DOTALL)
    if required and count == 0:
        raise RuntimeError(f"{path}: required pattern was not found: {pattern[:120]!r}")
    write(path, updated)


write(
    "crates/pgdumpx/src/limits.rs",
    '''/// Finite structural limits applied while opening archives and parsing COPY rows.
///
/// All fields are private and every configuration is finite. [`Default`] preserves
/// the compatibility-oriented bounds used by the initial v0.1 implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    max_toc_entries: u64,
    max_string_bytes: usize,
    max_dependencies_per_entry: u64,
    max_row_bytes: usize,
    max_fields_per_row: usize,
}

impl Limits {
    const DEFAULT_MAX_TOC_ENTRIES: u64 = 100_000;
    const DEFAULT_MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: u64 = 100_000;
    const DEFAULT_MAX_ROW_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_FIELDS_PER_ROW: usize = 4 * 1024;

    /// Returns compatibility-oriented finite defaults for v0.1 archives.
    pub const fn default_compatible() -> Self {
        Self {
            max_toc_entries: Self::DEFAULT_MAX_TOC_ENTRIES,
            max_string_bytes: Self::DEFAULT_MAX_STRING_BYTES,
            max_dependencies_per_entry: Self::DEFAULT_MAX_DEPENDENCIES_PER_ENTRY,
            max_row_bytes: Self::DEFAULT_MAX_ROW_BYTES,
            max_fields_per_row: Self::DEFAULT_MAX_FIELDS_PER_ROW,
        }
    }

    /// Returns the maximum number of TOC entries accepted while opening an archive.
    pub const fn max_toc_entries(self) -> u64 {
        self.max_toc_entries
    }

    /// Returns the maximum encoded byte length of one archive metadata string.
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    /// Returns the maximum dependency count accepted for one TOC entry.
    pub const fn max_dependencies_per_entry(self) -> u64 {
        self.max_dependencies_per_entry
    }

    /// Returns the maximum physical byte length of one COPY text row.
    pub const fn max_row_bytes(self) -> usize {
        self.max_row_bytes
    }

    /// Returns the maximum field count accepted in one COPY row and its metadata.
    pub const fn max_fields_per_row(self) -> usize {
        self.max_fields_per_row
    }

    /// Returns a configuration with a different maximum TOC entry count.
    #[must_use]
    pub const fn with_max_toc_entries(mut self, value: u64) -> Self {
        self.max_toc_entries = value;
        self
    }

    /// Returns a configuration with a different maximum archive-string byte length.
    #[must_use]
    pub const fn with_max_string_bytes(mut self, value: usize) -> Self {
        self.max_string_bytes = value;
        self
    }

    /// Returns a configuration with a different maximum dependency count per entry.
    #[must_use]
    pub const fn with_max_dependencies_per_entry(mut self, value: u64) -> Self {
        self.max_dependencies_per_entry = value;
        self
    }

    /// Returns a configuration with a different maximum physical COPY row length.
    #[must_use]
    pub const fn with_max_row_bytes(mut self, value: usize) -> Self {
        self.max_row_bytes = value;
        self
    }

    /// Returns a configuration with a different maximum COPY field count.
    #[must_use]
    pub const fn with_max_fields_per_row(mut self, value: usize) -> Self {
        self.max_fields_per_row = value;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::default_compatible()
    }
}
''',
)

replace(
    "crates/pgdumpx/src/lib.rs",
    "pub use error::PgDumpError;\n",
    "pub use error::PgDumpError;\npub use limits::Limits;\n",
)

archive = "crates/pgdumpx/src/archive.rs"
replace(
    archive,
    "    ArchiveHeader, Compression, DataLocation, DumpId, EntryDataReader, PgDumpError, TableRef,\n    TableRowReader, TocEntry,\n",
    "    ArchiveHeader, Compression, DataLocation, DumpId, EntryDataReader, Limits, PgDumpError,\n    TableRef, TableRowReader, TocEntry,\n",
)
replace(archive, "    limits::ALPHA1_METADATA_LIMITS,\n", "")
replace(
    archive,
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata}",
    "copy_metadata::{TableDataMetadata, parse_table_data_metadata_with_limits}",
)
replace(
    archive,
    "    integer_size: crate::custom::primitives::ArchiveIntegerSize,\n    entries: Vec<TocEntry>,\n",
    "    integer_size: crate::custom::primitives::ArchiveIntegerSize,\n    limits: Limits,\n    entries: Vec<TocEntry>,\n",
)
sub(
    archive,
    r"    /// Opens an exact archive-format 1\.16 custom archive and parses metadata only\.\n"
    r"    pub fn open\(reader: R\) -> Result<Self, PgDumpError> \{.*?\n    \}\n"
    r"(?=\n    /// Seeks)",
    '''    /// Opens an exact archive-format 1.16 custom archive with finite default limits.
    pub fn open(reader: R) -> Result<Self, PgDumpError> {
        Self::open_with_limits(reader, Limits::default())
    }

    /// Opens an exact archive-format 1.16 custom archive with caller-supplied limits.
    pub fn open_with_limits(reader: R, limits: Limits) -> Result<Self, PgDumpError> {
        let mut reader = ArchiveReader::new(reader);
        let parsed_header = read_header(&mut reader, limits)?;
        let entries = read_toc(
            &mut reader,
            parsed_header.integer_size,
            parsed_header.offset_size,
            limits,
        )?;
        let index = ArchiveIndex::build_with_limits(&entries, limits)?;

        Ok(Self {
            reader: reader.into_inner(),
            header: parsed_header.header,
            integer_size: parsed_header.integer_size,
            limits,
            entries,
            index,
        })
    }
''',
)
replace(
    archive,
    "        Ok(TableRowReader::new(data_id, metadata, entry_reader))\n",
    "        Ok(TableRowReader::new_with_limits(\n            data_id,\n            metadata,\n            entry_reader,\n            self.limits,\n        ))\n",
)
replace(
    archive,
    "    fn build(entries: &[TocEntry]) -> Result<Self, PgDumpError> {\n",
    '''    #[cfg(test)]
    fn build(entries: &[TocEntry]) -> Result<Self, PgDumpError> {
        Self::build_with_limits(entries, Limits::default())
    }

    fn build_with_limits(entries: &[TocEntry], limits: Limits) -> Result<Self, PgDumpError> {
''',
)
replace(
    archive,
    "            let metadata = parse_table_data_metadata(data.id(), data.copy_statement_bytes())?;\n",
    "            let metadata = parse_table_data_metadata_with_limits(\n                data.id(),\n                data.copy_statement_bytes(),\n                limits,\n            )?;\n",
)

primitives = "crates/pgdumpx/src/custom/primitives.rs"
replace(
    primitives,
    "use crate::{PgDumpError, io::archive_reader::ArchiveReader, limits::ArchiveStringLimit};\n",
    "use crate::{PgDumpError, io::archive_reader::ArchiveReader};\n",
)
replace(primitives, "limit: ArchiveStringLimit", "max_bytes: usize")
replace(primitives, "limit.max_bytes()", "max_bytes")

header = "crates/pgdumpx/src/custom/header.rs"
replace(
    header,
    "    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, PgDumpError,\n",
    "    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, Limits,\n    PgDumpError,\n",
)
replace(header, "    limits::MetadataLimits,\n", "")
replace(header, "MetadataLimits", "Limits")
replace(header, "limits.string()", "limits.max_string_bytes()")

toc = "crates/pgdumpx/src/custom/toc.rs"
replace(toc, "use crate::{\n    PgDumpError,\n", "use crate::{\n    Limits, PgDumpError,\n")
replace(toc, "    limits::MetadataLimits,\n", "")
replace(toc, "MetadataLimits", "Limits")
replace(toc, "limits.string()", "limits.max_string_bytes()")

metadata = "crates/pgdumpx/src/copy_metadata.rs"
replace(metadata, "use crate::{\n", "use crate::{\n    Limits,\n")
replace(metadata, "\nconst PROVISIONAL_MAX_COPY_COLUMNS: u64 = 4 * 1024;\n", "")
replace(
    metadata,
    "pub(crate) fn parse_table_data_metadata(\n",
    '''#[cfg(test)]
pub(crate) fn parse_table_data_metadata(
    dump_id: DumpId,
    copy_statement: Option<&[u8]>,
) -> Result<TableDataMetadata, PgDumpError> {
    parse_table_data_metadata_with_limits(dump_id, copy_statement, Limits::default())
}

pub(crate) fn parse_table_data_metadata_with_limits(
''',
)
replace(
    metadata,
    "    copy_statement: Option<&[u8]>,\n) -> Result<TableDataMetadata, PgDumpError> {\n",
    "    copy_statement: Option<&[u8]>,\n    limits: Limits,\n) -> Result<TableDataMetadata, PgDumpError> {\n",
)
replace(
    metadata,
    "    let parsed = match CopyStatementParser::new(statement, dump_id).parse() {\n",
    "    let parsed = match CopyStatementParser::new(\n        statement,\n        dump_id,\n        limits.max_fields_per_row(),\n    )\n    .parse()\n    {\n",
)
replace(
    metadata,
    "    dump_id: DumpId,\n}\n\nimpl<'a> CopyStatementParser<'a> {\n    const fn new(input: &'a [u8], dump_id: DumpId) -> Self {\n        Self {\n            input,\n            position: 0,\n            dump_id,\n        }\n    }\n",
    "    dump_id: DumpId,\n    max_columns: usize,\n}\n\nimpl<'a> CopyStatementParser<'a> {\n    const fn new(input: &'a [u8], dump_id: DumpId, max_columns: usize) -> Self {\n        Self {\n            input,\n            position: 0,\n            dump_id,\n            max_columns,\n        }\n    }\n",
)
replace(
    metadata,
    '''            let actual_u64 = to_u64(actual)?;
            if actual_u64 > PROVISIONAL_MAX_COPY_COLUMNS {
                return Err(PgDumpError::CopyColumnCountLimitExceeded {
                    dump_id: self.dump_id.as_i32(),
                    limit: PROVISIONAL_MAX_COPY_COLUMNS,
                    actual: actual_u64,
                }
                .into());
            }
''',
    '''            let actual_u64 = to_u64(actual)?;
            if actual > self.max_columns {
                return Err(PgDumpError::CopyColumnCountLimitExceeded {
                    dump_id: self.dump_id.as_i32(),
                    limit: to_u64(self.max_columns)?,
                    actual: actual_u64,
                }
                .into());
            }
''',
)
# Add a pre-parse check in a nested scope so the rejected identifier is never allocated.
metadata_content = read(metadata)
column_marker = "            let column = self.parse_identifier("
marker_index = metadata_content.find(column_marker)
if marker_index < 0:
    raise RuntimeError("COPY column parse marker was not found")
precheck = '''            {
                let actual = columns
                    .len()
                    .checked_add(1)
                    .ok_or(PgDumpError::ArithmeticOverflow { offset: 0 })?;
                if actual > self.max_columns {
                    return Err(PgDumpError::CopyColumnCountLimitExceeded {
                        dump_id: self.dump_id.as_i32(),
                        limit: to_u64(self.max_columns)?,
                        actual: to_u64(actual)?,
                    }
                    .into());
                }
            }
'''
metadata_content = metadata_content[:marker_index] + precheck + metadata_content[marker_index:]
write(metadata, metadata_content)

copy = "crates/pgdumpx/src/copy.rs"
replace(copy, "use crate::PgDumpError;\n", "use crate::{Limits, PgDumpError};\n")
copy_content = read(copy)
block_start = copy_content.find("const PROVISIONAL_MAX_ROW_BYTES")
block_end = copy_content.find("/// A borrowed logical field", block_start)
if block_start < 0 or block_end < 0:
    raise RuntimeError("provisional COPY limit block was not found")
copy_content = (
    copy_content[:block_start]
    + 'const INITIAL_ROW_CAPACITY_BYTES: usize = 8 * 1024;\n'
    + 'const COPY_TERMINATOR: &[u8] = b"\\\\.";\n\n'
    + copy_content[block_end:]
)
copy_content = copy_content.replace("CopyParserLimits", "Limits")
constructor_start = copy_content.find(
    "    /// Creates a COPY text row reader using provisional finite v0.1 bounds."
)
constructor_body = copy_content.find("        Self {", constructor_start)
if constructor_start < 0 or constructor_body < 0:
    raise RuntimeError("COPY reader constructor body was not found")
constructor = '''    /// Creates a COPY text row reader using finite compatibility-oriented defaults.
    pub fn new(reader: R) -> Self {
        Self::with_limits(reader, Limits::default())
    }

    /// Creates a COPY text row reader using caller-supplied structural limits.
    pub fn with_limits(reader: R, limits: Limits) -> Self {
'''
copy_content = copy_content[:constructor_start] + constructor + copy_content[constructor_body:]
copy_content = copy_content.replace(
    "Vec::with_capacity(INITIAL_ROW_CAPACITY_BYTES)",
    "Vec::with_capacity(INITIAL_ROW_CAPACITY_BYTES.min(limits.max_row_bytes()))",
)
write(copy, copy_content)
replace(
    copy,
    "        let field_count =\n            inspect_field_layout(&self.raw_row, self.limits.max_fields, row, row_start)?;\n",
    "        let max_fields = u64::try_from(self.limits.max_fields_per_row())\n            .map_err(|_| PgDumpError::ArithmeticOverflow { offset: row_start })?;\n        let field_count = inspect_field_layout(&self.raw_row, max_fields, row, row_start)?;\n",
)
replace(
    copy,
    '''        if actual > self.limits.max_row_bytes {
            return Err(PgDumpError::CopyRowByteLimitExceeded {
                row,
                limit: self.limits.max_row_bytes,
                actual,
                byte_offset: self.input.consumed(),
            });
        }

        if actual_usize > self.raw_row.capacity() {
            let max_capacity = usize::try_from(self.limits.max_row_bytes).unwrap_or(usize::MAX);
''',
    '''        let max_row_bytes = self.limits.max_row_bytes();
        if actual_usize > max_row_bytes {
            let limit = u64::try_from(max_row_bytes).map_err(|_| {
                PgDumpError::ArithmeticOverflow {
                    offset: self.input.consumed(),
                }
            })?;
            return Err(PgDumpError::CopyRowByteLimitExceeded {
                row,
                limit,
                actual,
                byte_offset: self.input.consumed(),
            });
        }

        if actual_usize > self.raw_row.capacity() {
            let max_capacity = max_row_bytes;
''',
)

table_rows = "crates/pgdumpx/src/table_rows.rs"
replace(
    table_rows,
    "    Column, CopyRowReader, DumpId, EntryDataReader, OwnedRow, PgDumpError, Row,\n",
    "    Column, CopyRowReader, DumpId, EntryDataReader, Limits, OwnedRow, PgDumpError, Row,\n",
)
replace(
    table_rows,
    "    pub(crate) fn new(\n",
    '''    #[cfg(test)]
    pub(crate) fn new(
''',
)
replace(
    table_rows,
    "        entry: EntryDataReader<'a, R>,\n    ) -> Self {\n",
    "        entry: EntryDataReader<'a, R>,\n    ) -> Self {\n        Self::new_with_limits(data_id, metadata, entry, Limits::default())\n    }\n\n    pub(crate) fn new_with_limits(\n        data_id: DumpId,\n        metadata: &'a TableDataMetadata,\n        entry: EntryDataReader<'a, R>,\n        limits: Limits,\n    ) -> Self {\n",
)
replace(
    table_rows,
    "            rows: CopyRowReader::new(entry),\n",
    "            rows: CopyRowReader::with_limits(entry, limits),\n",
)

archive_tests = "crates/pgdumpx/src/archive_primitives_tests.rs"
replace(archive_tests, "    PgDumpError,\n", "    Limits, PgDumpError,\n")
sub(
    archive_tests,
    r"    limits::\{[^\n]*\},\n",
    "",
)
replace(
    archive_tests,
    "ALPHA1_ARCHIVE_STRING_LIMIT",
    "Limits::default().max_string_bytes()",
)
sub(
    archive_tests,
    r"ArchiveStringLimit::new\(([^)]*)\)",
    r"\1",
    required=False,
)

metadata_tests = "crates/pgdumpx/src/metadata_open_tests.rs"
sub(
    metadata_tests,
    r"    limits::\{\n.*?\n    \},\n",
    "    Limits,\n",
)
replace(
    metadata_tests,
    "    let limits = MetadataLimits::new(ArchiveStringLimit::new(1), 100_000, 100_000);\n",
    "    let limits = Limits::default().with_max_string_bytes(1);\n",
)
replace(
    metadata_tests,
    "    let limits = MetadataLimits::new(ALPHA1_ARCHIVE_STRING_LIMIT, 0, 100_000);\n",
    "    let limits = Limits::default().with_max_toc_entries(0);\n",
)
replace(
    metadata_tests,
    "    let limits = MetadataLimits::new(ALPHA1_ARCHIVE_STRING_LIMIT, 100_000, 0);\n",
    "    let limits = Limits::default().with_max_dependencies_per_entry(0);\n",
)
replace(metadata_tests, "ALPHA1_METADATA_LIMITS", "Limits::default()")

copy_tests = "crates/pgdumpx/src/copy_tests.rs"
copy_tests_content = read(copy_tests)
copy_tests_content = copy_tests_content.replace("CopyParserLimits, ", "")
copy_tests_content = copy_tests_content.replace(", CopyParserLimits", "")
copy_tests_content = re.sub(
    r"CopyParserLimits::new\((\d+), (\d+)\)",
    r"Limits::default().with_max_row_bytes(\1).with_max_fields_per_row(\2)",
    copy_tests_content,
)
if "Limits" not in copy_tests_content.split("};", 1)[0]:
    copy_tests_content = copy_tests_content.replace(
        "use crate::{\n",
        "use crate::{\n    Limits,\n",
        1,
    )
write(copy_tests, copy_tests_content)

error = "crates/pgdumpx/src/error.rs"
error_content = read(error)
error_content = error_content.replace("provisional finite bound", "configured finite bound")
error_content = error_content.replace(
    "provisional finite column-count bound",
    "configured finite field-count bound",
)
error_content = error_content.replace(
    "provisional finite row-byte bound",
    "configured finite row-byte bound",
)
error_content = error_content.replace(
    "provisional finite field-count bound",
    "configured finite field-count bound",
)
write(error, error_content)

provisional_tokens = [
    "ALPHA1_METADATA_LIMITS",
    "ALPHA1_ARCHIVE_STRING_LIMIT",
    "ArchiveStringLimit",
    "MetadataLimits",
    "CopyParserLimits",
    "PROVISIONAL_MAX_ROW_BYTES",
    "PROVISIONAL_MAX_FIELDS",
    "PROVISIONAL_MAX_COPY_COLUMNS",
    "ALPHA1_COPY_LIMITS",
]
remaining: dict[str, list[str]] = {}
for token in provisional_tokens:
    paths = []
    for path in (ROOT / "crates/pgdumpx").rglob("*.rs"):
        if token in path.read_text(encoding="utf-8"):
            paths.append(str(path.relative_to(ROOT)))
    if paths:
        remaining[token] = paths
if remaining:
    raise RuntimeError(f"provisional limit tokens remain: {remaining}")
