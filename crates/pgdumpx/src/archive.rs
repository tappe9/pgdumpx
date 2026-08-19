use crate::{
    ArchiveHeader, Compression, DataLocation, DumpId, EntryDataReader, PgDumpError, TableRef,
    TableRowReader, TocEntry,
    copy_metadata::{TableDataMetadata, parse_table_data_metadata},
    custom::{
        data::{BLK_DATA, CustomChunkReader},
        header::read_header,
        primitives::read_archive_integer,
        toc::read_toc,
    },
    io::archive_reader::ArchiveReader,
    limits::ALPHA1_METADATA_LIMITS,
};
use std::{
    collections::{HashMap, hash_map::Entry},
    io::{Read, Seek, SeekFrom},
};

/// A read-only PostgreSQL custom-format archive with parsed metadata.
#[derive(Debug)]
pub struct Archive<R> {
    reader: R,
    header: ArchiveHeader,
    integer_size: crate::custom::primitives::ArchiveIntegerSize,
    entries: Vec<TocEntry>,
    index: ArchiveIndex,
}

impl<R: Read + Seek> Archive<R> {
    /// Opens an exact archive-format 1.16 custom archive and parses metadata only.
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

    /// Seeks to and opens one validated TOC entry as a streaming decompressed reader.
    ///
    /// The recorded data offset, custom block type, and dump ID are validated before
    /// any payload bytes are exposed. `Ok(None)` means the dump ID is not present.
    pub fn entry_reader(
        &mut self,
        id: DumpId,
    ) -> Result<Option<EntryDataReader<'_, R>>, PgDumpError> {
        let Some(index) = self.index.entry_index(id) else {
            return Ok(None);
        };
        let entry = &self.entries[index];
        open_entry_reader(
            &mut self.reader,
            self.header.compression(),
            self.integer_size,
            entry,
        )
        .map(Some)
    }

    /// Opens the related table-data entry as a lending COPY text row stream.
    ///
    /// Table lookup and representation validation use metadata parsed during
    /// [`Archive::open`]. Unsupported row representations fail before the
    /// selected payload is sought or parsed.
    pub fn table_rows(
        &mut self,
        schema: &[u8],
        name: &[u8],
    ) -> Result<TableRowReader<'_, R>, PgDumpError> {
        let table_id = self
            .index
            .table_id(schema, name)
            .ok_or(PgDumpError::TableNotFound)?;
        let data_id =
            self.index
                .data_id(table_id)
                .ok_or(PgDumpError::TableDataEntryUnavailable {
                    table_id: table_id.as_i32(),
                })?;
        let metadata = self.index.table_data_metadata(data_id).ok_or(
            PgDumpError::CopyColumnMetadataUnavailable {
                dump_id: data_id.as_i32(),
            },
        )?;
        metadata.validate_row_access(data_id)?;

        let entry_index =
            self.index
                .entry_index(data_id)
                .ok_or(PgDumpError::TableDataEntryUnavailable {
                    table_id: table_id.as_i32(),
                })?;
        let entry = &self.entries[entry_index];
        let entry_reader = open_entry_reader(
            &mut self.reader,
            self.header.compression(),
            self.integer_size,
            entry,
        )?;
        Ok(TableRowReader::new(data_id, metadata, entry_reader))
    }
}

fn open_entry_reader<'a, R: Read + Seek>(
    reader: &'a mut R,
    compression: Compression,
    integer_size: crate::custom::primitives::ArchiveIntegerSize,
    entry: &TocEntry,
) -> Result<EntryDataReader<'a, R>, PgDumpError> {
    let id = entry.id();
    let dump_id = id.as_i32();
    let offset = match entry.data_location() {
        DataLocation::NoData => return Err(PgDumpError::EntryHasNoData { dump_id }),
        DataLocation::Unknown => {
            return Err(PgDumpError::EntryDataOffsetUnavailable { dump_id });
        }
        DataLocation::Offset(offset) => offset,
    };

    let encoded_dump_id_bytes = u64::from(integer_size.get())
        .checked_add(1)
        .ok_or(PgDumpError::InvalidDataOffset { dump_id, offset })?;
    offset
        .checked_add(1)
        .and_then(|value| value.checked_add(encoded_dump_id_bytes))
        .ok_or(PgDumpError::InvalidDataOffset { dump_id, offset })?;

    let actual_position = reader
        .seek(SeekFrom::Start(offset))
        .map_err(|source| PgDumpError::Io { offset, source })?;
    if actual_position != offset {
        return Err(PgDumpError::EntrySeekPositionMismatch {
            dump_id,
            expected: offset,
            actual: actual_position,
        });
    }

    let mut reader = ArchiveReader::new_at(reader, offset);
    let marker_offset = reader.offset();
    let block_type = reader.read_byte()?;
    if block_type != BLK_DATA {
        return Err(PgDumpError::UnexpectedDataBlockType {
            dump_id,
            expected: BLK_DATA,
            actual: block_type,
            offset: marker_offset,
        });
    }

    let dump_id_offset = reader.offset();
    let actual_dump_id = read_archive_integer(&mut reader, integer_size)?;
    if actual_dump_id != dump_id {
        return Err(PgDumpError::DataBlockDumpIdMismatch {
            expected: dump_id,
            actual: actual_dump_id,
            offset: dump_id_offset,
        });
    }

    let chunks = CustomChunkReader::new(reader, integer_size, id);
    EntryDataReader::new(id, compression, chunks)
}

