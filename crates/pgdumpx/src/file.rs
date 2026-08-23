use crate::{Archive, Limits, PgDumpError};
use std::{fs::File, io::BufReader, path::Path};

impl Archive<BufReader<File>> {
    /// Opens a PostgreSQL custom-format archive from a filesystem path.
    ///
    /// This is a convenience wrapper around the same generic [`Archive::open`] parser
    /// used for arbitrary `Read + Seek` sources. The file is wrapped in a standard
    /// [`BufReader`], archive metadata is parsed eagerly, and selected entry payloads
    /// remain lazy until an entry/row API is used.
    ///
    /// File-opening failures are returned as [`PgDumpError::Io`] with the underlying
    /// [`std::io::Error`] preserved as the error source.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pgdumpx::Archive;
    ///
    /// # fn main() -> Result<(), pgdumpx::PgDumpError> {
    /// let archive = Archive::open_path("backup.dump")?;
    /// assert!(!archive.entries().is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, PgDumpError> {
        Self::open_path_with_limits(path, Limits::default())
    }

    /// Opens a filesystem archive with explicit structural [`Limits`].
    ///
    /// This only adds `File::open` and [`BufReader`] around
    /// [`Archive::open_with_limits`]; it does not introduce a filesystem-specific parser,
    /// decompression path, or payload-read policy. Raw-output and row-scan budgets remain
    /// controlled separately by [`crate::EntryReadLimits`] and [`crate::ScanLimits`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pgdumpx::{Archive, Limits};
    ///
    /// # fn main() -> Result<(), pgdumpx::PgDumpError> {
    /// let limits = Limits::default_compatible().with_max_toc_entries(50_000);
    /// let archive = Archive::open_path_with_limits("backup.dump", limits)?;
    /// assert!(!archive.entries().is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_path_with_limits(
        path: impl AsRef<Path>,
        limits: Limits,
    ) -> Result<Self, PgDumpError> {
        let file = File::open(path).map_err(|source| PgDumpError::Io { offset: 0, source })?;
        Self::open_with_limits(BufReader::new(file), limits)
    }
}
