//! A bounded, byte-oriented metadata reader for PostgreSQL custom-format dumps.
//!
//! The current v0.1 implementation opens exact archive version 1.16 metadata.
//! Entry seeking, payload framing, decompression, and row parsing are added separately.

#![forbid(unsafe_code)]

mod archive;
mod custom;
mod error;
mod io;
mod limits;
mod model;

#[cfg(test)]
mod archive_primitives_tests;
#[cfg(test)]
mod metadata_open_tests;

pub use archive::Archive;
pub use error::PgDumpError;
pub use model::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, DataLocation,
    DumpId, Section, TableRef, TocEntry,
};
