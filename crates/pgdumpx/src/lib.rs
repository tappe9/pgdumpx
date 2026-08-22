//! A bounded, byte-oriented reader for PostgreSQL custom-format dumps.
//!
//! The current v0.1 implementation opens supported archive versions 1.14 through 1.16,
//! exposes version-aware TOC metadata, and streams validated entries compressed with
//! PostgreSQL's none, gzip, LZ4, and Zstandard modes.

#![forbid(unsafe_code)]

mod archive;
mod copy;
mod copy_metadata;
mod custom;
mod entry;
mod error;
mod error_taxonomy;
mod io;
mod limits;
mod model;
mod raw_entry;
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
mod raw_entry_tests;
#[cfg(test)]
mod scan_limits_tests;
#[cfg(test)]
mod table_rows_tests;

pub use archive::Archive;
pub use copy::{CopyRowReader, FieldRef, OwnedField, OwnedRow, Row};
pub use copy_metadata::{Column, TableDataRepresentation};
pub use entry::EntryDataReader;
pub use error::PgDumpError;
pub use error_taxonomy::{ErrorCategory, LimitContext, ResourceLimit};
pub use limits::{Limits, ScanLimits};
pub use model::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, DataLocation,
    DumpId, Section, TableRef, TocEntry,
};
pub use raw_entry::{BoundedEntryDataReader, EntryReadLimits};
pub use table_rows::TableRowReader;
