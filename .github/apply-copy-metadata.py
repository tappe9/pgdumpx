from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file_path = Path(path)
    text = file_path.read_text()
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"expected one match in {path}, found {count}")
    file_path.write_text(text.replace(old, new, 1))


replace_once(
    "crates/pgdumpx/src/archive.rs",
    """use crate::{
    ArchiveHeader, DataLocation, DumpId, EntryDataReader, PgDumpError, TableRef, TocEntry,
    custom::{
""",
    """use crate::{
    ArchiveHeader, DataLocation, DumpId, EntryDataReader, PgDumpError, TableRef, TocEntry,
    copy_metadata::{TableDataMetadata, parse_table_data_metadata},
    custom::{
""",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    """        let table_id = self.index.table_id(schema, name)?;
        let table = self.entry(table_id)?;
        let data = self
            .index
            .data_id(table_id)
            .and_then(|data_id| self.entry(data_id));
        Some(TableRef::new(table, data))
""",
    """        let table_id = self.index.table_id(schema, name)?;
        let table = self.entry(table_id)?;
        let data_id = self.index.data_id(table_id);
        let data = data_id.and_then(|id| self.entry(id));
        let data_metadata = data_id.and_then(|id| self.index.table_data_metadata(id));
        Some(TableRef::new(table, data, data_metadata))
""",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    """struct ArchiveIndex {
    by_dump_id: HashMap<DumpId, usize>,
    tables_by_schema: HashMap<Vec<u8>, HashMap<Vec<u8>, DumpId>>,
    table_data_by_table: HashMap<DumpId, DumpId>,
}
""",
    """struct ArchiveIndex {
    by_dump_id: HashMap<DumpId, usize>,
    tables_by_schema: HashMap<Vec<u8>, HashMap<Vec<u8>, DumpId>>,
    table_data_by_table: HashMap<DumpId, DumpId>,
    table_data_metadata: HashMap<DumpId, TableDataMetadata>,
}
""",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    """        for data in entries.iter().filter(|entry| entry.is_table_data()) {
            let mut table_id = None;
""",
    """        let mut table_data_metadata = HashMap::new();
        reserve_map(
            &mut table_data_metadata,
            entries.len(),
            \"table-data metadata index\",
        )?;

        for data in entries.iter().filter(|entry| entry.is_table_data()) {
            let metadata = parse_table_data_metadata(data.id(), data.copy_statement_bytes())?;
            table_data_metadata.insert(data.id(), metadata);

            let mut table_id = None;
""",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    """        Ok(Self {
            by_dump_id,
            tables_by_schema,
            table_data_by_table,
        })
""",
    """        Ok(Self {
            by_dump_id,
            tables_by_schema,
            table_data_by_table,
            table_data_metadata,
        })
""",
)

replace_once(
    "crates/pgdumpx/src/archive.rs",
    """    fn data_id(&self, table_id: DumpId) -> Option<DumpId> {
        self.table_data_by_table.get(&table_id).copied()
    }
}
""",
    """    fn data_id(&self, table_id: DumpId) -> Option<DumpId> {
        self.table_data_by_table.get(&table_id).copied()
    }

    fn table_data_metadata(&self, data_id: DumpId) -> Option<&TableDataMetadata> {
        self.table_data_metadata.get(&data_id)
    }
}
""",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    "use std::str::Utf8Error;\n",
    """use crate::{
    copy_metadata::{Column, TableDataMetadata, TableDataRepresentation},
    error::PgDumpError,
};
use std::str::Utf8Error;
""",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    "    _copy_statement: Option<ArchiveString>,\n",
    "    copy_statement: Option<ArchiveString>,\n",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    "            _copy_statement: copy_statement,\n",
    "            copy_statement,\n",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    """    pub(crate) fn is_table_data(&self) -> bool {
        self.description.as_bytes() == b\"TABLE DATA\"
    }

    pub(crate) fn catalog_table_oid_bytes(&self) -> &[u8] {
""",
    """    pub(crate) fn is_table_data(&self) -> bool {
        self.description.as_bytes() == b\"TABLE DATA\"
    }

    pub(crate) fn copy_statement_bytes(&self) -> Option<&[u8]> {
        self.copy_statement.as_ref().map(ArchiveString::as_bytes)
    }

    pub(crate) fn catalog_table_oid_bytes(&self) -> &[u8] {
""",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    """pub struct TableRef<'a> {
    table: &'a TocEntry,
    data: Option<&'a TocEntry>,
}

impl<'a> TableRef<'a> {
    pub(crate) const fn new(table: &'a TocEntry, data: Option<&'a TocEntry>) -> Self {
        Self { table, data }
    }
""",
    """pub struct TableRef<'a> {
    table: &'a TocEntry,
    data: Option<&'a TocEntry>,
    data_metadata: Option<&'a TableDataMetadata>,
}

impl<'a> TableRef<'a> {
    pub(crate) const fn new(
        table: &'a TocEntry,
        data: Option<&'a TocEntry>,
        data_metadata: Option<&'a TableDataMetadata>,
    ) -> Self {
        Self {
            table,
            data,
            data_metadata,
        }
    }
""",
)

