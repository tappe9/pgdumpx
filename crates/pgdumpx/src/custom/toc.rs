use crate::{
    ArchiveVersion, Limits, PgDumpError,
    custom::primitives::{
        ArchiveIntegerSize, ArchiveOffset, ArchiveOffsetSize, read_archive_integer,
        read_archive_offset, read_archive_string, read_retained_archive_string,
    },
    io::archive_reader::ArchiveReader,
    metadata_budget::MetadataBudget,
    model::{ArchiveString, DataLocation, DumpId, Section, TocEntry},
};
use std::io::Read;

const ARCHIVE_VERSION_1_14: ArchiveVersion = ArchiveVersion::new(1, 14, 0);
const ARCHIVE_VERSION_1_16: ArchiveVersion = ArchiveVersion::new(1, 16, 0);

#[cfg(test)]
pub(crate) fn read_toc<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    offset_size: ArchiveOffsetSize,
    limits: Limits,
) -> Result<Vec<TocEntry>, PgDumpError> {
    read_toc_for_version(
        reader,
        ARCHIVE_VERSION_1_16,
        integer_size,
        offset_size,
        limits,
    )
}

#[cfg(test)]
pub(crate) fn read_toc_for_version<R: Read>(
    reader: &mut ArchiveReader<R>,
    version: ArchiveVersion,
    integer_size: ArchiveIntegerSize,
    offset_size: ArchiveOffsetSize,
    limits: Limits,
) -> Result<Vec<TocEntry>, PgDumpError> {
    let mut budget = MetadataBudget::new(limits)?;
    read_toc_for_version_with_budget(
        reader,
        version,
        integer_size,
        offset_size,
        limits,
        &mut budget,
    )
}

pub(crate) fn read_toc_for_version_with_budget<R: Read>(
    reader: &mut ArchiveReader<R>,
    version: ArchiveVersion,
    integer_size: ArchiveIntegerSize,
    offset_size: ArchiveOffsetSize,
    limits: Limits,
    budget: &mut MetadataBudget,
) -> Result<Vec<TocEntry>, PgDumpError> {
    let count_offset = reader.offset();
    let encoded_count = read_archive_integer(reader, integer_size)?;
    if encoded_count < 0 {
        return Err(PgDumpError::InvalidTocEntryCount {
            value: encoded_count,
            offset: count_offset,
        });
    }

    let count = usize::try_from(encoded_count).map_err(|_| PgDumpError::ArithmeticOverflow {
        offset: count_offset,
    })?;
    if count > limits.max_toc_entries() {
        return Err(PgDumpError::TocEntryLimitExceeded {
            count: to_u64(count, count_offset)?,
            limit: to_u64(limits.max_toc_entries(), count_offset)?,
            offset: count_offset,
        });
    }

    let count_u64 = to_u64(count, count_offset)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| PgDumpError::TocAllocationFailed {
            count: count_u64,
            offset: count_offset,
        })?;

    for _ in 0..count {
        entries.push(read_toc_entry(
            reader,
            version,
            integer_size,
            offset_size,
            limits,
            budget,
        )?);
    }

    Ok(entries)
}

fn read_toc_entry<R: Read>(
    reader: &mut ArchiveReader<R>,
    version: ArchiveVersion,
    integer_size: ArchiveIntegerSize,
    offset_size: ArchiveOffsetSize,
    limits: Limits,
    budget: &mut MetadataBudget,
) -> Result<TocEntry, PgDumpError> {
    let id_offset = reader.offset();
    let id_value = read_archive_integer(reader, integer_size)?;
    if id_value <= 0 {
        return Err(PgDumpError::InvalidDumpId {
            value: id_value,
            offset: id_offset,
        });
    }
    let id = DumpId::from_valid(id_value);

    let has_data = read_archive_integer(reader, integer_size)? != 0;
    let catalog_table_oid = read_required_string(
        reader,
        integer_size,
        limits,
        budget,
        "TOC catalog table OID",
    )?;
    let catalog_oid = read_required_string(
        reader,
        integer_size,
        limits,
        budget,
        "TOC catalog object OID",
    )?;
    let name = read_required_string(reader, integer_size, limits, budget, "TOC tag")?;
    let description =
        read_required_string(reader, integer_size, limits, budget, "TOC description")?;

    let section_offset = reader.offset();
    let section_value = read_archive_integer(reader, integer_size)?;
    let section = match section_value {
        1 => Section::None,
        2 => Section::PreData,
        3 => Section::Data,
        4 => Section::PostData,
        _ => {
            return Err(PgDumpError::InvalidSection {
                value: section_value,
                entry_id: id.as_i32(),
                offset: section_offset,
            });
        }
    };

    let definition = read_optional_string(reader, integer_size, limits, budget)?;
    let drop_statement = read_optional_string(reader, integer_size, limits, budget)?;
    let copy_statement = read_optional_string(reader, integer_size, limits, budget)?;
    let namespace = read_optional_string(reader, integer_size, limits, budget)?;
    let tablespace = read_optional_string(reader, integer_size, limits, budget)?;
    let table_access_method = if version >= ARCHIVE_VERSION_1_14 {
        read_optional_string(reader, integer_size, limits, budget)?
    } else {
        None
    };
    let relation_kind = if version >= ARCHIVE_VERSION_1_16 {
        Some(read_archive_integer(reader, integer_size)?)
    } else {
        None
    };
    let owner = read_optional_string(reader, integer_size, limits, budget)?;
    let with_oids =
        read_required_string(reader, integer_size, limits, budget, "TOC with-OIDs flag")?;
    let dependencies = read_dependencies(reader, integer_size, limits, id, budget)?;
    let data_location = match read_archive_offset(reader, offset_size)? {
        ArchiveOffset::PositionNotSet => DataLocation::Unknown,
        ArchiveOffset::Position(offset) => DataLocation::Offset(offset),
        ArchiveOffset::NoData => DataLocation::NoData,
    };

    Ok(TocEntry::new(
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
        with_oids,
        dependencies,
        data_location,
    ))
}

