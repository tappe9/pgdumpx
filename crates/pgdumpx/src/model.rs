use crate::{
    copy_metadata::{Column, TableDataMetadata, TableDataRepresentation},
    error::PgDumpError,
};
use std::str::Utf8Error;

/// The archive format version stored in a PostgreSQL custom archive header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveVersion {
    major: u8,
    minor: u8,
    revision: u8,
}

impl ArchiveVersion {
    /// Creates a version from its three on-disk components.
    pub const fn new(major: u8, minor: u8, revision: u8) -> Self {
        Self {
            major,
            minor,
            revision,
        }
    }

    /// Returns the major version component.
    pub const fn major(self) -> u8 {
        self.major
    }

    /// Returns the minor version component.
    pub const fn minor(self) -> u8 {
        self.minor
    }

    /// Returns the revision component.
    pub const fn revision(self) -> u8 {
        self.revision
    }
}

/// A byte-oriented archive metadata string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ArchiveString(Vec<u8>);

impl ArchiveString {
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns the exact metadata bytes stored in the archive.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Interprets the metadata as UTF-8 without changing the stored bytes.
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.0)
    }
}

/// Compression algorithm declared by an archive header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compression {
    /// The entry payload is stored without compression.
    None,
    /// The entry payload uses gzip/zlib compression.
    Gzip,
    /// The entry payload uses LZ4 compression.
    Lz4,
    /// The entry payload uses Zstandard compression.
    Zstd,
}

/// PostgreSQL's raw broken-down archive creation timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveTimestamp {
    second: i32,
    minute: i32,
    hour: i32,
    day_of_month: i32,
    month_zero_based: i32,
    year_since_1900: i32,
    is_dst: i32,
}

impl ArchiveTimestamp {
    pub(crate) const fn new(
        second: i32,
        minute: i32,
        hour: i32,
        day_of_month: i32,
        month_zero_based: i32,
        year_since_1900: i32,
        is_dst: i32,
    ) -> Self {
        Self {
            second,
            minute,
            hour,
            day_of_month,
            month_zero_based,
            year_since_1900,
            is_dst,
        }
    }

    /// Returns the `tm_sec` value stored by PostgreSQL.
    pub const fn second(&self) -> i32 {
        self.second
    }

    /// Returns the `tm_min` value stored by PostgreSQL.
    pub const fn minute(&self) -> i32 {
        self.minute
    }

    /// Returns the `tm_hour` value stored by PostgreSQL.
    pub const fn hour(&self) -> i32 {
        self.hour
    }

    /// Returns the `tm_mday` value stored by PostgreSQL.
    pub const fn day_of_month(&self) -> i32 {
        self.day_of_month
    }

    /// Returns the zero-based `tm_mon` value stored by PostgreSQL.
    pub const fn month_zero_based(&self) -> i32 {
        self.month_zero_based
    }

    /// Returns the `tm_year` value stored by PostgreSQL (years since 1900).
    pub const fn year_since_1900(&self) -> i32 {
        self.year_since_1900
    }

    /// Returns the `tm_isdst` value stored by PostgreSQL.
    pub const fn is_dst(&self) -> i32 {
        self.is_dst
    }
}

/// Metadata parsed from the custom archive header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveHeader {
    version: ArchiveVersion,
    integer_size: u8,
    offset_size: u8,
    compression: Compression,
    created_at: ArchiveTimestamp,
    database_name: ArchiveString,
    server_version: ArchiveString,
    dump_version: ArchiveString,
}

impl ArchiveHeader {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        version: ArchiveVersion,
        integer_size: u8,
        offset_size: u8,
        compression: Compression,
        created_at: ArchiveTimestamp,
        database_name: ArchiveString,
        server_version: ArchiveString,
        dump_version: ArchiveString,
    ) -> Self {
        Self {
            version,
            integer_size,
            offset_size,
            compression,
            created_at,
            database_name,
            server_version,
            dump_version,
        }
    }

    /// Returns the custom archive format version.
    pub const fn version(&self) -> ArchiveVersion {
        self.version
    }

    /// Returns the on-disk archive integer width in bytes.
    pub const fn integer_size(&self) -> u8 {
        self.integer_size
    }

    /// Returns the on-disk file-offset width in bytes.
    pub const fn offset_size(&self) -> u8 {
        self.offset_size
    }

    /// Returns the compression algorithm declared by the header.
    pub const fn compression(&self) -> Compression {
        self.compression
    }

    /// Returns PostgreSQL's raw archive creation timestamp fields.
    pub const fn created_at(&self) -> &ArchiveTimestamp {
        &self.created_at
    }

    /// Returns the archive database name as bytes.
    pub const fn database_name(&self) -> &ArchiveString {
        &self.database_name
    }

    /// Returns the dumped server version as bytes.
    pub const fn server_version(&self) -> &ArchiveString {
        &self.server_version
    }

    /// Returns the `pg_dump` version as bytes.
    pub const fn dump_version(&self) -> &ArchiveString {
        &self.dump_version
    }
}

