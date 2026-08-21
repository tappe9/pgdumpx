from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, content: str) -> None:
    (ROOT / path).write_text(content, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    content = read(path)
    count = content.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one match, found {count}: {old[:80]!r}")
    write(path, content.replace(old, new, 1))


def replace_all(path: str, old: str, new: str, expected: int | None = None) -> None:
    content = read(path)
    count = content.count(old)
    if expected is not None and count != expected:
        raise RuntimeError(f"{path}: expected {expected} matches, found {count}: {old!r}")
    if count == 0:
        raise RuntimeError(f"{path}: no matches: {old!r}")
    write(path, content.replace(old, new))


def regex_once(path: str, pattern: str, replacement: str) -> None:
    content = read(path)
    updated, count = re.subn(pattern, replacement, content, count=1, flags=re.MULTILINE | re.DOTALL)
    if count != 1:
        raise RuntimeError(f"{path}: expected one regex match, found {count}: {pattern[:80]!r}")
    write(path, updated)


write(
    "crates/pgdumpx/src/limits.rs",
    '''/// Finite structural limits applied while opening archives and parsing COPY rows.
///
/// Every field is finite and private. [`Default`] preserves the compatibility-oriented
/// bounds used by the Alpha 1 implementation; callers can derive stricter configurations
/// with the `with_*` methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    max_toc_entries: usize,
    max_string_bytes: usize,
    max_dependencies_per_entry: usize,
    max_row_bytes: usize,
    max_fields_per_row: usize,
}

impl Limits {
    const DEFAULT_MAX_TOC_ENTRIES: usize = 100_000;
    const DEFAULT_MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: usize = 100_000;
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

    /// Returns the maximum number of TOC entries accepted during archive open.
    pub const fn max_toc_entries(self) -> usize {
        self.max_toc_entries
    }

    /// Returns the maximum encoded byte length of one archive metadata string.
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    /// Returns the maximum number of dependencies accepted for one TOC entry.
    pub const fn max_dependencies_per_entry(self) -> usize {
        self.max_dependencies_per_entry
    }

    /// Returns the maximum physical byte length of one COPY text row.
    pub const fn max_row_bytes(self) -> usize {
        self.max_row_bytes
    }

    /// Returns the maximum number of fields accepted in one COPY row and its metadata.
    pub const fn max_fields_per_row(self) -> usize {
        self.max_fields_per_row
    }

    /// Returns a configuration with a different maximum TOC entry count.
    #[must_use]
    pub const fn with_max_toc_entries(mut self, value: usize) -> Self {
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
    pub const fn with_max_dependencies_per_entry(mut self, value: usize) -> Self {
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

replace_once(
    "crates/pgdumpx/src/lib.rs",
    "pub use error::PgDumpError;\n",
    "pub use error::PgDumpError;\npub use limits::Limits;\n",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    "    ArchiveHeader, Compression, DataLocation, DumpId, EntryDataReader, PgDumpError, TableRef,\n    TableRowReader, TocEntry,\n",
    "    ArchiveHeader, Compression, DataLocation, DumpId, EntryDataReader, Limits, PgDumpError,\n    TableRef, TableRowReader, TocEntry,\n",
)
replace_once("crates/pgdumpx/src/archive.rs", "    limits::ALPHA1_METADATA_LIMITS,\n", "")
replace_once(
    "crates/pgdumpx/src/archive.rs",
    "    integer_size: crate::custom::primitives::ArchiveIntegerSize,\n    entries: Vec<TocEntry>,\n",
    "    integer_size: crate::custom::primitives::ArchiveIntegerSize,\n    limits: Limits,\n    entries: Vec<TocEntry>,\n",
)
replace_once(
    "crates/pgdumpx/src/archive.rs",
    '''    /// Opens an exact archive-format 1.16 custom archive and parses metadata only.
    pub fn open(reader: R) -> Result<Self, PgDumpError> {
        let mut reader = ArchiveReader::new(reader);
        let parsed_header = read_header(&mut reader, ALPHA1_METADATA_LIMITS)?;
        let entries = read_toc(
            &mut reader,
            parsed_header.integer_size,
            parsed_header.offset_size,
            ALPHA1_METADATA_LIMITS,
        )?;
        let index = ArchiveIndex::build(&entries)?;

        Ok(Self {
            reader: reader.into_inner(),
            header: parsed_header.header,
            integer_size: parsed_header.integer_size,
            entries,
            index,
        })
    }
''',
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
        let index = ArchiveIndex::build(&entries, limits)?;

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
replace_once(
    "crates/pgdumpx/src/archive.rs",
    "        Ok(TableRowReader::new(data_id, metadata, entry_reader))\n",
    "        Ok(TableRowReader::new(\n            data_id,\n            metadata,\n            entry_reader,\n            self.limits,\n        ))\n",
)
replace_once(
    "crates/pgdumpx/src/archive.rs",
    "    fn build(entries: &[TocEntry]) -> Result<Self, PgDumpError> {\n",
    "    fn build(entries: &[TocEntry], limits: Limits) -> Result<Self, PgDumpError> {\n",
)
replace_once(
    "crates/pgdumpx/src/archive.rs",
    "            let metadata = parse_table_data_metadata(data.id(), data.copy_statement_bytes())?;\n",
    "            let metadata =\n                parse_table_data_metadata(data.id(), data.copy_statement_bytes(), limits)?;\n",
)

replace_once(
    "crates/pgdumpx/src/custom/primitives.rs",
    "use crate::{PgDumpError, io::archive_reader::ArchiveReader, limits::ArchiveStringLimit};\n",
    "use crate::{PgDumpError, io::archive_reader::ArchiveReader};\n",
)
replace_once(
    "crates/pgdumpx/src/custom/primitives.rs",
    "    limit: ArchiveStringLimit,\n",
    "    max_bytes: usize,\n",
)
replace_all(
    "crates/pgdumpx/src/custom/primitives.rs",
    "limit.max_bytes()",
    "max_bytes",
    expected=2,
)

replace_once(
    "crates/pgdumpx/src/custom/header.rs",
    "    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, PgDumpError,\n",
    "    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, Limits,\n    PgDumpError,\n",
)
replace_once("crates/pgdumpx/src/custom/header.rs", "    limits::MetadataLimits,\n", "")
replace_all(
    "crates/pgdumpx/src/custom/header.rs",
    "limits: MetadataLimits",
    "limits: Limits",
    expected=2,
)
replace_all(
    "crates/pgdumpx/src/custom/header.rs",
    "limits.string()",
    "limits.max_string_bytes()",
    expected=1,
)

replace_once(
    "crates/pgdumpx/src/custom/toc.rs",
    "use crate::{\n    PgDumpError,\n",
    "use crate::{\n    Limits, PgDumpError,\n",
)
replace_once("crates/pgdumpx/src/custom/toc.rs", "    limits::MetadataLimits,\n", "")
replace_all(
    "crates/pgdumpx/src/custom/toc.rs",
    "limits: MetadataLimits",
    "limits: Limits",
    expected=5,
)
replace_all(
    "crates/pgdumpx/src/custom/toc.rs",
    "limits.string()",
    "limits.max_string_bytes()",
    expected=2,
)

replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
    "use crate::{\n    error::PgDumpError,\n    model::{ArchiveString, DumpId},\n};\n",
    "use crate::{\n    Limits,\n    error::PgDumpError,\n    model::{ArchiveString, DumpId},\n};\n",
)
replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
    "\nconst PROVISIONAL_MAX_COPY_COLUMNS: u64 = 4 * 1024;\n",
    "",
)
replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
    "    copy_statement: Option<&[u8]>,\n) -> Result<TableDataMetadata, PgDumpError> {\n",
    "    copy_statement: Option<&[u8]>,\n    limits: Limits,\n) -> Result<TableDataMetadata, PgDumpError> {\n",
)
replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
    "    let parsed = match CopyStatementParser::new(statement, dump_id).parse() {\n",
    "    let parsed = match CopyStatementParser::new(\n        statement,\n        dump_id,\n        limits.max_fields_per_row(),\n    )\n    .parse()\n    {\n",
)
replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
    "    dump_id: DumpId,\n}\n\nimpl<'a> CopyStatementParser<'a> {\n    const fn new(input: &'a [u8], dump_id: DumpId) -> Self {\n        Self {\n            input,\n            position: 0,\n            dump_id,\n        }\n    }\n",
    "    dump_id: DumpId,\n    max_columns: usize,\n}\n\nimpl<'a> CopyStatementParser<'a> {\n    const fn new(input: &'a [u8], dump_id: DumpId, max_columns: usize) -> Self {\n        Self {\n            input,\n            position: 0,\n            dump_id,\n            max_columns,\n        }\n    }\n",
)
replace_once(
    "crates/pgdumpx/src/copy_metadata.rs",
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

replace_once(
    "crates/pgdumpx/src/copy.rs",
    "use crate::PgDumpError;\n",
    "use crate::{Limits, PgDumpError};\n",
)
regex_once(
    "crates/pgdumpx/src/copy.rs",
    r"const PROVISIONAL_MAX_ROW_BYTES: u64 = 16 \* 1024 \* 1024;\nconst PROVISIONAL_MAX_FIELDS: u64 = 4 \* 1024;\n(const INITIAL_ROW_CAPACITY_BYTES: usize = 8 \* 1024;\nconst COPY_TERMINATOR: &\[u8\] = b\"\\\\\\\.\";\n)\nconst ALPHA1_COPY_LIMITS: CopyParserLimits =\n    CopyParserLimits::new\(PROVISIONAL_MAX_ROW_BYTES, PROVISIONAL_MAX_FIELDS\);\n\n#\[derive\(Clone, Copy, Debug\)\]\npub\(crate\) struct CopyParserLimits \{.*?\n\}\n\nimpl CopyParserLimits \{.*?\n\}\n",
    r"\1",
)
replace_once(
    "crates/pgdumpx/src/copy.rs",
    "    limits: CopyParserLimits,\n",
    "    limits: Limits,\n",
)
replace_once(
    "crates/pgdumpx/src/copy.rs",
    "    /// Creates a COPY text row reader using provisional finite v0.1 bounds.\n    pub fn new(reader: R) -> Self {\n        Self::with_limits(reader, ALPHA1_COPY_LIMITS)\n    }\n\n    pub(crate) fn with_limits(reader: R, limits: CopyParserLimits) -> Self {\n",
    "    /// Creates a COPY text row reader using finite compatibility-oriented defaults.\n    pub fn new(reader: R) -> Self {\n        Self::with_limits(reader, Limits::default())\n    }\n\n    /// Creates a COPY text row reader using caller-supplied structural limits.\n    pub fn with_limits(reader: R, limits: Limits) -> Self {\n",
)
replace_once(
    "crates/pgdumpx/src/copy.rs",
    "        limits: CopyParserLimits,\n",
    "        limits: Limits,\n",
)
replace_once(
    "crates/pgdumpx/src/copy.rs",
    "        let field_count =\n            inspect_field_layout(&self.raw_row, self.limits.max_fields, row, row_start)?;\n",
    "        let max_fields = u64::try_from(self.limits.max_fields_per_row())\n            .map_err(|_| PgDumpError::ArithmeticOverflow { offset: row_start })?;\n        let field_count = inspect_field_layout(&self.raw_row, max_fields, row, row_start)?;\n",
)
replace_once(
    "crates/pgdumpx/src/copy.rs",
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

replace_once(
    "crates/pgdumpx/src/table_rows.rs",
    "    Column, CopyRowReader, DumpId, EntryDataReader, OwnedRow, PgDumpError, Row,\n",
    "    Column, CopyRowReader, DumpId, EntryDataReader, Limits, OwnedRow, PgDumpError, Row,\n",
)
replace_once(
    "crates/pgdumpx/src/table_rows.rs",
    "        entry: EntryDataReader<'a, R>,\n    ) -> Self {\n",
    "        entry: EntryDataReader<'a, R>,\n        limits: Limits,\n    ) -> Self {\n",
)
replace_once(
    "crates/pgdumpx/src/table_rows.rs",
    "            rows: CopyRowReader::new(entry),\n",
    "            rows: CopyRowReader::with_limits(entry, limits),\n",
)

replace_once(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "    PgDumpError,\n",
    "    Limits, PgDumpError,\n",
)
replace_once(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "    limits::{ALPHA1_ARCHIVE_STRING_LIMIT, ArchiveStringLimit},\n",
    "",
)
replace_all(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "ALPHA1_ARCHIVE_STRING_LIMIT",
    "Limits::default().max_string_bytes()",
    expected=5,
)
replace_all(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "ArchiveStringLimit::new(usize::MAX)",
    "usize::MAX",
    expected=1,
)
replace_all(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "ArchiveStringLimit::new(4)",
    "4",
    expected=1,
)
replace_all(
    "crates/pgdumpx/src/archive_primitives_tests.rs",
    "ArchiveStringLimit::new(3)",
    "3",
    expected=1,
)

replace_once(
    "crates/pgdumpx/src/metadata_open_tests.rs",
    "    limits::{\n        ALPHA1_ARCHIVE_STRING_LIMIT, ALPHA1_METADATA_LIMITS, ArchiveStringLimit, MetadataLimits,\n    },\n",
    "    Limits,\n",
)
replace_once(
    "crates/pgdumpx/src/metadata_open_tests.rs",
    "    let limits = MetadataLimits::new(ArchiveStringLimit::new(1), 100_000, 100_000);\n",
    "    let limits = Limits::default().with_max_string_bytes(1);\n",
)
replace_once(
    "crates/pgdumpx/src/metadata_open_tests.rs",
    "    let limits = MetadataLimits::new(ALPHA1_ARCHIVE_STRING_LIMIT, 0, 100_000);\n",
    "    let limits = Limits::default().with_max_toc_entries(0);\n",
)
replace_once(
    "crates/pgdumpx/src/metadata_open_tests.rs",
    "    let limits = MetadataLimits::new(ALPHA1_ARCHIVE_STRING_LIMIT, 100_000, 0);\n",
    "    let limits = Limits::default().with_max_dependencies_per_entry(0);\n",
)
replace_all(
    "crates/pgdumpx/src/metadata_open_tests.rs",
    "ALPHA1_METADATA_LIMITS",
    "Limits::default()",
    expected=3,
)

replace_once(
    "crates/pgdumpx/src/copy_tests.rs",
    "    PgDumpError,\n    copy::{CopyParserLimits, CopyRowReader, FieldRef},\n",
    "    Limits, PgDumpError,\n    copy::{CopyRowReader, FieldRef},\n",
)
replace_all(
    "crates/pgdumpx/src/copy_tests.rs",
    "CopyParserLimits::new(3, 64)",
    "Limits::default()\n            .with_max_row_bytes(3)\n            .with_max_fields_per_row(64)",
    expected=1,
)
replace_all(
    "crates/pgdumpx/src/copy_tests.rs",
    "CopyParserLimits::new(64, 2)",
    "Limits::default()\n            .with_max_row_bytes(64)\n            .with_max_fields_per_row(2)",
    expected=1,
)
replace_all(
    "crates/pgdumpx/src/copy_tests.rs",
    "CopyParserLimits::new(64, 64)",
    "Limits::default()\n            .with_max_row_bytes(64)\n            .with_max_fields_per_row(64)",
    expected=1,
)

for path in [
    "crates/pgdumpx/src/error.rs",
]:
    content = read(path)
    content = content.replace("provisional finite bound", "configured finite bound")
    content = content.replace("provisional finite column-count bound", "configured finite field-count bound")
    content = content.replace("provisional finite row-byte bound", "configured finite row-byte bound")
    content = content.replace("provisional finite field-count bound", "configured finite field-count bound")
    write(path, content)

for token in [
    "ALPHA1_METADATA_LIMITS",
    "ALPHA1_ARCHIVE_STRING_LIMIT",
    "ArchiveStringLimit",
    "MetadataLimits",
    "CopyParserLimits",
    "PROVISIONAL_MAX_ROW_BYTES",
    "PROVISIONAL_MAX_FIELDS",
    "PROVISIONAL_MAX_COPY_COLUMNS",
]:
    matches = []
    for path in (ROOT / "crates/pgdumpx").rglob("*.rs"):
        if token in path.read_text(encoding="utf-8"):
            matches.append(str(path.relative_to(ROOT)))
    if matches:
        raise RuntimeError(f"provisional token {token!r} remains in {matches}")
