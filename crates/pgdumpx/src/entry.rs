use crate::{
    Compression, DumpId, PgDumpError, custom::data::CustomChunkReader, error::into_io_error,
};
use flate2::{Decompress, FlushDecompress, Status};
#[cfg(feature = "lz4")]
use lz4_flex::frame::FrameDecoder;
use std::{
    fmt,
    io::{self, Read},
};

const COMPRESSED_INPUT_BUFFER_BYTES: usize = 8 * 1024;

/// A streaming, decompressed view of one validated custom-archive entry.
///
/// The concrete decompression backend is intentionally private. Reading from
/// this value never requires buffering the complete archive entry.
pub struct EntryDataReader<'a, R> {
    dump_id: DumpId,
    backend: EntryBackend<'a, R>,
}

enum EntryBackend<'a, R> {
    None(CustomChunkReader<'a, R>),
    Gzip(ZlibEntryDecoder<'a, R>),
    #[cfg(feature = "lz4")]
    Lz4(Lz4EntryDecoder<'a, R>),
}

impl<'a, R: Read> EntryDataReader<'a, R> {
    pub(crate) fn new(
        dump_id: DumpId,
        compression: Compression,
        chunks: CustomChunkReader<'a, R>,
    ) -> Result<Self, PgDumpError> {
        let backend = match compression {
            Compression::None => EntryBackend::None(chunks),
            Compression::Gzip => EntryBackend::Gzip(ZlibEntryDecoder::new(dump_id, chunks)?),
            Compression::Lz4 => {
                #[cfg(feature = "lz4")]
                {
                    EntryBackend::Lz4(Lz4EntryDecoder::new(dump_id, chunks))
                }
                #[cfg(not(feature = "lz4"))]
                {
                    return Err(PgDumpError::UnsupportedEntryCompression {
                        dump_id: dump_id.as_i32(),
                        algorithm: "lz4",
                    });
                }
            }
            Compression::Zstd => {
                return Err(PgDumpError::UnsupportedEntryCompression {
                    dump_id: dump_id.as_i32(),
                    algorithm: "zstandard",
                });
            }
        };
        Ok(Self { dump_id, backend })
    }

    fn algorithm(&self) -> &'static str {
        match &self.backend {
            EntryBackend::None(_) => "none",
            EntryBackend::Gzip(_) => "gzip",
            #[cfg(feature = "lz4")]
            EntryBackend::Lz4(_) => "lz4",
        }
    }
}

impl<R: Read> fmt::Debug for EntryDataReader<'_, R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryDataReader")
            .field("dump_id", &self.dump_id)
            .field("algorithm", &self.algorithm())
            .finish_non_exhaustive()
    }
}

impl<R: Read> Read for EntryDataReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match &mut self.backend {
            EntryBackend::None(reader) => reader.read(output),
            EntryBackend::Gzip(reader) => reader.read(output),
            #[cfg(feature = "lz4")]
            EntryBackend::Lz4(reader) => reader.read(output),
        }
    }
}

struct ZlibEntryDecoder<'a, R> {
    dump_id: DumpId,
    source: CustomChunkReader<'a, R>,
    decoder: Decompress,
    input: Vec<u8>,
    input_start: usize,
    input_end: usize,
    source_eof: bool,
    stream_end: bool,
    pending_error: Option<io::Error>,
}

impl<'a, R> ZlibEntryDecoder<'a, R> {
    fn new(dump_id: DumpId, source: CustomChunkReader<'a, R>) -> Result<Self, PgDumpError> {
        let mut input = Vec::new();
        input
            .try_reserve_exact(COMPRESSED_INPUT_BUFFER_BYTES)
            .map_err(|_| PgDumpError::EntryBufferAllocationFailed {
                dump_id: dump_id.as_i32(),
                requested: COMPRESSED_INPUT_BUFFER_BYTES as u64,
            })?;
        input.resize(COMPRESSED_INPUT_BUFFER_BYTES, 0);

        Ok(Self {
            dump_id,
            source,
            decoder: Decompress::new(true),
            input,
            input_start: 0,
            input_end: 0,
            source_eof: false,
            stream_end: false,
            pending_error: None,
        })
    }

    fn decompression_error(&self, source: io::Error) -> io::Error {
        into_io_error(PgDumpError::DecompressionFailed {
            dump_id: self.dump_id.as_i32(),
            algorithm: "gzip",
            source,
        })
    }

