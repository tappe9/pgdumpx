use crate::PgDumpError;
use std::io::{ErrorKind, Read};

pub(crate) struct ArchiveReader<R> {
    inner: R,
    offset: u64,
}

impl<R> ArchiveReader<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self::new_at(inner, 0)
    }

    pub(crate) fn new_at(inner: R, offset: u64) -> Self {
        Self { inner, offset }
    }

    pub(crate) const fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> ArchiveReader<R> {
    pub(crate) fn read_byte(&mut self) -> Result<u8, PgDumpError> {
        let mut byte = [0_u8; 1];
        self.read_exact(&mut byte)?;
        Ok(byte[0])
    }

    pub(crate) fn read_exact(&mut self, mut output: &mut [u8]) -> Result<(), PgDumpError> {
        let requested =
            u64::try_from(output.len()).map_err(|_| PgDumpError::ArithmeticOverflow {
                offset: self.offset,
            })?;
        self.offset
            .checked_add(requested)
            .ok_or(PgDumpError::ArithmeticOverflow {
                offset: self.offset,
            })?;

        while !output.is_empty() {
            match self.inner.read(output) {
                Ok(0) => {
                    return Err(PgDumpError::UnexpectedEof {
                        offset: self.offset,
                    });
                }
                Ok(read) => {
                    let read_u64 =
                        u64::try_from(read).map_err(|_| PgDumpError::ArithmeticOverflow {
                            offset: self.offset,
                        })?;
                    self.offset = self.offset.checked_add(read_u64).ok_or(
                        PgDumpError::ArithmeticOverflow {
                            offset: self.offset,
                        },
                    )?;
                    let (_, remaining) = output.split_at_mut(read);
                    output = remaining;
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
                    return Err(PgDumpError::UnexpectedEof {
                        offset: self.offset,
                    });
                }
                Err(source) => {
                    return Err(PgDumpError::Io {
                        offset: self.offset,
                        source,
                    });
                }
            }
        }

        Ok(())
    }
}