/// A positive PostgreSQL dump identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DumpId(i32);

impl DumpId {
    pub(crate) const fn from_valid(value: i32) -> Self {
        Self(value)
    }

    /// Returns the integer value stored in the archive.
    pub const fn as_i32(self) -> i32 {
        self.0
    }
}

/// The restore section assigned to a TOC entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Section {
    /// An entry that can occur in any restore section.
    None,
    /// An entry restored before table data.
    PreData,
    /// An entry restored as data.
    Data,
    /// An entry restored after table data.
    PostData,
}

/// The custom archive location state associated with a TOC entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataLocation {
    /// PostgreSQL explicitly recorded that the TOC entry has no data block.
    NoData,
    /// A data position exists conceptually but was not recorded.
    Unknown,
    /// The stored absolute archive byte offset.
    Offset(u64),
}

/// Metadata for one archive table-of-contents entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    id: DumpId,
    has_data: bool,
    catalog_table_oid: ArchiveString,
    catalog_oid: ArchiveString,
    name: ArchiveString,
    description: ArchiveString,
    section: Section,
    definition: Option<ArchiveString>,
    drop_statement: Option<ArchiveString>,
    copy_statement: Option<ArchiveString>,
    namespace: Option<ArchiveString>,
    tablespace: Option<ArchiveString>,
    table_access_method: Option<ArchiveString>,
    relation_kind: Option<i32>,
    owner: Option<ArchiveString>,
    _with_oids: ArchiveString,
    dependencies: Vec<DumpId>,
    data_location: DataLocation,
}

