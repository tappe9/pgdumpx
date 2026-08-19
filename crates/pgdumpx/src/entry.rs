use crate::{
    Compression, DumpId, PgDumpError, custom::data::CustomChunkReader, error::into_io_error,
};
use flate2::read::ZlibDecoder;
use std::{
    fmt,
    io::{self, Read},
};

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
    Gzip(ZlibDecoder<CustomChunkReader<'a, R>>),
}

impl<'a, R: Read> EntryDataReader<'a, R> {
    pub(crate) fn new(
        dump_id: DumpId,
        compression: Compression,
        chunks: CustomChunkReader<'a, R>,
    ) -> Result<Self, PgDumpError> {
        let backend = match compression {
            Compression::None => EntryBackend::None(chunks),
            Compression::Gzip => EntryBackend::Gzip(ZlibDecoder::new(chunks)),
            Compression::Lz4 => {
                return Err(PgDumpError::UnsupportedEntryCompression {
                    dump_id: dump_id.as_i32(),
                    algorithm: "lz4",
                });
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
        let dump_id = self.dump_id.as_i32();
        match &mut self.backend {
            EntryBackend::None(reader) => reader.read(output),
            EntryBackend::Gzip(reader) => reader.read(output).map_err(|source| {
                if source
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<PgDumpError>())
                    .is_some()
                {
                    source
                } else {
                    into_io_error(PgDumpError::DecompressionFailed {
                        dump_id,
                        algorithm: "gzip",
                        source,
                    })
                }
            }),
        }
    }
}
