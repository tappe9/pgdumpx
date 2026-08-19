/// Provisional Alpha 1 bound for one decoded archive string.
const ALPHA1_MAX_ARCHIVE_STRING_BYTES: usize = 16 * 1024 * 1024;

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

pub(crate) const ALPHA1_ARCHIVE_STRING_LIMIT: ArchiveStringLimit =
    ArchiveStringLimit::new(ALPHA1_MAX_ARCHIVE_STRING_BYTES);
