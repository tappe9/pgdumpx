use crate::{
    Column, CopyRowReader, DumpId, EntryDataReader, FieldRef, Limits, OwnedRow, PgDumpError, Row,
    ScanLimits, copy_metadata::TableDataMetadata,
};
use std::io::Read;

/// Result of a named-column exact-equality row search.
///
/// `Match` owns only the first matching row. `NoMatch` means the named column was valid
/// but no remaining row matched. `ColumnNotFound` means valid COPY column metadata did not
/// contain the requested exact byte name; unavailable or malformed metadata remains a
/// typed [`PgDumpError`] instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColumnEqualityResult {
    /// The first matching row, copied into owned storage.
    Match(OwnedRow),
    /// The named column exists, but no remaining row matched the requested value.
    NoMatch,
    /// Valid COPY metadata did not contain the requested exact byte column name.
    ColumnNotFound,
}

/// A lending COPY-text row stream for one selected `TABLE DATA` entry.
///
/// [`crate::Archive::table_rows`] constructs this reader after exact byte-oriented
/// table lookup and table-data representation validation. It composes validated
/// selected-entry seeking, custom chunk framing, streaming decompression, and
/// [`CopyRowReader`] without buffering the complete entry or table.
///
/// When valid COPY column metadata is available, every parsed row is checked against
/// that positional field count before it is returned or passed to a caller predicate.
/// Metadata-unavailable or malformed COPY streams remain positionally readable as
/// documented; this reader does not infer a schema from payload rows.
///
/// Rows borrow reusable parser storage. A [`Row`] and its field slices remain valid
/// only until this reader is mutably borrowed again, so this type intentionally does
/// not implement `Iterator`. Use [`OwnedRow`] when a row must outlive advancement.
///
/// A newly created `TableRowReader` starts at the beginning of the selected table-data
/// body. After calls to [`TableRowReader::next_row`], searches continue from the current
/// stream position; they do not rewind the archive.
///
/// Any row-reading error makes the composed COPY reader terminal. The original call
/// returns its existing typed error, while subsequent row-reading or search calls return
/// `Ok(None)` and never expose bytes from the rejected record or a later record.
pub struct TableRowReader<'a, R> {
    data_id: DumpId,
    metadata: &'a TableDataMetadata,
    expected_field_count: Option<usize>,
    next_row_number: u64,
    failed: bool,
    rows: CopyRowReader<EntryDataReader<'a, R>>,
}

impl<'a, R: Read> TableRowReader<'a, R> {
    pub(crate) fn new_with_limits(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
        limits: Limits,
    ) -> Self {
        Self::new_with_scan_limits(data_id, metadata, entry, limits, ScanLimits::unlimited())
    }

    pub(crate) fn new_with_scan_limits(
        data_id: DumpId,
        metadata: &'a TableDataMetadata,
        entry: EntryDataReader<'a, R>,
        limits: Limits,
        scan_limits: ScanLimits,
    ) -> Self {
        let expected_field_count = metadata.columns(data_id).ok().map(|columns| columns.len());
        Self {
            data_id,
            metadata,
            expected_field_count,
            next_row_number: 1,
            failed: false,
            rows: CopyRowReader::with_limits_and_scan_limits(entry, limits, scan_limits),
        }
    }

    /// Returns COPY columns in the exact positional order used by parsed rows.
    ///
    /// This reads metadata parsed while the archive was opened; it does not scan the
    /// entry body. `CopyColumnMetadataUnavailable`, `MalformedCopyStatement`, and
    /// `UnsupportedTableDataRepresentation` remain distinct errors. Positional row
    /// iteration can still be available for readable COPY data when only the column
    /// metadata is unavailable or malformed.
    pub fn columns(&self) -> Result<&[Column], PgDumpError> {
        self.metadata.columns(self.data_id)
    }

    /// Resolves a byte-oriented COPY column name to its zero-based field index.
    ///
    /// `Ok(Some(index))` means valid metadata contained an exact byte match.
    /// `Ok(None)` means the metadata was valid but that name was absent. Metadata
    /// unavailable/malformed and unsupported representations are returned as distinct
    /// typed errors rather than being conflated with a missing column.
    pub fn column_index(&self, name: &[u8]) -> Result<Option<usize>, PgDumpError> {
        self.metadata.column_index(self.data_id, name)
    }

    /// Parses and lends the next logical row from the selected table-data entry.
    ///
    /// A returned row borrows reusable parser storage and remains valid only until the
    /// next mutable operation on this reader. Fields are byte-oriented logical COPY
    /// values after escape decoding; they are not required to be UTF-8.
    ///
    /// When valid COPY column metadata is available, the row is returned only if its
    /// field count matches that metadata. A short or long row returns
    /// [`PgDumpError::CopyRowFieldCountMismatch`] with dump ID, row number, expected
    /// count, and actual count before the row is exposed.
    ///
    /// Any error makes row iteration terminal because the underlying record may have
    /// been partially consumed. Later `next_row` and search calls return `Ok(None)`;
    /// the original failing call retains its typed archive, parser, limit, or I/O context.
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
        if self.failed {
            return Ok(None);
        }

        let data_id = self.data_id.as_i32();
        let row_number = self.next_row_number;
        let expected_field_count = self.expected_field_count;
        let Some(row) = self.rows.next_row()? else {
            return Ok(None);
        };

