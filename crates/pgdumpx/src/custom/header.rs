use crate::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, Limits,
    PgDumpError,
    custom::primitives::{
        ArchiveIntegerSize, ArchiveOffsetSize, read_archive_integer, read_archive_string,
    },
    io::archive_reader::ArchiveReader,
};
use std::io::Read;

const ARCHIVE_MAGIC: &[u8; 5] = b"PGDMP";
const CUSTOM_ARCHIVE_FORMAT: u8 = 1;
const SUPPORTED_VERSION: ArchiveVersion = ArchiveVersion::new(1, 16, 0);

#[derive(Debug)]
pub(crate) struct ParsedHeader {
    pub(crate) header: ArchiveHeader,
    pub(crate) integer_size: ArchiveIntegerSize,
    pub(crate) offset_size: ArchiveOffsetSize,
}

pub(crate) fn read_header<R: Read>(
    reader: &mut ArchiveReader<R>,
    limits: Limits,
) -> Result<ParsedHeader, PgDumpError> {
    let magic_offset = reader.offset();
    let mut magic = [0_u8; ARCHIVE_MAGIC.len()];
    reader.read_exact(&mut magic)?;
    if &magic != ARCHIVE_MAGIC {
        return Err(PgDumpError::InvalidArchiveMagic {
            offset: magic_offset,
        });
    }

    let version_offset = reader.offset();
    let version = ArchiveVersion::new(
        reader.read_byte()?,
        reader.read_byte()?,
        reader.read_byte()?,
    );
    if version != SUPPORTED_VERSION {
        return Err(PgDumpError::UnsupportedArchiveVersion {
            major: version.major(),
            minor: version.minor(),
            revision: version.revision(),
            offset: version_offset,
        });
    }

    let integer_size_offset = reader.offset();
    let integer_size_byte = reader.read_byte()?;
    let integer_size = ArchiveIntegerSize::new(integer_size_byte, integer_size_offset)?;

    let offset_size_offset = reader.offset();
    let offset_size_byte = reader.read_byte()?;
    let offset_size = ArchiveOffsetSize::new(offset_size_byte, offset_size_offset)?;

    let format_offset = reader.offset();
    let format = reader.read_byte()?;
    if format != CUSTOM_ARCHIVE_FORMAT {
        return Err(PgDumpError::UnexpectedArchiveFormat {
            format,
            offset: format_offset,
        });
    }

    let compression_offset = reader.offset();
    let compression = match reader.read_byte()? {
        0 => Compression::None,
        1 => Compression::Gzip,
        2 => Compression::Lz4,
        3 => Compression::Zstd,
        algorithm => {
            return Err(PgDumpError::UnsupportedCompressionAlgorithm {
                algorithm,
                offset: compression_offset,
            });
        }
    };

    let created_at = ArchiveTimestamp::new(
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
        read_archive_integer(reader, integer_size)?,
    );
    let database_name =
        read_required_string(reader, integer_size, limits, "archive database name")?;
    let server_version =
        read_required_string(reader, integer_size, limits, "archive server version")?;
    let dump_version = read_required_string(reader, integer_size, limits, "archive dump version")?;

    Ok(ParsedHeader {
        header: ArchiveHeader::new(
            version,
            integer_size_byte,
            offset_size_byte,
            compression,
            created_at,
            database_name,
            server_version,
            dump_version,
        ),
        integer_size,
        offset_size,
    })
}

fn read_required_string<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    limits: Limits,
    field: &'static str,
) -> Result<ArchiveString, PgDumpError> {
    let offset = reader.offset();
    let bytes = read_archive_string(reader, integer_size, limits.max_string_bytes())?
        .ok_or(PgDumpError::MissingRequiredArchiveString { field, offset })?;
    Ok(ArchiveString::from_bytes(bytes))
}
