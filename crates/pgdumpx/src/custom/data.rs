use crate::{
    DumpId, PgDumpError,
    custom::primitives::{ArchiveIntegerSize, read_archive_integer},
    error::into_io_error,
    io::archive_reader::ArchiveReader,
};
use std::io::{self, Read};

pub(crate) const BLK_DATA: u8 = 1;

pub(crate) struct CustomChunkReader<'a, R> {
    reader: ArchiveReader<&'a mut R>,
    integer_size: ArchiveIntegerSize,
    dump_id: DumpId,
    remaining: u64,
    done: bool,
}

impl<'a, R> CustomChunkReader<'a, R> {
    pub(crate) const fn new(
        reader: ArchiveReader<&'a mut R>,
        integer_size: ArchiveIntegerSize,
        dump_id: DumpId,
    ) -> Self {
        Self {
            reader,
            integer_size,
            dump_id,
            remaining: 0,
            done: false,
        }
    }

    fn begin_next_chunk(&mut self) -> io::Result<()>
    where
        R: Read,
    {
        let length_offset = self.reader.offset();
        let length = read_archive_integer(&mut self.reader, self.integer_size).map_err(
            |error| match error {
                PgDumpError::UnexpectedEof { .. } => {
                    into_io_error(PgDumpError::TruncatedDataChunkLength {
                        dump_id: self.dump_id.as_i32(),
                        offset: length_offset,
                    })
                }
                other => into_io_error(other),
            },
        )?;
        if length < 0 {
            return Err(into_io_error(PgDumpError::InvalidDataChunkLength {
                dump_id: self.dump_id.as_i32(),
                length,
                offset: length_offset,
            }));
        }
        if length == 0 {
            self.done = true;
            return Ok(());
        }

        self.remaining = u64::try_from(length).map_err(|_| {
            into_io_error(PgDumpError::InvalidDataChunkLength {
                dump_id: self.dump_id.as_i32(),
                length,
                offset: length_offset,
            })
        })?;
        Ok(())
    }
}

impl<R: Read> Read for CustomChunkReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.done {
            return Ok(0);
        }

        while self.remaining == 0 && !self.done {
            self.begin_next_chunk()?;
        }
        if self.done {
            return Ok(0);
        }

        let available = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        let requested = output.len().min(available);
        let read = self
            .reader
            .read_some(&mut output[..requested])
            .map_err(into_io_error)?;
        if read == 0 {
            return Err(into_io_error(PgDumpError::TruncatedDataChunk {
                dump_id: self.dump_id.as_i32(),
                remaining: self.remaining,
                offset: self.reader.offset(),
            }));
        }

        let read_u64 = u64::try_from(read).map_err(|_| {
            into_io_error(PgDumpError::ArithmeticOverflow {
                offset: self.reader.offset(),
            })
        })?;
        self.remaining = self.remaining.checked_sub(read_u64).ok_or_else(|| {
            into_io_error(PgDumpError::ArithmeticOverflow {
                offset: self.reader.offset(),
            })
        })?;
        Ok(read)
    }
}