impl<R> Archive<R> {
    /// Returns parsed archive-header metadata.
    pub const fn header(&self) -> &ArchiveHeader {
        &self.header
    }

    /// Returns all parsed TOC entries in archive order.
    pub fn entries(&self) -> &[TocEntry] {
        &self.entries
    }

    /// Resolves one TOC entry by dump ID.
    pub fn entry(&self, id: DumpId) -> Option<&TocEntry> {
        self.index
            .entry_index(id)
            .and_then(|index| self.entries.get(index))
    }

    /// Resolves a table by exact byte-oriented schema and name.
    pub fn table(&self, schema: &[u8], name: &[u8]) -> Option<TableRef<'_>> {
        let table_id = self.index.table_id(schema, name)?;
        let table = self.entry(table_id)?;
        let data_id = self.index.data_id(table_id);
        let data = data_id.and_then(|id| self.entry(id));
        let data_metadata = data_id.and_then(|id| self.index.table_data_metadata(id));
        Some(TableRef::new(table, data, data_metadata))
    }
}

#[derive(Debug)]
struct ArchiveIndex {
    by_dump_id: HashMap<DumpId, usize>,
    tables_by_schema: HashMap<Vec<u8>, HashMap<Vec<u8>, DumpId>>,
    table_data_by_table: HashMap<DumpId, DumpId>,
    table_data_metadata: HashMap<DumpId, TableDataMetadata>,
}

impl ArchiveIndex {
    fn build(entries: &[TocEntry]) -> Result<Self, PgDumpError> {
        let mut by_dump_id = HashMap::new();
        reserve_map(&mut by_dump_id, entries.len(), "dump-ID index")?;
        for (index, entry) in entries.iter().enumerate() {
            if by_dump_id.insert(entry.id(), index).is_some() {
                return Err(PgDumpError::DuplicateDumpId {
                    dump_id: entry.id().as_i32(),
                });
            }
        }

        let mut tables_by_schema = HashMap::new();
        reserve_map(&mut tables_by_schema, entries.len(), "table schema index")?;
        let mut tables_without_schema = HashMap::new();
        reserve_map(
            &mut tables_without_schema,
            entries.len(),
            "schema-less table index",
        )?;

        for entry in entries.iter().filter(|entry| entry.is_table()) {
            match entry.namespace_bytes() {
                Some(schema) => insert_table_with_schema(
                    &mut tables_by_schema,
                    schema,
                    entry.name_bytes(),
                    entry.id(),
                )?,
                None => insert_schema_less_table(
                    &mut tables_without_schema,
                    entry.name_bytes(),
                    entry.id(),
                )?,
            }
        }

        let mut table_data_by_table = HashMap::new();
        reserve_map(
            &mut table_data_by_table,
            entries.len(),
            "table-data relationship index",
        )?;

        let mut table_data_metadata = HashMap::new();
        reserve_map(
            &mut table_data_metadata,
            entries.len(),
            "table-data metadata index",
        )?;

        for data in entries.iter().filter(|entry| entry.is_table_data()) {
            let metadata = parse_table_data_metadata(data.id(), data.copy_statement_bytes())?;
            table_data_metadata.insert(data.id(), metadata);

            let mut table_id = None;
            for dependency in data.dependencies() {
                let Some(index) = by_dump_id.get(dependency).copied() else {
                    continue;
                };
                if !entries[index].is_table() {
                    continue;
                }

                match table_id {
                    None => table_id = Some(*dependency),
                    Some(existing) if existing == *dependency => {}
                    Some(_) => {
                        return Err(PgDumpError::AmbiguousTableDataRelationship {
                            data_id: data.id().as_i32(),
                        });
                    }
                }
            }

            let Some(table_id) = table_id else {
                continue;
            };
            let Some(table_index) = by_dump_id.get(&table_id).copied() else {
                continue;
            };
            let table = &entries[table_index];
            if !relationship_metadata_matches(table, data) {
                return Err(PgDumpError::ConflictingTableDataRelationship {
                    table_id: table_id.as_i32(),
                    data_id: data.id().as_i32(),
                });
            }

            if let Some(previous) = table_data_by_table.insert(table_id, data.id()) {
                return Err(PgDumpError::DuplicateTableDataRelationship {
                    table_id: table_id.as_i32(),
                    first_data_id: previous.as_i32(),
                    second_data_id: data.id().as_i32(),
                });
            }
        }

        Ok(Self {
            by_dump_id,
            tables_by_schema,
            table_data_by_table,
            table_data_metadata,
        })
    }

