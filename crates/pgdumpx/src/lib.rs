#![forbid(unsafe_code)]

mod archive;
mod custom;
mod error;
mod io;
mod limits;
mod model;

#[cfg(test)]
mod archive_primitives_tests;

pub use archive::Archive;
pub use error::PgDumpError;
pub use model::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, DataLocation,
    DumpId, Section, TableRef, TocEntry,
};