fn read_dependencies<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    limits: Limits,
    entry_id: DumpId,
    budget: &mut MetadataBudget,
) -> Result<Vec<DumpId>, PgDumpError> {
    let mut dependencies = Vec::new();

    loop {
        let offset = reader.offset();
        let Some(bytes) = read_archive_string(reader, integer_size, limits.max_string_bytes())?
        else {
            return Ok(dependencies);
        };

        if dependencies.len() >= limits.max_dependencies_per_entry() {
            let count = dependencies
                .len()
                .checked_add(1)
                .ok_or(PgDumpError::ArithmeticOverflow { offset })?;
            return Err(PgDumpError::DependencyLimitExceeded {
                entry_id: entry_id.as_i32(),
                count: to_u64(count, offset)?,
                limit: to_u64(limits.max_dependencies_per_entry(), offset)?,
                offset,
            });
        }

        let dependency = parse_dependency(&bytes, entry_id, offset)?;
        budget.charge_dependency(entry_id, offset)?;
        let next_count = dependencies
            .len()
            .checked_add(1)
            .ok_or(PgDumpError::ArithmeticOverflow { offset })?;
        let next_count_u64 = to_u64(next_count, offset)?;
        dependencies
            .try_reserve(1)
            .map_err(|_| PgDumpError::DependencyAllocationFailed {
                entry_id: entry_id.as_i32(),
                count: next_count_u64,
                offset,
            })?;
        dependencies.push(dependency);
    }
}

fn parse_dependency(bytes: &[u8], entry_id: DumpId, offset: u64) -> Result<DumpId, PgDumpError> {
    if bytes.is_empty() {
        return Err(PgDumpError::InvalidDependencyEncoding {
            entry_id: entry_id.as_i32(),
            offset,
        });
    }

    let mut value = 0_u64;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(PgDumpError::InvalidDependencyEncoding {
                entry_id: entry_id.as_i32(),
                offset,
            });
        }
        let digit = *byte - b'0';
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u64::from(digit)))
            .ok_or(PgDumpError::InvalidDependencyEncoding {
                entry_id: entry_id.as_i32(),
                offset,
            })?;
    }

    let value = i32::try_from(value).map_err(|_| PgDumpError::InvalidDependencyEncoding {
        entry_id: entry_id.as_i32(),
        offset,
    })?;
    if value <= 0 {
        return Err(PgDumpError::InvalidDependencyEncoding {
            entry_id: entry_id.as_i32(),
            offset,
        });
    }

    Ok(DumpId::from_valid(value))
}

fn read_required_string<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    limits: Limits,
    budget: &mut MetadataBudget,
    field: &'static str,
) -> Result<ArchiveString, PgDumpError> {
    let offset = reader.offset();
    let bytes =
        read_retained_archive_string(reader, integer_size, limits.max_string_bytes(), budget)?
            .ok_or(PgDumpError::MissingRequiredArchiveString { field, offset })?;
    Ok(ArchiveString::from_bytes(bytes))
}

fn read_optional_string<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    limits: Limits,
    budget: &mut MetadataBudget,
) -> Result<Option<ArchiveString>, PgDumpError> {
    Ok(
        read_retained_archive_string(reader, integer_size, limits.max_string_bytes(), budget)?
            .map(ArchiveString::from_bytes),
    )
}

fn to_u64(value: usize, offset: u64) -> Result<u64, PgDumpError> {
    u64::try_from(value).map_err(|_| PgDumpError::ArithmeticOverflow { offset })
}