    fn entry_index(&self, id: DumpId) -> Option<usize> {
        self.by_dump_id.get(&id).copied()
    }

    fn table_id(&self, schema: &[u8], name: &[u8]) -> Option<DumpId> {
        self.tables_by_schema
            .get(schema)
            .and_then(|tables| tables.get(name))
            .copied()
    }

    fn data_id(&self, table_id: DumpId) -> Option<DumpId> {
        self.table_data_by_table.get(&table_id).copied()
    }

    fn table_data_metadata(&self, data_id: DumpId) -> Option<&TableDataMetadata> {
        self.table_data_metadata.get(&data_id)
    }
}

fn insert_table_with_schema(
    schemas: &mut HashMap<Vec<u8>, HashMap<Vec<u8>, DumpId>>,
    schema: &[u8],
    name: &[u8],
    table_id: DumpId,
) -> Result<(), PgDumpError> {
    match schemas.entry(clone_index_bytes(schema, "table schema key")?) {
        Entry::Occupied(mut occupied) => {
            let tables = occupied.get_mut();
            tables
                .try_reserve(1)
                .map_err(|_| PgDumpError::ArchiveIndexAllocationFailed {
                    context: "table name index",
                    requested: 1,
                })?;
            if let Some(previous) =
                tables.insert(clone_index_bytes(name, "table name key")?, table_id)
            {
                return Err(PgDumpError::DuplicateTableIdentity {
                    first_table_id: previous.as_i32(),
                    second_table_id: table_id.as_i32(),
                });
            }
        }
        Entry::Vacant(vacant) => {
            let mut tables = HashMap::new();
            tables
                .try_reserve(1)
                .map_err(|_| PgDumpError::ArchiveIndexAllocationFailed {
                    context: "table name index",
                    requested: 1,
                })?;
            tables.insert(clone_index_bytes(name, "table name key")?, table_id);
            vacant.insert(tables);
        }
    }

    Ok(())
}

fn insert_schema_less_table(
    tables: &mut HashMap<Vec<u8>, DumpId>,
    name: &[u8],
    table_id: DumpId,
) -> Result<(), PgDumpError> {
    if let Some(previous) = tables.insert(
        clone_index_bytes(name, "schema-less table name key")?,
        table_id,
    ) {
        return Err(PgDumpError::DuplicateTableIdentity {
            first_table_id: previous.as_i32(),
            second_table_id: table_id.as_i32(),
        });
    }
    Ok(())
}

fn relationship_metadata_matches(table: &TocEntry, data: &TocEntry) -> bool {
    table.namespace_bytes() == data.namespace_bytes()
        && table.name_bytes() == data.name_bytes()
        && catalog_oids_compatible(
            table.catalog_table_oid_bytes(),
            data.catalog_table_oid_bytes(),
        )
        && catalog_oids_compatible(table.catalog_oid_bytes(), data.catalog_oid_bytes())
}

fn catalog_oids_compatible(table_oid: &[u8], data_oid: &[u8]) -> bool {
    table_oid == data_oid || table_oid == b"0" || data_oid == b"0"
}

fn clone_index_bytes(bytes: &[u8], context: &'static str) -> Result<Vec<u8>, PgDumpError> {
    let requested =
        u64::try_from(bytes.len()).map_err(|_| PgDumpError::ArithmeticOverflow { offset: 0 })?;
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| PgDumpError::ArchiveIndexAllocationFailed { context, requested })?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

fn reserve_map<K: Eq + std::hash::Hash, V>(
    map: &mut HashMap<K, V>,
    requested: usize,
    context: &'static str,
) -> Result<(), PgDumpError> {
    let requested_u64 =
        u64::try_from(requested).map_err(|_| PgDumpError::ArithmeticOverflow { offset: 0 })?;
    map.try_reserve(requested)
        .map_err(|_| PgDumpError::ArchiveIndexAllocationFailed {
            context,
            requested: requested_u64,
        })
}
