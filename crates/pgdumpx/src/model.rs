use crate::{
    copy_metadata::{Column, TableDataMetadata, TableDataRepresentation},
    error::PgDumpError,
};
use std::str::Utf8Error;

/// The custom-archive format version stored in a PostgreSQL archive header.
///
/// `pgdumpx` v0.1 accepts archive versions 1.14 through 1.16 when opening an archive.
/// [`ArchiveVersion::new`] is a value constructor only; constructing another version
/// does not imply that [`crate::Archive::open`] supports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveVersion {
    major: u8,
    minor: u8,
    revision: u8,
}

impl ArchiveVersion {
    /// Creates a version value from its three on-disk components.
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

/// An owned, byte-oriented archive metadata string.
///
/// PostgreSQL custom-archive strings are not forced through UTF-8 by the core API.
/// [`ArchiveString::as_bytes`] always returns the parsed bytes. Callers that require
/// text can opt into [`ArchiveString::to_str`], which is fallible and does not mutate
/// or normalize the stored value.
///
/// Where the archive grammar permits NULL, surrounding accessors return `Option` so
/// encoded NULL remains distinct from an encoded empty string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ArchiveString(Vec<u8>);

impl ArchiveString {
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns the exact parsed metadata bytes without performing text conversion.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Interprets the metadata as UTF-8 without changing the stored bytes.
    ///
    /// Invalid UTF-8 returns [`Utf8Error`]; it is not a structural archive error at the
    /// byte-oriented metadata layer.
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.0)
    }
}

/// Compression algorithm declared by a supported custom archive header.
///
/// The enum describes archive metadata and does not expose implementation-specific
/// decoder types. LZ4 and Zstandard entry decoding is feature-gated; a recognized
/// archive compression mode whose backend is disabled fails selected-entry access with
/// [`PgDumpError::UnsupportedEntryCompression`].
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

/// PostgreSQL's raw broken-down archive creation timestamp fields.
///
/// These accessors intentionally expose the stored `struct tm`-style components rather
/// than constructing a timezone-aware timestamp or normalizing calendar fields.
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

    /// Returns the stored `tm_sec` value.
    pub const fn second(&self) -> i32 {
        self.second
    }

    /// Returns the stored `tm_min` value.
    pub const fn minute(&self) -> i32 {
        self.minute
    }

    /// Returns the stored `tm_hour` value.
    pub const fn hour(&self) -> i32 {
        self.hour
    }

    /// Returns the stored `tm_mday` value.
    pub const fn day_of_month(&self) -> i32 {
        self.day_of_month
    }

    /// Returns the zero-based stored `tm_mon` value.
    pub const fn month_zero_based(&self) -> i32 {
        self.month_zero_based
    }

    /// Returns the stored `tm_year` value (years since 1900).
    pub const fn year_since_1900(&self) -> i32 {
        self.year_since_1900
    }

    /// Returns the stored `tm_isdst` value.
    pub const fn is_dst(&self) -> i32 {
        self.is_dst
    }
}

/// Metadata parsed from a supported PostgreSQL custom archive header.
///
/// Header metadata is available after [`crate::Archive::open`] without reading or
/// decompressing selected entry bodies. String-valued fields remain byte-oriented via
/// [`ArchiveString`].
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

    /// Returns the archive database name as a byte-oriented string.
    pub const fn database_name(&self) -> &ArchiveString {
        &self.database_name
    }

    /// Returns the dumped server version as a byte-oriented string.
    pub const fn server_version(&self) -> &ArchiveString {
        &self.server_version
    }

    /// Returns the `pg_dump` version as a byte-oriented string.
    pub const fn dump_version(&self) -> &ArchiveString {
        &self.dump_version
    }
}

/// A positive PostgreSQL dump identifier parsed from the archive TOC.
///
/// `DumpId` values obtained from a successfully opened archive are validated positive
/// `i32` values. v0.1 intentionally does not expose a public unchecked constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DumpId(i32);

impl DumpId {
    pub(crate) const fn from_valid(value: i32) -> Self {
        Self(value)
    }

    /// Returns the validated positive integer value stored in the archive.
    pub const fn as_i32(self) -> i32 {
        self.0
    }
}

/// The restore section assigned to a TOC entry by PostgreSQL.
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

/// The custom-archive data-location state associated with a TOC entry.
///
/// The three states preserve PostgreSQL's distinction between explicitly having no data,
/// having no recorded direct-seek position, and having a validated numeric offset value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataLocation {
    /// PostgreSQL explicitly recorded that the TOC entry has no data block.
    NoData,
    /// A data position exists conceptually but no direct-seek offset was recorded.
    Unknown,
    /// The stored absolute archive byte offset.
    Offset(u64),
}

