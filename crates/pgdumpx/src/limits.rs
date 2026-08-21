/// Finite structural limits applied while opening archives and parsing COPY rows.
///
/// All fields are private and every configuration is finite. [`Default`] preserves
/// the compatibility-oriented bounds used by the initial v0.1 implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    max_toc_entries: u64,
    max_string_bytes: usize,
    max_dependencies_per_entry: u64,
    max_row_bytes: usize,
    max_fields_per_row: usize,
}

impl Limits {
    const DEFAULT_MAX_TOC_ENTRIES: u64 = 100_000;
    const DEFAULT_MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_DEPENDENCIES_PER_ENTRY: u64 = 100_000;
    const DEFAULT_MAX_ROW_BYTES: usize = 16 * 1024 * 1024;
    const DEFAULT_MAX_FIELDS_PER_ROW: usize = 4 * 1024;

    /// Returns compatibility-oriented finite defaults for v0.1 archives.
    pub const fn default_compatible() -> Self {
        Self {
            max_toc_entries: Self::DEFAULT_MAX_TOC_ENTRIES,
            max_string_bytes: Self::DEFAULT_MAX_STRING_BYTES,
            max_dependencies_per_entry: Self::DEFAULT_MAX_DEPENDENCIES_PER_ENTRY,
            max_row_bytes: Self::DEFAULT_MAX_ROW_BYTES,
            max_fields_per_row: Self::DEFAULT_MAX_FIELDS_PER_ROW,
        }
    }

    /// Returns the maximum number of TOC entries accepted while opening an archive.
    pub const fn max_toc_entries(self) -> u64 {
        self.max_toc_entries
    }

    /// Returns the maximum encoded byte length of one archive metadata string.
    pub const fn max_string_bytes(self) -> usize {
        self.max_string_bytes
    }

    /// Returns the maximum dependency count accepted for one TOC entry.
    pub const fn max_dependencies_per_entry(self) -> u64 {
        self.max_dependencies_per_entry
    }

    /// Returns the maximum physical byte length of one COPY text row.
    pub const fn max_row_bytes(self) -> usize {
        self.max_row_bytes
    }

    /// Returns the maximum field count accepted in one COPY row and its metadata.
    pub const fn max_fields_per_row(self) -> usize {
        self.max_fields_per_row
    }

    /// Returns a configuration with a different maximum TOC entry count.
    #[must_use]
    pub const fn with_max_toc_entries(mut self, value: u64) -> Self {
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
    pub const fn with_max_dependencies_per_entry(mut self, value: u64) -> Self {
        self.max_dependencies_per_entry = value;
        self
    }

    /// Returns a configuration with a different maximum physical COPY row length.
    #[must_use]
    pub const fn with_max_row_bytes(mut self, value: usize) -> Self {
        self.max_row_bytes = value;
        self
    }

    /// Returns a configuration with a different maximum COPY field count.
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
