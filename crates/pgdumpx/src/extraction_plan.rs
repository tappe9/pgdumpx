use crate::{Archive, EntryReadLimits, PgDumpError, TableRef, TableSelector};
use std::{error::Error, fmt};

/// Errors produced while constructing a reusable [`ExtractionPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtractionPlanError {
    /// The same exact byte-oriented table selector appears more than once.
    DuplicateSelector { selector: TableSelector },
}

impl ExtractionPlanError {
    /// Returns the selector associated with this plan-construction failure.
    pub const fn selector(&self) -> &TableSelector {
        match self {
            Self::DuplicateSelector { selector } => selector,
        }
    }
}

impl fmt::Display for ExtractionPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSelector { selector } => write!(
                formatter,
                "duplicate table selector in extraction plan: schema={:?}, table={:?}",
                selector.schema(),
                selector.name()
            ),
        }
    }
}

impl Error for ExtractionPlanError {}

/// An owned reusable plan describing ordered table extraction intent.
///
/// A plan stores only logical [`TableSelector`] values and the existing raw-output
/// [`EntryReadLimits`] policy. It never stores dump IDs, file offsets, readers, or other
/// archive-specific seek state, so the same plan can be preflighted independently against
/// multiple archives. Selector order is preserved exactly as supplied by the caller.
///
/// Duplicate selectors are rejected during construction. [`ExtractionPlan::preflight`]
/// resolves all selectors and their related `TABLE DATA` entries using archive metadata
/// only; it does not seek to or decompress payloads. Extraction execution and output-file
/// policy are intentionally outside this abstraction.
///
/// # Example
///
/// ```no_run
/// use pgdumpx::{Archive, EntryReadLimits, ExtractionPlan, TableSelector};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let plan = ExtractionPlan::with_entry_read_limits(
///     vec![
///         TableSelector::new(b"public", b"orders"),
///         TableSelector::new(b"public", b"customers"),
///     ],
///     EntryReadLimits::unlimited().with_max_decompressed_bytes(512 * 1024 * 1024),
/// )?;
///
/// let archive = Archive::open_path("backup.dump")?;
/// let resolved = plan.preflight(&archive)?;
/// assert_eq!(resolved.tables().len(), 2);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionPlan {
    selectors: Vec<TableSelector>,
    entry_read_limits: EntryReadLimits,
}

impl ExtractionPlan {
    /// Creates a plan using [`EntryReadLimits::default`] for later raw extraction.
    ///
    /// The caller's selector order is retained. Repeating an exactly equal selector is
    /// rejected deterministically before any archive or payload I/O occurs.
    pub fn new(selectors: Vec<TableSelector>) -> Result<Self, ExtractionPlanError> {
        Self::with_entry_read_limits(selectors, EntryReadLimits::default())
    }

    /// Creates a plan with an explicit existing raw-entry output policy.
    ///
    /// `entry_read_limits` is carried unchanged for later execution; plan construction and
    /// preflight do not consume its budget or invent successful truncation semantics.
    pub fn with_entry_read_limits(
        selectors: Vec<TableSelector>,
        entry_read_limits: EntryReadLimits,
    ) -> Result<Self, ExtractionPlanError> {
        for (index, selector) in selectors.iter().enumerate() {
            if selectors[..index].contains(selector) {
                return Err(ExtractionPlanError::DuplicateSelector {
                    selector: selector.clone(),
                });
            }
        }

        Ok(Self {
            selectors,
            entry_read_limits,
        })
    }

    /// Returns selectors in the caller-defined deterministic order.
    pub fn selectors(&self) -> &[TableSelector] {
        &self.selectors
    }

    /// Returns the raw-entry output policy carried by this plan.
    pub const fn entry_read_limits(&self) -> EntryReadLimits {
        self.entry_read_limits
    }

    /// Resolves every requested table and related `TABLE DATA` entry using metadata only.
    ///
    /// The returned [`ResolvedExtractionPlan`] borrows metadata from `archive`; the owned
    /// plan itself remains archive-independent and can be reused. Each call resolves the
    /// selectors again, so dump IDs and offsets are never assumed portable across archives.
    ///
    /// If any table is missing, [`PgDumpError::TableNotFound`] is returned. If a table has
    /// no related table-data entry, [`PgDumpError::TableDataEntryUnavailable`] is returned.
    /// No entry reader is opened before or during these checks, so failure occurs before
    /// selected-entry seeks or decompression.
    pub fn preflight<'a, R>(
        &self,
        archive: &'a Archive<R>,
    ) -> Result<ResolvedExtractionPlan<'a>, PgDumpError> {
        let mut tables = Vec::with_capacity(self.selectors.len());
        for selector in &self.selectors {
            let table = archive
                .resolve_table(selector)
                .ok_or(PgDumpError::TableNotFound)?;
            if table.data_entry_id().is_none() {
                return Err(PgDumpError::TableDataEntryUnavailable {
                    table_id: table.table_entry_id().as_i32(),
                });
            }
            tables.push(table);
        }

        Ok(ResolvedExtractionPlan {
            tables,
            entry_read_limits: self.entry_read_limits,
        })
    }
}

/// Metadata resolved for one [`ExtractionPlan`] preflight against a specific archive.
///
/// This value preserves selector order and borrows [`TableRef`] handles from the archive.
/// It contains no open entry readers and performs no payload I/O. The logical
/// [`ExtractionPlan`] should be retained when the same intent needs to be resolved against
/// another archive instance.
#[derive(Debug, Clone)]
pub struct ResolvedExtractionPlan<'a> {
    tables: Vec<TableRef<'a>>,
    entry_read_limits: EntryReadLimits,
}

impl<'a> ResolvedExtractionPlan<'a> {
    /// Returns resolved tables in the original selector order.
    pub fn tables(&self) -> &[TableRef<'a>] {
        &self.tables
    }

    /// Returns the raw-entry output policy carried from the logical plan.
    pub const fn entry_read_limits(&self) -> EntryReadLimits {
        self.entry_read_limits
    }
}
