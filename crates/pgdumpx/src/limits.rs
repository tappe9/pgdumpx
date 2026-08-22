/// Finite structural limits used while opening archives and parsing COPY metadata/rows.
///
/// These limits protect individual structures and allocations: TOC cardinality,
/// archive-string length, per-entry dependency count, one physical COPY row, and one
/// row/column layout. They do **not** bound the total amount of decompressed data or
/// number of rows processed by a long scan; use [`ScanLimits`] for that. They also do
/// not bound raw selected-entry output; use [`crate::EntryReadLimits`] for that.
///
/// [`Default`] uses compatibility-oriented finite v0.1 bounds. A configured value is
/// an inclusive maximum: an observed value equal to the limit is accepted and the
/// first value above it is rejected with a typed resource error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    max_toc_entries: usize,
    max_string_bytes: usize,
    max_dependencies_per_entry: usize,
    max_row_bytes: usize,
    max_fields_per_row: usize,
}

impl Limits {
    const DEFAULT_MAX_TOC_ENTRIES: usize = 100_000;
    const DEFAULT_MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: usize = 100_000;
    const DEFAULT_MAX_ROW_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_FIELDS_PER_ROW: usize = 4 * 1024;

    /// Returns the compatibility-oriented finite defaults used by [`Default`].
    ///
    /// v0.1 defaults allow 100,000 TOC entries, 16 MiB per archive string,
    /// 100,000 dependencies per TOC entry, 16 MiB per physical COPY row, and
    /// 4,096 fields/columns per row layout.
    pub const fn default_compatible() -> Self {
        Self {
            max_toc_entries: Self::DEFAULT_MAX_TOC_ENTRIES,
            max_string_bytes: Self::DEFAULT_MAX_STRING_BYTES,
            max_dependencies_per_entry: Self::DEFAULT_MAX_DEPENDENCIES_PER_ENTRY,
            max_row_bytes: Self::DEFAULT_MAX_ROW_BYTES,
            max_fields_per_row: Self::DEFAULT_MAX_FIELDS_PER_ROW,
        }
    }

    /// Returns the inclusive maximum number of TOC entries accepted while opening.
    pub const fn max_toc_entries(self) -> usize {
        self.max_toc_entries
    }

    /// Returns the inclusive maximum encoded byte length of one archive metadata string.
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    /// Returns the inclusive maximum dependency count accepted for one TOC entry.
    pub const fn max_dependencies_per_entry(self) -> usize {
        self.max_dependencies_per_entry
    }

    /// Returns the inclusive maximum physical byte length of one COPY text row.
    ///
    /// This counts the physical row representation before COPY escape decoding, not
    /// logical decoded field bytes and not total scan bytes.
    pub const fn max_row_bytes(self) -> usize {
        self.max_row_bytes
    }

    /// Returns the inclusive maximum field count in one COPY row or column layout.
    pub const fn max_fields_per_row(self) -> usize {
        self.max_fields_per_row
    }

    /// Returns a configuration with a different maximum TOC entry count.
    #[must_use]
    pub const fn with_max_toc_entries(mut self, value: usize) -> Self {
        self.max_toc_entries = value;
        self
    }

    /// Returns a configuration with a different maximum archive-string byte length.
    #[must_use]
    pub const fn with_max_string_bytes(mut self, value: usize) -> Self {
        self.max_string_bytes = value;
        self
    }

    /// Returns a configuration with a different maximum dependency count per entry.
    #[must_use]
    pub const fn with_max_dependencies_per_entry(mut self, value: usize) -> Self {
        self.max_dependencies_per_entry = value;
        self
    }

    /// Returns a configuration with a different maximum physical COPY row length.
    #[must_use]
    pub const fn with_max_row_bytes(mut self, value: usize) -> Self {
        self.max_row_bytes = value;
        self
    }

    /// Returns a configuration with a different maximum COPY field/column count.
    #[must_use]
    pub const fn with_max_fields_per_row(mut self, value: usize) -> Self {
        self.max_fields_per_row = value;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::default_compatible()
    }
}

/// Optional total-work budgets for streaming COPY row scans.
///
/// Unlike [`Limits`], these budgets accumulate work across rows. `max_rows` counts
/// complete rows evaluated/yielded by the scan. `max_decompressed_bytes` counts
/// physical decompressed COPY bytes actually consumed by the parser, including field
/// separators, row terminators, and the COPY terminator when consumed. Decoder or
/// buffered-reader lookahead that has not been consumed by the parser does not count,
/// and logical post-unescape field length is not used.
///
/// A configured value is inclusive. A row that would make either counter exceed its
/// limit is not exposed to the caller or predicate. [`ScanLimits::unlimited`] and
/// [`Default`] apply neither budget; applications handling untrusted or very large
/// inputs should choose explicit bounds appropriate to the operation.
///
/// Raw byte extraction uses the separate [`crate::EntryReadLimits`] budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    max_rows: Option<u64>,
    max_decompressed_bytes: Option<u64>,
}

impl ScanLimits {
    /// Returns scan limits with neither total-work budget enabled.
    pub const fn unlimited() -> Self {
        Self {
            max_rows: None,
            max_decompressed_bytes: None,
        }
    }

    /// Returns the inclusive maximum number of complete rows, if configured.
    pub const fn max_rows(self) -> Option<u64> {
        self.max_rows
    }

    /// Returns the inclusive maximum parser-consumed decompressed byte count, if configured.
    pub const fn max_decompressed_bytes(self) -> Option<u64> {
        self.max_decompressed_bytes
    }

    /// Returns a configuration with a maximum complete-row budget.
    ///
    /// A value of `N` permits exactly `N` complete rows; the next row is rejected
    /// before it is exposed to the caller or predicate.
    #[must_use]
    pub const fn with_max_rows(mut self, value: u64) -> Self {
        self.max_rows = Some(value);
        self
    }

    /// Returns a configuration with a maximum parser-consumed decompressed-byte budget.
    ///
    /// A value of `N` permits exactly `N` consumed COPY bytes. Consuming byte `N + 1`
    /// produces a typed scan resource error before a crossing row is exposed.
    #[must_use]
    pub const fn with_max_decompressed_bytes(mut self, value: u64) -> Self {
        self.max_decompressed_bytes = Some(value);
        self
    }
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self::unlimited()
    }
}
