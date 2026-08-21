//! A bounded, byte-oriented reader for PostgreSQL custom-format dumps.
//!
//! The current v0.1 implementation opens exact archive version 1.16 metadata
//! and streams validated entries compressed with PostgreSQL's none/gzip modes.

#![forbid(unsafe_code)]

mod archive;
mod copy;
mod copy_metadata;
mod custom;
mod entry;
mod error;
mod io;
mod limits;
mod model;
mod table_rows;

#[cfg(test)]
mod archive_primitives_tests;
#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod metadata_open_tests;
#[cfg(test)]
mod table_rows_tests;

pub use archive::Archive;
pub use copy::{CopyRowReader, FieldRef, OwnedField, OwnedRow, Row};
pub use copy_metadata::{Column, TableDataRepresentation};
pub use entry::EntryDataReader;
pub use error::{ErrorCategory, LimitContext, PgDumpError, ResourceLimit};
pub use limits::Limits;
pub use model::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, DataLocation,
    DumpId, Section, TableRef, TocEntry,
};
pub use table_rows::TableRowReader;
