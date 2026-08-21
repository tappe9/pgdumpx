use crate::{
    Compression, DumpId, PgDumpError, custom::data::CustomChunkReader, error::into_io_error,
};
use flate2::{Decompress, FlushDecompress, Status};
#[cfg(feature = "lz4")]
use lz4_flex::frame::FrameDecoder as Lz4FrameDecoder;
#[cfg(feature = "zstd")]
use ruzstd::decoding::{FrameDecoder as ZstdFrameDecoder, StreamingDecoder};
#[cfg(any(feature = "lz4", feature = "zstd"))]
use std::{cell::RefCell, rc::Rc};
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
    Lz4(Lz4EntryDecoder<'a>),
    #[cfg(feature = "zstd")]
    Zstd(ZstdEntryDecoder<'a>),
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
                #[cfg(feature = "zstd")]
                {
                    EntryBackend::Zstd(ZstdEntryDecoder::new(dump_id, chunks))
                }
                #[cfg(not(feature = "zstd"))]
                {
                    return Err(PgDumpError::UnsupportedEntryCompression {
                        dump_id: dump_id.as_i32(),
                        algorithm: "zstandard",
                    });
                }
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
            #[cfg(feature = "zstd")]
            EntryBackend::Zstd(_) => "zstandard",
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
            #[cfg(feature = "zstd")]
            EntryBackend::Zstd(reader) => reader.read(output),
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

#[cfg(feature = "zstd")]
struct ZstdEntryDecoder<'a> {
    dump_id: DumpId,
    state: ZstdDecoderState<'a>,
    source_error: Rc<RefCell<Option<io::Error>>>,
}

#[cfg(feature = "zstd")]
enum ZstdDecoderState<'a> {
    Uninitialized(Option<Box<dyn Read + 'a>>),
    Decoding(StreamingDecoder<Box<dyn Read + 'a>, ZstdFrameDecoder>),
    Failed,
}

#[cfg(feature = "zstd")]
impl<'a> ZstdEntryDecoder<'a> {
    fn new<R: Read + 'a>(dump_id: DumpId, source: CustomChunkReader<'a, R>) -> Self {
        let source_error = Rc::new(RefCell::new(None));
        let tracked = TrackedCompressedSource {
            inner: source,
            error: Rc::clone(&source_error),
        };
        Self {
            dump_id,
            state: ZstdDecoderState::Uninitialized(Some(Box::new(tracked))),
            source_error,
        }
    }

    fn decompression_error(&self, source: io::Error) -> io::Error {
        into_io_error(PgDumpError::DecompressionFailed {
            dump_id: self.dump_id.as_i32(),
            algorithm: "zstandard",
            source,
        })
    }

    fn take_source_error(&self) -> Option<io::Error> {
        self.source_error.borrow_mut().take()
    }

    fn initialize(&mut self) -> io::Result<()> {
        let source = match &mut self.state {
            ZstdDecoderState::Uninitialized(source) => source
                .take()
                .expect("uninitialized Zstandard decoder must own its source"),
            ZstdDecoderState::Decoding(_) => return Ok(()),
            ZstdDecoderState::Failed => {
                return Err(self.decompression_error(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Zstandard decoder is unavailable after an earlier failure",
                )));
            }
        };

        match StreamingDecoder::new(source) {
            Ok(decoder) => {
                self.state = ZstdDecoderState::Decoding(decoder);
                Ok(())
            }
            Err(source) => {
                self.state = ZstdDecoderState::Failed;
                if let Some(source) = self.take_source_error() {
                    Err(source)
                } else {
                    Err(self.decompression_error(io::Error::new(
                        io::ErrorKind::InvalidData,
                        source.to_string(),
                    )))
                }
            }
        }
    }
}