/// Metadata for one archive table-of-contents entry.
///
/// `TocEntry` is parsed eagerly during [`crate::Archive::open`]; its accessors do not
/// decompress the associated payload. Archive strings remain byte-oriented. Optional
/// strings preserve encoded NULL as `None` rather than conflating NULL with an encoded
/// empty value.
///
/// Some TOC fields are version-dependent. In supported archive versions 1.14–1.16,
/// `table_access_method` is part of the parsed layout. [`TocEntry::relation_kind`] is
/// `None` for 1.14/1.15 where no relkind slot exists and `Some(value)` for 1.16,
/// including `Some(0)` when zero was actually encoded.
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

    /// Returns this entry's validated dump identifier.
    pub const fn id(&self) -> DumpId {
        self.id
    }

    /// Returns whether PostgreSQL associated a data dumper with this entry.
    ///
    /// This metadata flag is not itself a promise that a directly readable offset exists;
    /// inspect [`TocEntry::data_location`] or use the selected-entry APIs for that.
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

    /// Returns the exact entry-description/object-type bytes.
    pub fn description_bytes(&self) -> &[u8] {
        self.description.as_bytes()
    }

    /// Returns the entry restore section.
    pub const fn section(&self) -> Section {
        self.section
    }

    /// Returns the optional object definition exactly as stored in the TOC.
    ///
    /// `None` represents encoded NULL; `Some` may contain an empty byte string.
    pub const fn definition(&self) -> Option<&ArchiveString> {
        self.definition.as_ref()
    }

    /// Returns the optional DROP statement exactly as stored in the TOC.
    ///
    /// `None` represents encoded NULL rather than an empty statement.
    pub const fn drop_statement(&self) -> Option<&ArchiveString> {
        self.drop_statement.as_ref()
    }

    /// Returns the optional COPY statement exactly as stored in the TOC.
    ///
    /// The row-aware metadata layer derives COPY column layout/representation from this
    /// value; callers do not need to parse it to use [`TableRef::columns`].
    pub const fn copy_statement(&self) -> Option<&ArchiveString> {
        self.copy_statement.as_ref()
    }

    /// Returns the optional namespace exactly as stored in the TOC.
    pub const fn namespace(&self) -> Option<&ArchiveString> {
        self.namespace.as_ref()
    }

    /// Returns optional namespace bytes, preserving encoded NULL as `None`.
    pub fn namespace_bytes(&self) -> Option<&[u8]> {
        self.namespace.as_ref().map(ArchiveString::as_bytes)
    }

    /// Returns the optional tablespace exactly as stored in the TOC.
    pub const fn tablespace(&self) -> Option<&ArchiveString> {
        self.tablespace.as_ref()
    }

    /// Returns the table access method encoded by supported archive layouts.
    ///
    /// Encoded NULL remains `None`; an encoded empty string remains `Some`.
    pub const fn table_access_method(&self) -> Option<&ArchiveString> {
        self.table_access_method.as_ref()
    }

    /// Returns PostgreSQL's encoded relation-kind value for archive 1.16 entries.
    ///
    /// `None` means the field was not encoded by the archive version (1.14/1.15).
    /// A zero value in a 1.16 archive remains `Some(0)` and is distinct from absence.
    pub const fn relation_kind(&self) -> Option<i32> {
        self.relation_kind
    }

    /// Returns the optional owner exactly as stored in the TOC.
    pub const fn owner(&self) -> Option<&ArchiveString> {
        self.owner.as_ref()
    }

    /// Returns optional owner bytes without conflating NULL with an empty value.
    pub fn owner_bytes(&self) -> Option<&[u8]> {
        self.owner.as_ref().map(ArchiveString::as_bytes)
    }

    /// Returns this entry's validated dependency dump IDs in archive order.
    pub fn dependencies(&self) -> &[DumpId] {
        &self.dependencies
    }

    /// Returns the custom archive data-location state recorded for this entry.
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

/// A metadata-only view of a `TABLE` and its optional related `TABLE DATA` entry.
///
/// This handle borrows metadata parsed and indexed by [`crate::Archive::open`]. Creating
/// or querying it does not seek to or decompress table data. Schema, table, and column
/// identities are byte-oriented.
///
/// Row representation and column-layout queries deliberately distinguish unavailable
/// metadata, malformed COPY metadata, and unsupported table-data representations. Raw
/// selected-entry access remains separate and may still be possible for a representation
/// the row parser does not support.
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

    /// Returns table namespace bytes, or `None` when the `TABLE` namespace was NULL.
    pub fn schema(&self) -> Option<&[u8]> {
        self.table.namespace_bytes()
    }

    /// Returns exact table-name bytes.
    pub fn name(&self) -> &[u8] {
        self.table.name_bytes()
    }

    /// Returns the dump ID of the `TABLE` TOC entry.
    pub const fn table_entry_id(&self) -> DumpId {
        self.table.id()
    }

    /// Returns the dump ID of the related `TABLE DATA` entry, when present.
    ///
    /// `None` means the metadata index found no related table-data entry.
    pub fn data_entry_id(&self) -> Option<DumpId> {
        self.data.map(TocEntry::id)
    }

    /// Returns the table-data representation derived from stored TOC metadata.
    ///
    /// `CopyText`, `Insert`, `Binary`, and other recognized states are values. If the
    /// representation cannot be derived because metadata is absent or malformed, this
    /// returns the corresponding typed metadata error instead.
    pub fn data_representation(&self) -> Result<TableDataRepresentation, PgDumpError> {
        let (data, metadata) = self.require_data_metadata()?;
        metadata.representation(data.id())
    }

    /// Returns COPY columns in the exact positional order used by parsed rows.
    ///
    /// Missing table-data/COPY metadata, malformed COPY statements, and unsupported
    /// table-data representations are reported distinctly. This metadata-only call does
    /// not decompress data. Raw entry access remains available through
    /// [`crate::Archive::entry_reader`] when the selected entry itself is readable.
    pub fn columns(&self) -> Result<&[Column], PgDumpError> {
        let (data, metadata) = self.require_data_metadata()?;
        metadata.columns(data.id())
    }

    /// Resolves an exact byte-oriented COPY column name to its zero-based field index.
    ///
    /// `Ok(Some(index))` means valid metadata contained the name. `Ok(None)` means
    /// metadata was valid but the name was absent. Metadata unavailable/malformed and
    /// unsupported representations are returned as distinct typed errors.
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
