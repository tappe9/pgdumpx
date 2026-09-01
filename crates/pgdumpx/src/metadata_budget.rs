use crate::{DumpId, Limits, PgDumpError};

/// Checked cumulative accounting shared by archive metadata parsing and index construction.
#[derive(Debug)]
pub(crate) struct MetadataBudget {
    string_bytes: u64,
    dependencies: u64,
    index_bytes: u64,
    max_string_bytes: u64,
    max_dependencies: u64,
    max_index_bytes: u64,
}

impl MetadataBudget {
    pub(crate) fn new(limits: Limits) -> Result<Self, PgDumpError> {
        Ok(Self {
            string_bytes: 0,
            dependencies: 0,
            index_bytes: 0,
            max_string_bytes: to_u64(limits.max_metadata_string_bytes(), 0)?,
            max_dependencies: to_u64(limits.max_metadata_dependencies(), 0)?,
            max_index_bytes: to_u64(limits.max_metadata_index_bytes(), 0)?,
        })
    }

    /// Charges bytes that will remain owned by header or TOC [`crate::ArchiveString`] values.
    pub(crate) fn charge_string_bytes(
        &mut self,
        amount: usize,
        offset: u64,
    ) -> Result<(), PgDumpError> {
        let amount = to_u64(amount, offset)?;
        let attempted = self
            .string_bytes
            .checked_add(amount)
            .ok_or(PgDumpError::ArithmeticOverflow { offset })?;
        if attempted > self.max_string_bytes {
            return Err(PgDumpError::MetadataStringByteLimitExceeded {
                limit: self.max_string_bytes,
                attempted,
                offset,
            });
        }
        self.string_bytes = attempted;
        Ok(())
    }

    /// Charges one dependency immediately before it is retained in a TOC dependency vector.
    pub(crate) fn charge_dependency(
        &mut self,
        entry_id: DumpId,
        offset: u64,
    ) -> Result<(), PgDumpError> {
        let attempted = self
            .dependencies
            .checked_add(1)
            .ok_or(PgDumpError::ArithmeticOverflow { offset })?;
        if attempted > self.max_dependencies {
            return Err(PgDumpError::MetadataDependencyLimitExceeded {
                entry_id: entry_id.as_i32(),
                limit: self.max_dependencies,
                attempted,
                offset,
            });
        }
        self.dependencies = attempted;
        Ok(())
    }

    /// Charges variable-length names retained by derived metadata and lookup indexes.
    pub(crate) fn charge_index_bytes(
        &mut self,
        amount: usize,
        context: &'static str,
    ) -> Result<(), PgDumpError> {
        let amount = to_u64(amount, 0)?;
        let attempted = self
            .index_bytes
            .checked_add(amount)
            .ok_or(PgDumpError::ArithmeticOverflow { offset: 0 })?;
        if attempted > self.max_index_bytes {
            return Err(PgDumpError::MetadataIndexByteLimitExceeded {
                context,
                limit: self.max_index_bytes,
                attempted,
            });
        }
        self.index_bytes = attempted;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_usage_for_test(
        limits: Limits,
        string_bytes: u64,
        dependencies: u64,
        index_bytes: u64,
    ) -> Self {
        Self {
            string_bytes,
            dependencies,
            index_bytes,
            max_string_bytes: u64::try_from(limits.max_metadata_string_bytes()).unwrap_or(u64::MAX),
            max_dependencies: u64::try_from(limits.max_metadata_dependencies()).unwrap_or(u64::MAX),
            max_index_bytes: u64::try_from(limits.max_metadata_index_bytes()).unwrap_or(u64::MAX),
        }
    }
}

fn to_u64(value: usize, offset: u64) -> Result<u64, PgDumpError> {
    u64::try_from(value).map_err(|_| PgDumpError::ArithmeticOverflow { offset })
}