#[cfg(feature = "zstd")]
impl Read for ZstdEntryDecoder<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.initialize()?;

        let result = match &mut self.state {
            ZstdDecoderState::Decoding(decoder) => decoder.read(output),
            ZstdDecoderState::Uninitialized(_) => unreachable!("decoder was initialized above"),
            ZstdDecoderState::Failed => unreachable!("failed initialization returned above"),
        };
        match result {
            Ok(read) => Ok(read),
            Err(source) => {
                if let Some(source) = self.take_source_error() {
                    Err(source)
                } else {
                    Err(self.decompression_error(source))
                }
            }
        }
    }
}

#[cfg(feature = "zstd")]
struct TrackedCompressedSource<R> {
    inner: R,
    error: Rc<RefCell<Option<io::Error>>>,
}

#[cfg(feature = "zstd")]
impl<R: Read> Read for TrackedCompressedSource<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(output) {
            Ok(read) => Ok(read),
            Err(source) => {
                let kind = source.kind();
                *self.error.borrow_mut() = Some(source);
                Err(io::Error::new(kind, "compressed entry source read failed"))
            }
        }
    }
}

#[cfg(feature = "lz4")]
struct Lz4EntryDecoder<'a> {
    dump_id: DumpId,
    decoder: Lz4FrameDecoder<Box<dyn Read + 'a>>,
    source_state: Rc<RefCell<Lz4SourceState>>,
    stream_end: bool,
}

#[cfg(feature = "lz4")]
impl<'a> Lz4EntryDecoder<'a> {
    fn new<R: Read + 'a>(dump_id: DumpId, source: CustomChunkReader<'a, R>) -> Self {
        let source_state = Rc::new(RefCell::new(Lz4SourceState::new()));
        let tracked = TrackedLz4Source {
            inner: source,
            state: Rc::clone(&source_state),
        };
        let reader: Box<dyn Read + 'a> = Box::new(tracked);
        Self {
            dump_id,
            decoder: Lz4FrameDecoder::new(reader),
            source_state,
            stream_end: false,
        }
    }

    fn decompression_error(&self, source: io::Error) -> io::Error {
        into_io_error(PgDumpError::DecompressionFailed {
            dump_id: self.dump_id.as_i32(),
            algorithm: "lz4",
            source,
        })
    }

    fn take_source_error(&self) -> Option<io::Error> {
        self.source_state.borrow_mut().source_error.take()
    }

    fn finish_stream(&mut self) -> io::Result<usize> {
        if !self.source_state.borrow().frame.is_finished() {
            return Err(self.decompression_error(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "LZ4 frame ended before its end mark",
            )));
        }

        let mut trailing = [0_u8; 1];
        match self.decoder.get_mut().read(&mut trailing) {
            Ok(0) => {
                self.stream_end = true;
                Ok(0)
            }
            Ok(_) => Err(self.decompression_error(io::Error::new(
                io::ErrorKind::InvalidData,
                "trailing compressed bytes after LZ4 frame",
            ))),
            Err(source) => {
                if let Some(source) = self.take_source_error() {
                    Err(source)
                } else {
                    Err(self.decompression_error(source))
                }
            }
        }
    }
}

#[cfg(feature = "lz4")]
impl Read for Lz4EntryDecoder<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.stream_end {
            return Ok(0);
        }

        match self.decoder.read(output) {
            Ok(0) => self.finish_stream(),
            Ok(read) => Ok(read),
            Err(source) => {
                if let Some(source) = self.take_source_error() {
                    Err(source)
                } else {
                    Err(self.decompression_error(source))
                }
            }
        }
    }
}

#[cfg(feature = "lz4")]
struct Lz4SourceState {
    source_error: Option<io::Error>,
    frame: Lz4FrameTracker,
}

#[cfg(feature = "lz4")]
impl Lz4SourceState {
    fn new() -> Self {
        Self {
            source_error: None,
            frame: Lz4FrameTracker::new(),
        }
    }
}

#[cfg(feature = "lz4")]
struct TrackedLz4Source<R> {
    inner: R,
    state: Rc<RefCell<Lz4SourceState>>,
}

