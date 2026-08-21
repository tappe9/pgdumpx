use crate::{PgDumpError, io::archive_reader::ArchiveReader};
use std::io::Read;

const POSITION_NOT_SET: u8 = 1;
const POSITION_SET: u8 = 2;
const NO_DATA: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchiveIntegerSize(u8);

impl ArchiveIntegerSize {
    pub(crate) fn new(size: u8, offset: u64) -> Result<Self, PgDumpError> {
        if (1..=4).contains(&size) {
            Ok(Self(size))
        } else {
            Err(PgDumpError::UnsupportedArchiveIntegerSize { size, offset })
        }
    }

    pub(crate) const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchiveOffsetSize(u8);

impl ArchiveOffsetSize {
    pub(crate) fn new(size: u8, offset: u64) -> Result<Self, PgDumpError> {
        if size == 0 {
            Err(PgDumpError::InvalidArchiveOffsetSize { size, offset })
        } else {
            Ok(Self(size))
        }
    }

    pub(crate) const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveOffset {
    PositionNotSet,
    Position(u64),
    NoData,
}

pub(crate) fn read_archive_integer<R: Read>(
    reader: &mut ArchiveReader<R>,
    size: ArchiveIntegerSize,
) -> Result<i32, PgDumpError> {
    let start_offset = reader.offset();
    let sign = reader.read_byte()?;
    let mut magnitude = 0_u64;

    for byte_index in 0..size.get() {
        let byte = reader.read_byte()?;
        let shift =
            u32::from(byte_index)
                .checked_mul(8)
                .ok_or(PgDumpError::ArithmeticOverflow {
                    offset: start_offset,
                })?;
        let component =
            u64::from(byte)
                .checked_shl(shift)
                .ok_or(PgDumpError::ArithmeticOverflow {
                    offset: start_offset,
                })?;
        magnitude = magnitude
            .checked_add(component)
            .ok_or(PgDumpError::ArithmeticOverflow {
                offset: start_offset,
            })?;
    }

    let magnitude =
        i64::try_from(magnitude).map_err(|_| PgDumpError::ArchiveIntegerOutOfRange {
            offset: start_offset,
        })?;
    let signed = if sign == 0 {
        magnitude
    } else {
        magnitude
            .checked_neg()
            .ok_or(PgDumpError::ArchiveIntegerOutOfRange {
                offset: start_offset,
            })?
    };

    i32::try_from(signed).map_err(|_| PgDumpError::ArchiveIntegerOutOfRange {
        offset: start_offset,
    })
}

pub(crate) fn read_archive_offset<R: Read>(
    reader: &mut ArchiveReader<R>,
    size: ArchiveOffsetSize,
) -> Result<ArchiveOffset, PgDumpError> {
    let state_offset = reader.offset();
    let state = reader.read_byte()?;
    if !matches!(state, POSITION_NOT_SET | POSITION_SET | NO_DATA) {
        return Err(PgDumpError::InvalidArchiveOffsetState {
            state,
            offset: state_offset,
        });
    }

    let mut value = 0_u64;
    for byte_index in 0..size.get() {
        let byte_offset = reader.offset();
        let byte = reader.read_byte()?;
        if byte_index < 8 {
            let shift =
                u32::from(byte_index)
                    .checked_mul(8)
                    .ok_or(PgDumpError::ArithmeticOverflow {
                        offset: byte_offset,
                    })?;
            let component =
                u64::from(byte)
                    .checked_shl(shift)
                    .ok_or(PgDumpError::ArithmeticOverflow {
                        offset: byte_offset,
                    })?;
            value = value
                .checked_add(component)
                .ok_or(PgDumpError::ArithmeticOverflow {
                    offset: byte_offset,
                })?;
        } else if byte != 0 {
            return Err(PgDumpError::ArchiveOffsetOutOfRange {
                offset: byte_offset,
            });
        }
    }

    match state {
        POSITION_NOT_SET => Ok(ArchiveOffset::PositionNotSet),
        POSITION_SET => Ok(ArchiveOffset::Position(value)),
        NO_DATA => Ok(ArchiveOffset::NoData),
        _ => Err(PgDumpError::InvalidArchiveOffsetState {
            state,
            offset: state_offset,
        }),
    }
}

pub(crate) fn read_archive_string<R: Read>(
    reader: &mut ArchiveReader<R>,
    integer_size: ArchiveIntegerSize,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, PgDumpError> {
    let length_offset = reader.offset();
    let encoded_length = read_archive_integer(reader, integer_size)?;
    if encoded_length < 0 {
        return Ok(None);
    }

    let length = usize::try_from(encoded_length).map_err(|_| PgDumpError::ArithmeticOverflow {
        offset: length_offset,
    })?;
    let length_u64 = u64::try_from(length).map_err(|_| PgDumpError::ArithmeticOverflow {
        offset: length_offset,
    })?;
    let limit_u64 =
        u64::try_from(max_bytes).map_err(|_| PgDumpError::ArithmeticOverflow {
            offset: length_offset,
        })?;

    if length > max_bytes {
        return Err(PgDumpError::ArchiveStringLimitExceeded {
            length: length_u64,
            limit: limit_u64,
            offset: length_offset,
        });
    }

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| PgDumpError::ArchiveStringAllocationFailed {
            length: length_u64,
            offset: length_offset,
        })?;
    bytes.resize(length, 0);
    reader.read_exact(&mut bytes)?;

    Ok(Some(bytes))
}