    fn fail_or_defer(&mut self, error: io::Error, written: usize) -> io::Result<usize> {
        if written == 0 {
            Err(error)
        } else {
            self.pending_error = Some(error);
            Ok(written)
        }
    }
}

impl<R: Read> Read for ZlibEntryDecoder<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.stream_end {
            return Ok(0);
        }
        if let Some(error) = self.pending_error.take() {
            return Err(error);
        }

        let mut written = 0;
        loop {
            if self.input_start == self.input_end && !self.source_eof {
                let read = self.source.read(&mut self.input)?;
                self.input_start = 0;
                self.input_end = read;
                self.source_eof = read == 0;
            }

            let before_in = self.decoder.total_in();
            let before_out = self.decoder.total_out();
            let status = match self.decoder.decompress(
                &self.input[self.input_start..self.input_end],
                &mut output[written..],
                FlushDecompress::None,
            ) {
                Ok(status) => status,
                Err(source) => {
                    let error = self.decompression_error(io::Error::from(source));
                    return self.fail_or_defer(error, written);
                }
            };

            let consumed = self
                .decoder
                .total_in()
                .checked_sub(before_in)
                .ok_or_else(|| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;
            let produced = self
                .decoder
                .total_out()
                .checked_sub(before_out)
                .ok_or_else(|| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;
            let consumed = usize::try_from(consumed)
                .map_err(|_| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;
            let produced = usize::try_from(produced)
                .map_err(|_| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;
            self.input_start = self
                .input_start
                .checked_add(consumed)
                .ok_or_else(|| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;
            written = written
                .checked_add(produced)
                .ok_or_else(|| into_io_error(PgDumpError::ArithmeticOverflow { offset: 0 }))?;

            if status == Status::StreamEnd {
                self.stream_end = true;
                return Ok(written);
            }
            if written == output.len() {
                return Ok(written);
            }
            if consumed == 0 && produced == 0 {
                if self.input_start == self.input_end && self.source_eof {
                    let error = self.decompression_error(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "zlib stream ended before its checksum trailer",
                    ));
                    return self.fail_or_defer(error, written);
                }
                if self.input_start != self.input_end {
                    let error = self.decompression_error(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "zlib decoder made no progress with input remaining",
                    ));
                    return self.fail_or_defer(error, written);
                }
            }
        }
    }
}

#[cfg(feature = "lz4")]
struct Lz4EntryDecoder<'a, R> {
    dump_id: DumpId,
    decoder: FrameDecoder<TrackedCompressedSource<'a, R>>,
}

#[cfg(feature = "lz4")]
impl<'a, R> Lz4EntryDecoder<'a, R> {
    fn new(dump_id: DumpId, source: CustomChunkReader<'a, R>) -> Self {
        Self {
            dump_id,
            decoder: FrameDecoder::new(TrackedCompressedSource::new(source)),
        }
    }

    fn decompression_error(&self, source: io::Error) -> io::Error {
        into_io_error(PgDumpError::DecompressionFailed {
            dump_id: self.dump_id.as_i32(),
            algorithm: "lz4",
            source,
        })
    }
}

#[cfg(feature = "lz4")]
impl<R: Read> Read for Lz4EntryDecoder<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match self.decoder.read(output) {
            Ok(read) => Ok(read),
            Err(source) => {
                if let Some(source) = self.decoder.get_mut().take_error() {
                    Err(source)
                } else {
                    Err(self.decompression_error(source))
                }
            }
        }
    }
}

#[cfg(feature = "lz4")]
struct TrackedCompressedSource<'a, R> {
    inner: CustomChunkReader<'a, R>,
    error: Option<io::Error>,
}

#[cfg(feature = "lz4")]
impl<'a, R> TrackedCompressedSource<'a, R> {
    fn new(inner: CustomChunkReader<'a, R>) -> Self {
        Self { inner, error: None }
    }

    fn take_error(&mut self) -> Option<io::Error> {
        self.error.take()
    }
}

#[cfg(feature = "lz4")]
impl<R: Read> Read for TrackedCompressedSource<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(output) {
            Ok(read) => Ok(read),
            Err(source) => {
                let kind = source.kind();
                self.error = Some(source);
                Err(io::Error::new(
                    kind,
                    "compressed entry source read failed",
                ))
            }
        }
    }
}