#[cfg(feature = "lz4")]
impl<R: Read> Read for TrackedLz4Source<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(output) {
            Ok(read) => {
                self.state.borrow_mut().frame.observe(&output[..read]);
                Ok(read)
            }
            Err(source) => {
                let kind = source.kind();
                self.state.borrow_mut().source_error = Some(source);
                Err(io::Error::new(kind, "compressed entry source read failed"))
            }
        }
    }
}

#[cfg(feature = "lz4")]
struct Lz4FrameTracker {
    phase: Lz4FramePhase,
    block_checksum: bool,
    content_checksum: bool,
}

#[cfg(feature = "lz4")]
impl Lz4FrameTracker {
    fn new() -> Self {
        Self {
            phase: Lz4FramePhase::Header {
                bytes: [0; 19],
                filled: 0,
                required: 6,
            },
            block_checksum: false,
            content_checksum: false,
        }
    }

    fn is_finished(&self) -> bool {
        matches!(self.phase, Lz4FramePhase::Finished)
    }

    fn observe(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            match &mut self.phase {
                Lz4FramePhase::Header {
                    bytes,
                    filled,
                    required,
                } => {
                    let take = (*required - *filled).min(input.len());
                    bytes[*filled..*filled + take].copy_from_slice(&input[..take]);
                    *filled += take;
                    input = &input[take..];

                    if *filled == 6 && *required == 6 {
                        let flg = bytes[4];
                        self.block_checksum = flg & 0x10 != 0;
                        self.content_checksum = flg & 0x04 != 0;
                        *required =
                            7 + usize::from(flg & 0x08 != 0) * 8 + usize::from(flg & 0x01 != 0) * 4;
                    }
                    if *filled == *required {
                        self.phase = Lz4FramePhase::BlockHeader {
                            bytes: [0; 4],
                            filled: 0,
                        };
                    }
                }
                Lz4FramePhase::BlockHeader { bytes, filled } => {
                    let take = (4 - *filled).min(input.len());
                    bytes[*filled..*filled + take].copy_from_slice(&input[..take]);
                    *filled += take;
                    input = &input[take..];
                    if *filled == 4 {
                        let block = u32::from_le_bytes(*bytes);
                        if block == 0 {
                            self.phase = if self.content_checksum {
                                Lz4FramePhase::ContentChecksum { remaining: 4 }
                            } else {
                                Lz4FramePhase::Finished
                            };
                        } else {
                            self.phase = Lz4FramePhase::BlockPayload {
                                remaining: (block & 0x7fff_ffff) as usize,
                            };
                        }
                    }
                }
                Lz4FramePhase::BlockPayload { remaining } => {
                    let take = (*remaining).min(input.len());
                    *remaining -= take;
                    input = &input[take..];
                    if *remaining == 0 {
                        self.phase = if self.block_checksum {
                            Lz4FramePhase::BlockChecksum { remaining: 4 }
                        } else {
                            Lz4FramePhase::BlockHeader {
                                bytes: [0; 4],
                                filled: 0,
                            }
                        };
                    }
                }
                Lz4FramePhase::BlockChecksum { remaining } => {
                    let take = (*remaining).min(input.len());
                    *remaining -= take;
                    input = &input[take..];
                    if *remaining == 0 {
                        self.phase = Lz4FramePhase::BlockHeader {
                            bytes: [0; 4],
                            filled: 0,
                        };
                    }
                }
                Lz4FramePhase::ContentChecksum { remaining } => {
                    let take = (*remaining).min(input.len());
                    *remaining -= take;
                    input = &input[take..];
                    if *remaining == 0 {
                        self.phase = Lz4FramePhase::Finished;
                    }
                }
                Lz4FramePhase::Finished => return,
            }
        }
    }
}

#[cfg(feature = "lz4")]
enum Lz4FramePhase {
    Header {
        bytes: [u8; 19],
        filled: usize,
        required: usize,
    },
    BlockHeader {
        bytes: [u8; 4],
        filled: usize,
    },
    BlockPayload {
        remaining: usize,
    },
    BlockChecksum {
        remaining: usize,
    },
    ContentChecksum {
        remaining: usize,
    },
    Finished,
}
