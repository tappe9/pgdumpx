//! Bounded, byte-oriented access to PostgreSQL custom-format (`pg_dump -Fc`) archives.
//!
//! `pgdumpx` opens supported archive versions 1.14 through 1.16, parses the header and
//! table of contents eagerly, and keeps entry payload access lazy. Selected entries are
//! validated before they are exposed and are decompressed as streams through
//! [`EntryDataReader`]. The default feature set supports PostgreSQL's none, gzip, LZ4,
//! and Zstandard archive compression modes.
//!
//! # Byte-oriented contract
//!
//! Archive metadata and COPY fields are bytes first. [`ArchiveString::as_bytes`], table
//! lookup, column lookup, [`Row`], and [`FieldRef`] do not require UTF-8. Callers opt into
//! text conversion with fallible helpers such as [`ArchiveString::to_str`] and
//! [`Column::name_str`]. [`FieldRef::Bytes`] and [`OwnedField::Bytes`] contain logical
//! field bytes *after* PostgreSQL COPY-text backslash decoding; they are not the escaped
//! on-wire spelling. `\N` is represented as [`FieldRef::Null`] rather than as bytes.
//!
//! # Lending rows
//!
//! [`CopyRowReader`] and [`TableRowReader`] reuse their current-row storage. A [`Row`]
//! therefore borrows from the reader and remains valid only until that reader is mutably
//! borrowed again. The row readers intentionally expose `next_row(&mut self)` instead of
//! implementing `Iterator`. [`OwnedRow`] is used when a matched row must outlive reader
//! advancement; normal iteration stays borrowed.
//!
//! # Three independent limit classes
//!
//! Resource controls are intentionally separated because they protect different work:
//!
//! - [`Limits`] contains finite structural bounds used while opening archives and parsing
//!   individual COPY rows/column layouts.
//! - [`ScanLimits`] bounds total rows and parser-consumed decompressed COPY bytes for a row
//!   scan. These limits do not measure decoder read-ahead that the COPY parser has not
//!   consumed.
//! - [`EntryReadLimits`] bounds decompressed bytes returned by raw selected-entry access.
//!   Crossing a raw-output limit is an error, never successful truncation.
//!
//! [`Limits::default_compatible`] supplies the finite `Limits` default. In contrast,
//! [`ScanLimits::unlimited`] and [`EntryReadLimits::unlimited`] are also their respective
//! library defaults so trusted callers can choose operation policy explicitly. The
//! `pgdumpx extract` CLI applies its own finite 1 GiB default when its raw-output option
//! is omitted.
//!
//! # Sequential row search
//!
//! [`CopyRowReader::find_first`] and [`TableRowReader::find_first`] are sequential scans
//! from the reader's current position. A freshly created [`TableRowReader`] starts at the
//! beginning of the selected `TABLE DATA` entry, but there is no row-level value index in
//! the archive. An early match terminates immediately; a late or absent match can process
//! the rest of the selected entry unless [`ScanLimits`] stops it. Worst-case unrestricted
//! work is proportional to the remaining selected table-data size.
//!
//! # Raw extraction and partial output
//!
//! [`Archive::copy_entry_to`] streams decompressed bytes incrementally. If a later input,
//! decompression, limit, or destination error occurs, bytes already accepted by the
//! destination cannot be rolled back. The operation still returns an error and never
//! reports a partial stream as successful extraction.
//!
//! [`ExtractionPlan::execute`] extends that same bounded raw path to multiple tables while
//! preserving the archive's single mutable seek invariant. It completes metadata preflight
//! for every selector before requesting the first destination, then executes targets in
//! deterministic plan order. If a target fails, earlier completed outcomes are retained,
//! the current target may already have partial output, and later targets are not started.
//!
//! # Typed errors
//!
//! [`PgDumpError`] is the detailed error type. [`PgDumpError::category`] exposes a stable
//! high-level [`ErrorCategory`], while helpers such as [`PgDumpError::dump_id`],
//! [`PgDumpError::row_number`], [`PgDumpError::byte_offset`], and
//! [`PgDumpError::limit_context`] expose machine-readable context without parsing display
//! text. Variants wrapping lower-level I/O, decompression, output, COPY-input, or UTF-8
//! failures preserve those failures through [`std::error::Error::source`].
//!
//! # CLI encoding boundary
//!
//! The Rust API remains byte-oriented. The `pgdumpx find` and `pgdumpx extract` CLI
//! commands accept UTF-8 schema/table arguments and use an exact `SCHEMA.TABLE` selector:
//! exactly one ASCII `.` separator, two non-empty components, and no SQL identifier
//! quoting. `find` also requires UTF-8 column/value arguments and compares their UTF-8
//! bytes with logical post-unescape field bytes. Raw `extract` output remains binary-safe.
//!
//! # Example: production row path
//!
//! The following compiles against the same public path used by the CLI. The search is a
//! bounded sequential scan of one selected table-data entry.
//!
//! ```no_run
//! use pgdumpx::{Archive, FieldRef, ScanLimits};
//! use std::{fs::File, io::BufReader};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let file = File::open("backup.dump")?;
//! let mut archive = Archive::open(BufReader::new(file))?;
//! let mut rows = archive.table_rows(b"public", b"orders")?;
//! let Some(order_number) = rows.column_index(b"order_number")? else {
//!     return Ok(());
//! };
//!
//! let limits = ScanLimits::unlimited()
//!     .with_max_rows(100_000)
//!     .with_max_decompressed_bytes(64 * 1024 * 1024);
//! let matched = rows.find_first_with_limits(limits, |row| {
//!     row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
//! })?;
//!
//! if let Some(row) = matched {
//!     assert!(!row.fields().is_empty());
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]

mod archive;
mod copy;
mod copy_metadata;
mod custom;
mod entry;
mod error;
mod error_taxonomy;
mod extraction_plan;
mod file;
mod io;
mod limits;
mod model;
mod raw_entry;
mod selector;
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
pub use extraction_plan::{
    ExtractionExecutionError, ExtractionOutcome, ExtractionPlan, ExtractionPlanError,
    ExtractionTarget, ResolvedExtractionPlan,
};
pub use limits::{Limits, ScanLimits};
pub use model::{
    ArchiveHeader, ArchiveString, ArchiveTimestamp, ArchiveVersion, Compression, DataLocation,
    DumpId, Section, TableRef, TocEntry,
};
pub use raw_entry::{BoundedEntryDataReader, EntryReadLimits};
pub use selector::TableSelector;
pub use table_rows::TableRowReader;
