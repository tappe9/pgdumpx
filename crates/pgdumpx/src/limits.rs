/// Provisional Alpha 1 bound for one decoded archive string.
const ALPHA1_MAX_ARCHIVE_STRING_BYTES: usize = 16 * 1024 * 1024;
/// Provisional Alpha 1 bound for the number of TOC entries.
const ALPHA1_MAX_TOC_ENTRIES: usize = 100_000;
/// Provisional Alpha 1 bound for dependencies attached to one TOC entry.
const ALPHA1_MAX_DEPENDENCIES_PER_ENTRY: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchiveStringLimit {
    max_bytes: usize,
}

impl ArchiveStringLimit {
    pub(crate) const fn new(max_bytes: usize) -> Self {
        Self { max_bytes }
    }

    pub(crate) const fn max_bytes(self) -> usize {
        self.max_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetadataLimits {
    string: ArchiveStringLimit,
    max_toc_entries: usize,
    max_dependencies_per_entry: usize,
}

impl MetadataLimits {
    pub(crate) const fn new(
        string: ArchiveStringLimit,
        max_toc_entries: usize,
        max_dependencies_per_entry: usize,
    ) -> Self {
        Self {
            string,
            max_toc_entries,
            max_dependencies_per_entry,
        }
    }

    pub(crate) const fn string(self) -> ArchiveStringLimit {
        self.string
    }

    pub(crate) const fn max_toc_entries(self) -> usize {
        self.max_toc_entries
    }

    pub(crate) const fn max_dependencies_per_entry(self) -> usize {
        self.max_dependencies_per_entry
    }
}

pub(crate) const ALPHA1_ARCHIVE_STRING_LIMIT: ArchiveStringLimit =
    ArchiveStringLimit::new(ALPHA1_MAX_ARCHIVE_STRING_BYTES);

pub(crate) const ALPHA1_METADATA_LIMITS: MetadataLimits = MetadataLimits::new(
    ALPHA1_ARCHIVE_STRING_LIMIT,
    ALPHA1_MAX_TOC_ENTRIES,
    ALPHA1_MAX_DEPENDENCIES_PER_ENTRY,
);