replace_once(
    "crates/pgdumpx/src/model.rs",
    """    pub fn data_entry_id(&self) -> Option<DumpId> {
        self.data.map(TocEntry::id)
    }
}
""",
    """    pub fn data_entry_id(&self) -> Option<DumpId> {
        self.data.map(TocEntry::id)
    }

    /// Returns the table-data representation derived from stored TOC metadata.
    pub fn data_representation(&self) -> Result<TableDataRepresentation, PgDumpError> {
        let (data, metadata) = self.require_data_metadata()?;
        metadata.representation(data.id())
    }

    /// Returns COPY columns in the exact row-field order stored by `pg_dump`.
    ///
    /// Missing metadata, malformed COPY statements, and unsupported table-data
    /// representations are reported distinctly. Raw entry access remains
    /// available through [`crate::Archive::entry_reader`].
    pub fn columns(&self) -> Result<&[Column], PgDumpError> {
        let (data, metadata) = self.require_data_metadata()?;
        metadata.columns(data.id())
    }

    /// Resolves a byte-oriented COPY column name to its zero-based field index.
    pub fn column_index(&self, name: &[u8]) -> Result<Option<usize>, PgDumpError> {
        let (data, metadata) = self.require_data_metadata()?;
        metadata.column_index(data.id(), name)
    }

    fn require_data_metadata(
        &self,
    ) -> Result<(&TocEntry, &TableDataMetadata), PgDumpError> {
        let data = self
            .data
            .ok_or(PgDumpError::TableDataEntryUnavailable {
                table_id: self.table.id().as_i32(),
            })?;
        let metadata = self.data_metadata.ok_or(
            PgDumpError::CopyColumnMetadataUnavailable {
                dump_id: data.id().as_i32(),
            },
        )?;
        Ok((data, metadata))
    }
}
""",
)

replace_once(
    "crates/pgdumpx/src/lib.rs",
    "mod copy;\nmod custom;\n",
    "mod copy;\nmod copy_metadata;\nmod custom;\n",
)

replace_once(
    "crates/pgdumpx/src/lib.rs",
    "pub use copy::{CopyRowReader, FieldRef, Row};\n",
    """pub use copy::{CopyRowReader, FieldRef, Row};
pub use copy_metadata::{Column, TableDataRepresentation};
""",
)

replace_once(
    "crates/pgdumpx/src/error.rs",
    "use std::{error::Error, fmt, io};\n",
    """use crate::copy_metadata::TableDataRepresentation;
use std::{error::Error, fmt, io};
""",
)

replace_once(
    "crates/pgdumpx/src/error.rs",
    """    DuplicateTableDataRelationship {
        table_id: i32,
        first_data_id: i32,
        second_data_id: i32,
    },
    /// The selected TOC entry explicitly has no data block.
""",
    """    DuplicateTableDataRelationship {
        table_id: i32,
        first_data_id: i32,
        second_data_id: i32,
    },
    /// The selected table has no related `TABLE DATA` TOC entry.
    TableDataEntryUnavailable { table_id: i32 },
    /// A `TABLE DATA` entry has no usable COPY statement metadata.
    CopyColumnMetadataUnavailable { dump_id: i32 },
    /// A non-empty COPY statement is outside the supported pg_dump shape.
    MalformedCopyStatement {
        dump_id: i32,
        reason: &'static str,
    },
    /// Row-aware parsing was requested for an unsupported data representation.
    UnsupportedTableDataRepresentation {
        dump_id: i32,
        representation: TableDataRepresentation,
    },
    /// A COPY statement exceeds the provisional finite column-count bound.
    CopyColumnCountLimitExceeded {
        dump_id: i32,
        limit: u64,
        actual: u64,
    },
    /// Memory for bounded COPY column metadata could not be reserved.
    CopyColumnMetadataAllocationFailed {
        dump_id: i32,
        requested: u64,
    },
    /// The selected TOC entry explicitly has no data block.
""",
)

replace_once(
    "crates/pgdumpx/src/error.rs",
    """            Self::DuplicateTableDataRelationship {
                table_id,
                first_data_id,
                second_data_id,
            } => write!(
                formatter,
                \"TABLE dump ID {table_id} is claimed by TABLE DATA dump IDs {first_data_id} and {second_data_id}\"
            ),
            Self::EntryHasNoData { dump_id } => {
""",
    """            Self::DuplicateTableDataRelationship {
                table_id,
                first_data_id,
                second_data_id,
            } => write!(
                formatter,
                \"TABLE dump ID {table_id} is claimed by TABLE DATA dump IDs {first_data_id} and {second_data_id}\"
            ),
            Self::TableDataEntryUnavailable { table_id } => write!(
                formatter,
                \"TABLE dump ID {table_id} has no related TABLE DATA entry\"
            ),
            Self::CopyColumnMetadataUnavailable { dump_id } => write!(
                formatter,
                \"TABLE DATA dump ID {dump_id} has no usable COPY column metadata\"
            ),
            Self::MalformedCopyStatement { dump_id, reason } => write!(
                formatter,
                \"TABLE DATA dump ID {dump_id} has a malformed COPY statement: {reason}\"
            ),
            Self::UnsupportedTableDataRepresentation {
                dump_id,
                representation,
            } => write!(
                formatter,
                \"TABLE DATA dump ID {dump_id} uses unsupported {representation:?} row representation\"
            ),
            Self::CopyColumnCountLimitExceeded {
                dump_id,
                limit,
                actual,
            } => write!(
                formatter,
                \"COPY metadata for dump ID {dump_id} has {actual} columns, exceeding limit {limit}\"
            ),
            Self::CopyColumnMetadataAllocationFailed { dump_id, requested } => write!(
                formatter,
                \"could not reserve {requested} elements or bytes for COPY metadata of dump ID {dump_id}\"
            ),
            Self::EntryHasNoData { dump_id } => {
""",
)