impl TocEntry {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: DumpId,
        has_data: bool,
        catalog_table_oid: ArchiveString,
        catalog_oid: ArchiveString,
        name: ArchiveString,
        description: ArchiveString,
        section: Section,
        definition: Option<ArchiveString>,
        drop_statement: Option<ArchiveString>,
        copy_statement: Option<ArchiveString>,
        namespace: Option<ArchiveString>,
        tablespace: Option<ArchiveString>,
        table_access_method: Option<ArchiveString>,
        relation_kind: Option<i32>,
        owner: Option<ArchiveString>,
        with_oids: ArchiveString,
        dependencies: Vec<DumpId>,
        data_location: DataLocation,
    ) -> Self {
        Self {
            id,
            has_data,
            catalog_table_oid,
            catalog_oid,
            name,
            description,
            section,
            definition,
            drop_statement,
            copy_statement,
            namespace,
            tablespace,
            table_access_method,
            relation_kind,
            owner,
            _with_oids: with_oids,
            dependencies,
            data_location,
        }
    }

    /// Returns this entry's dump identifier.
    pub const fn id(&self) -> DumpId {
        self.id
    }

    /// Returns whether PostgreSQL associated a data dumper with this entry.
    pub const fn has_data(&self) -> bool {
        self.has_data
    }

    /// Returns the catalog table OID exactly as PostgreSQL encoded it.
    pub const fn catalog_table_oid(&self) -> &ArchiveString {
        &self.catalog_table_oid
    }

    /// Returns the catalog object OID exactly as PostgreSQL encoded it.
    pub const fn catalog_oid(&self) -> &ArchiveString {
        &self.catalog_oid
    }

    /// Returns the entry name as a byte-oriented archive string.
    pub const fn name(&self) -> &ArchiveString {
        &self.name
    }

    /// Returns the exact entry-name bytes.
    pub fn name_bytes(&self) -> &[u8] {
        self.name.as_bytes()
    }

    /// Returns the entry description/object type as a byte-oriented archive string.
    pub const fn description(&self) -> &ArchiveString {
        &self.description
    }

    /// Returns the exact entry-description bytes.
    pub fn description_bytes(&self) -> &[u8] {
        self.description.as_bytes()
    }

    /// Returns the entry restore section.
    pub const fn section(&self) -> Section {
        self.section
    }

    /// Returns the optional object definition exactly as stored in the TOC.
    pub const fn definition(&self) -> Option<&ArchiveString> {
        self.definition.as_ref()
    }

    /// Returns the optional DROP statement exactly as stored in the TOC.
    pub const fn drop_statement(&self) -> Option<&ArchiveString> {
        self.drop_statement.as_ref()
    }

    /// Returns the optional COPY statement exactly as stored in the TOC.
    pub const fn copy_statement(&self) -> Option<&ArchiveString> {
        self.copy_statement.as_ref()
    }

    /// Returns the optional namespace exactly as stored in the TOC.
    pub const fn namespace(&self) -> Option<&ArchiveString> {
        self.namespace.as_ref()
    }

    /// Returns the optional namespace bytes.
    pub fn namespace_bytes(&self) -> Option<&[u8]> {
        self.namespace.as_ref().map(ArchiveString::as_bytes)
    }

    /// Returns the optional tablespace exactly as stored in the TOC.
    pub const fn tablespace(&self) -> Option<&ArchiveString> {
        self.tablespace.as_ref()
    }

    /// Returns the table access method when encoded by archive 1.14 and newer.
    pub const fn table_access_method(&self) -> Option<&ArchiveString> {
        self.table_access_method.as_ref()
    }

    /// Returns PostgreSQL's encoded relation-kind value for archive 1.16 entries.
    ///
    /// `None` means the field was not encoded by the archive version. A zero value
    /// in a 1.16 archive remains `Some(0)` and is therefore distinct from absence.
    pub const fn relation_kind(&self) -> Option<i32> {
        self.relation_kind
    }

    /// Returns the optional owner exactly as stored in the TOC.
    pub const fn owner(&self) -> Option<&ArchiveString> {
        self.owner.as_ref()
    }

    /// Returns the optional owner bytes without conflating NULL with an empty value.
    pub fn owner_bytes(&self) -> Option<&[u8]> {
        self.owner.as_ref().map(ArchiveString::as_bytes)
    }

    /// Returns the entry's dependency dump IDs.
    pub fn dependencies(&self) -> &[DumpId] {
        &self.dependencies
    }

    /// Returns the custom archive data-location state.
    pub const fn data_location(&self) -> DataLocation {
        self.data_location
    }

    pub(crate) fn is_table(&self) -> bool {
        self.description.as_bytes() == b"TABLE"
    }

    pub(crate) fn is_table_data(&self) -> bool {
        self.description.as_bytes() == b"TABLE DATA"
    }

    pub(crate) fn copy_statement_bytes(&self) -> Option<&[u8]> {
        self.copy_statement.as_ref().map(ArchiveString::as_bytes)
    }

    pub(crate) fn catalog_table_oid_bytes(&self) -> &[u8] {
        self.catalog_table_oid.as_bytes()
    }

    pub(crate) fn catalog_oid_bytes(&self) -> &[u8] {
        self.catalog_oid.as_bytes()
    }
}

/// A metadata-only view of a table and its optional table-data entry.
#[derive(Debug, Clone, Copy)]
pub struct TableRef<'a> {
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

    /// Returns the table namespace bytes.
    pub fn schema(&self) -> Option<&[u8]> {
        self.table.namespace_bytes()
    }

    /// Returns the table name bytes.
    pub fn name(&self) -> &[u8] {
        self.table.name_bytes()
    }

    /// Returns the dump ID of the `TABLE` entry.
    pub const fn table_entry_id(&self) -> DumpId {
        self.table.id()
    }

    /// Returns the dump ID of the related `TABLE DATA` entry, when present.
    pub fn data_entry_id(&self) -> Option<DumpId> {
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

    fn require_data_metadata(&self) -> Result<(&TocEntry, &TableDataMetadata), PgDumpError> {
        let data = self.data.ok_or(PgDumpError::TableDataEntryUnavailable {
            table_id: self.table.id().as_i32(),
        })?;
        let metadata = self
            .data_metadata
            .ok_or(PgDumpError::CopyColumnMetadataUnavailable {
                dump_id: data.id().as_i32(),
            })?;
        Ok((data, metadata))
    }
}