        if let Some(expected) = expected_field_count {
            let actual = row.len();
            if actual != expected {
                self.failed = true;
                return Err(PgDumpError::CopyRowFieldCountMismatch {
                    dump_id: data_id,
                    row: row_number,
                    expected: u64::try_from(expected).unwrap_or(u64::MAX),
                    actual: u64::try_from(actual).unwrap_or(u64::MAX),
                });
            }
        }

        self.next_row_number = row_number
            .checked_add(1)
            .ok_or(PgDumpError::CopyRowNumberOverflow { row: row_number })?;
        Ok(Some(row))
    }

    /// Sequentially scans from the current stream position for the first match.
    ///
    /// Non-matching rows reuse lending parser storage. On the first predicate result
    /// of `true`, only that row is copied into [`OwnedRow`] and no later row is read.
    /// `Ok(None)` means the remaining stream ended without a match.
    ///
    /// Valid column metadata is checked before the caller predicate sees each row, so
    /// malformed short or long rows return a structural error rather than appearing as
    /// a non-match.
    ///
    /// This is not an indexed row lookup. A fresh `TableRowReader` scans from the
    /// beginning of the selected table-data entry; after prior row reads it scans from
    /// the current position. A late or absent match can require processing all remaining
    /// selected data. Use [`TableRowReader::find_first_with_limits`] to add an explicit
    /// operation-level work budget.
    pub fn find_first<F>(&mut self, predicate: F) -> Result<Option<OwnedRow>, PgDumpError>
    where
        F: FnMut(&Row<'_>) -> bool,
    {
        self.find_first_with_limits(ScanLimits::unlimited(), predicate)
    }

    /// Sequentially scans for the first match with operation-level [`ScanLimits`].
    ///
    /// The limits are measured from this call's current stream position. Row limits
    /// count complete rows before predicate invocation; a crossing row is not exposed.
    /// Byte accounting counts physical decompressed COPY bytes consumed by the parser,
    /// including separators and terminators, rather than logical decoded field length or
    /// decoder/buffered-reader lookahead. The matching row counts toward the budgets,
    /// and the scan stops without consuming rows after a match.
    ///
    /// Valid column metadata is checked before `predicate` is called. A field-count
    /// mismatch terminates this reader and is returned instead of a match/no-match result.
    pub fn find_first_with_limits<F>(
        &mut self,
        scan_limits: ScanLimits,
        mut predicate: F,
    ) -> Result<Option<OwnedRow>, PgDumpError>
    where
        F: FnMut(&Row<'_>) -> bool,
    {
        if self.failed {
            return Ok(None);
        }

        let expected_field_count = self.expected_field_count;
        let mut next_row_number = self.next_row_number;
        let mut mismatch = None;
        let result = self.rows.find_first_with_limits(scan_limits, |row| {
            let row_number = next_row_number;
            next_row_number = next_row_number.saturating_add(1);

            if let Some(expected) = expected_field_count {
                let actual = row.len();
                if actual != expected {
                    mismatch = Some((row_number, expected, actual));
                    return true;
                }
            }

            predicate(row)
        });
        self.next_row_number = next_row_number;

        if let Some((row, expected, actual)) = mismatch {
            self.failed = true;
            return Err(PgDumpError::CopyRowFieldCountMismatch {
                dump_id: self.data_id.as_i32(),
                row,
                expected: u64::try_from(expected).unwrap_or(u64::MAX),
                actual: u64::try_from(actual).unwrap_or(u64::MAX),
            });
        }

        result
    }

    /// Finds the first row whose named column exactly equals one logical COPY value.
    ///
    /// The column name is resolved exactly once from already-parsed metadata before any
    /// row is scanned. [`FieldRef::Bytes`] compares logical post-unescape bytes exactly;
    /// [`FieldRef::Null`] matches only SQL NULL, so empty bytes and literal `b"\\N"` bytes
    /// remain distinct. No UTF-8 conversion, collation, SQL coercion, or typed comparison
    /// is performed.
    ///
    /// This convenience method preserves the same sequential scan, early termination,
    /// reader-wide limits, typed errors, field-count validation, and owned-match behavior
    /// as [`Self::find_first`].
    pub fn find_first_equal(
        &mut self,
        column: &[u8],
        expected: FieldRef<'_>,
    ) -> Result<ColumnEqualityResult, PgDumpError> {
        self.find_first_equal_with_limits(ScanLimits::unlimited(), column, expected)
    }

    /// Finds the first exact named-column equality match with operation-level limits.
    ///
    /// Column resolution happens before the operation budget starts scanning rows. If the
    /// name is absent, [`ColumnEqualityResult::ColumnNotFound`] is returned without row
    /// consumption. Metadata failures remain their existing typed [`PgDumpError`] values.
    /// For a valid column, this delegates directly to [`Self::find_first_with_limits`], so
    /// row/decompressed-byte accounting, field-count validation, and early termination are
    /// unchanged.
    pub fn find_first_equal_with_limits(
        &mut self,
        scan_limits: ScanLimits,
        column: &[u8],
        expected: FieldRef<'_>,
    ) -> Result<ColumnEqualityResult, PgDumpError> {
        let Some(column_index) = self.column_index(column)? else {
            return Ok(ColumnEqualityResult::ColumnNotFound);
        };

        match self
            .find_first_with_limits(scan_limits, |row| row.field(column_index) == Some(expected))?
        {
            Some(row) => Ok(ColumnEqualityResult::Match(row)),
            None => Ok(ColumnEqualityResult::NoMatch),
        }
    }

    #[cfg(test)]
    pub(crate) const fn consumed_input_bytes(&self) -> u64 {
        self.rows.consumed_input_bytes()
    }
}
