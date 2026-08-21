use crate::{
    Column, CopyRowReader, DumpId, EntryDataReader, Limits, OwnedRow, PgDumpError, Row,
    copy_metadata::TableDataMetadata,
};
use std::io::Read;

/// A lending stream of COPY text rows for one selected table-data entry.
///
/// The reader composes validated archive seeking, custom chunk framing,
/// streaming decompression, and COPY text parsing without buffering the
/// complete entry or table data.
pub struct TableRowReader<'a, R> {
    data_id: DumpId,
    metadata: &'a TableDataMetadata,
    rows: CopyRowReader<EntryDataReader<'a, R>>,
}

impl<'a, R: Read> TableRowReader<'a, R> {
    pub(crate) fn new_with_limits(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
        limits: Limits,
    ) -> Self {
        Self {
            data_id,
            metadata,
            rows: CopyRowReader::with_limits(entry, limits),
        }
    }

    /// Returns COPY columns in the exact positional order used by parsed rows.
    ///
    /// Positional row iteration may remain available when column metadata is
    /// unavailable or malformed; in that case this method returns the
    /// corresponding typed metadata error.
    pub fn columns(&self) -> Result<&[Column], PgDumpError> {
        self.metadata.columns(self.data_id)
    }

    /// Resolves a byte-oriented COPY column name to its zero-based field index.
    pub fn column_index(&self, name: &[u8]) -> Result<Option<usize>, PgDumpError> {
        self.metadata.column_index(self.data_id, name)
    }

    /// Parses and lends the next logical row from the selected table-data entry.
    ///
    /// A returned row borrows reusable parser storage and remains valid only
    /// until the next mutable operation on this reader.
    ///
    /// ```compile_fail
    /// use pgdumpx::{PgDumpError, TableRowReader};
    /// use std::io::Read;
    ///
    /// fn cannot_hold_two_rows<R: Read>(
    ///     rows: &mut TableRowReader<'_, R>,
    /// ) -> Result<(), PgDumpError> {
    ///     let first = rows.next_row()?.unwrap();
    ///     let _second = rows.next_row()?;
    ///     println!("{first:?}");
    ///     Ok(())
    /// }
    /// ```
    pub fn next_row(&mut self) -> Result<Option<Row<'_>>, PgDumpError> {
        self.rows.next_row()
    }

    /// Returns the first row for which `predicate` evaluates to `true`.
    ///
    /// Non-matching rows continue to borrow reusable parser storage. Only the
    /// matched row is copied into an [`OwnedRow`], and the stream is not read
    /// after that match.
    pub fn find_first<F>(&mut self, mut predicate: F) -> Result<Option<OwnedRow>, PgDumpError>
    where
        F: FnMut(&Row<'_>) -> bool,
    {
        while let Some(row) = self.rows.next_row()? {
            if predicate(&row) {
                return OwnedRow::try_from_borrowed(&row).map(Some);
            }
        }
        Ok(None)
    }

    #[cfg(test)]
    pub(crate) const fn consumed_input_bytes(&self) -> u64 {
        self.rows.consumed_input_bytes()
    }
}
