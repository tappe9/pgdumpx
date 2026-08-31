use crate::{Archive, DumpId, EntryReadLimits, PgDumpError, TableRef, TableSelector};
use std::{
    collections::{HashSet, TryReserveError},
    error::Error,
    fmt,
    hash::Hash,
    io::{self, Read, Seek, Write},
};

/// Errors produced while constructing a reusable [`ExtractionPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtractionPlanError {
    /// The same exact byte-oriented table selector appears more than once.
    DuplicateSelector { selector: TableSelector },
    /// Memory for the auxiliary duplicate-selector index could not be reserved.
    ///
    /// `selector` retains the first input selector as deterministic failure context without
    /// requiring another allocation after the reservation failure.
    DuplicateIndexAllocationFailed {
        selector: TableSelector,
        requested: usize,
    },
}

impl ExtractionPlanError {
    /// Returns the selector associated with this plan-construction failure.
    ///
    /// A duplicate error returns the first repeated selector in input order. An auxiliary
    /// allocation error returns the first input selector retained as deterministic context.
    pub const fn selector(&self) -> &TableSelector {
        match self {
            Self::DuplicateSelector { selector }
            | Self::DuplicateIndexAllocationFailed { selector, .. } => selector,
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
            Self::DuplicateIndexAllocationFailed {
                selector,
                requested,
            } => write!(
                formatter,
                "failed to reserve duplicate-selector index for {requested} extraction target(s): first schema={:?}, table={:?}",
                selector.schema(),
                selector.name()
            ),
        }
    }
}

impl Error for ExtractionPlanError {}

/// Returns the input index of the first repeated key using one pre-reserved hash lookup per key.
pub(crate) fn first_duplicate_index<I, K>(keys: I) -> Result<Option<usize>, TryReserveError>
where
    I: ExactSizeIterator<Item = K>,
    K: Eq + Hash,
{
    let mut seen = HashSet::new();
    seen.try_reserve(keys.len())?;

    for (index, key) in keys.enumerate() {
        if !seen.insert(key) {
            return Ok(Some(index));
        }
    }

    Ok(None)
}

/// One archive-specific table-data target resolved for plan execution.
///
/// The selector remains owned and byte-oriented. The two dump IDs are copied from the
/// metadata preflight so no archive borrow is retained while sequential entry reads mutate
/// the archive's single seekable source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionTarget {
    selector: TableSelector,
    table_entry_id: DumpId,
    data_entry_id: DumpId,
}

impl ExtractionTarget {
    /// Returns the logical table selector that produced this target.
    pub const fn selector(&self) -> &TableSelector {
        &self.selector
    }

    /// Returns the resolved `TABLE` dump ID for this archive.
    pub const fn table_entry_id(&self) -> DumpId {
        self.table_entry_id
    }

    /// Returns the resolved `TABLE DATA` dump ID copied by execution.
    pub const fn data_entry_id(&self) -> DumpId {
        self.data_entry_id
    }
}

/// Successful result for one target in an [`ExtractionPlan`] execution.
///
/// Completion means both bounded copying and destination flushing succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionOutcome {
    target: ExtractionTarget,
    copied_bytes: u64,
}

impl ExtractionOutcome {
    /// Returns the archive-specific target that completed successfully.
    pub const fn target(&self) -> &ExtractionTarget {
        &self.target
    }

    /// Returns the number of decompressed bytes accepted by that target's destination.
    pub const fn copied_bytes(&self) -> u64 {
        self.copied_bytes
    }
}

/// Failure while executing an [`ExtractionPlan`] against one archive.
///
/// A preflight failure occurs before any destination is requested and therefore has no
/// failed target or completed outcomes. A target failure retains the outcomes that fully
/// completed earlier in plan order and identifies the target that failed. Bytes already
/// accepted by the failing target cannot be rolled back; the embedded [`PgDumpError`]
/// preserves the existing raw-entry dump-ID, limit, decompression, input, or output context.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExtractionExecutionError {
    /// Metadata preflight rejected the logical plan before target execution began.
    Preflight { source: PgDumpError },
    /// One target failed after zero or more earlier targets completed.
    Target {
        target: ExtractionTarget,
        completed: Vec<ExtractionOutcome>,
        source: PgDumpError,
    },
}

impl ExtractionExecutionError {
    /// Returns targets that fully completed before this failure, in plan order.
    pub fn completed(&self) -> &[ExtractionOutcome] {
        match self {
            Self::Preflight { .. } => &[],
            Self::Target { completed, .. } => completed,
        }
    }

    /// Returns the target that failed after preflight, or `None` for a preflight failure.
    pub const fn failed_target(&self) -> Option<&ExtractionTarget> {
        match self {
            Self::Preflight { .. } => None,
            Self::Target { target, .. } => Some(target),
        }
    }

    /// Returns the detailed existing pgdumpx error for this execution failure.
    pub const fn pgdump_error(&self) -> &PgDumpError {
        match self {
            Self::Preflight { source } | Self::Target { source, .. } => source,
        }
    }
}

impl fmt::Display for ExtractionExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight { source } => {
                write!(formatter, "extraction plan preflight failed: {source}")
            }
            Self::Target {
                target,
                completed,
                source,
            } => write!(
                formatter,
                "extraction target failed after {} completed target(s): schema={:?}, table={:?}: {source}",
                completed.len(),
                target.selector().schema(),
                target.selector().name()
            ),
        }
    }
}

impl Error for ExtractionExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.pgdump_error())
    }
}

/// An owned reusable plan describing ordered table extraction intent.
///
/// A plan stores only logical [`TableSelector`] values and the existing raw-output
/// [`EntryReadLimits`] policy. It never stores dump IDs, file offsets, readers, or other
/// archive-specific seek state, so the same plan can be preflighted or executed against
/// multiple archives. Selector order is preserved exactly as supplied by the caller.
///
/// Duplicate selectors are rejected during construction by an expected-linear, input-order
/// membership pass over exact `(schema, table)` byte keys. [`ExtractionPlan::preflight`]
/// resolves all selectors and their related `TABLE DATA` entries using archive metadata
/// only; it does not seek to or decompress payloads. [`ExtractionPlan::execute`] always
/// completes that full metadata preflight before requesting the first destination, then
/// streams each target sequentially through the existing bounded raw-entry copy path.
/// Destination creation, naming, overwrite, framing, and atomic-output policy remain the
/// caller's responsibility.
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
        let duplicate_index = match first_duplicate_index(
            selectors
                .iter()
                .map(|selector| (selector.schema(), selector.name())),
        ) {
            Ok(duplicate_index) => duplicate_index,
            Err(_) => {
                let requested = selectors.len();
                let selector = selectors
                    .into_iter()
                    .next()
                    .expect("a nonzero reservation failure has an input selector");
                return Err(ExtractionPlanError::DuplicateIndexAllocationFailed {
                    selector,
                    requested,
                });
            }
        };

        if let Some(index) = duplicate_index {
            let selector = selectors
                .into_iter()
                .nth(index)
                .expect("duplicate index originated from the selector iterator");
            return Err(ExtractionPlanError::DuplicateSelector { selector });
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

    /// Executes this plan as deterministic bounded sequential raw table-data streams.
    ///
    /// The complete logical plan is preflighted before `destination_for` is called even
    /// once. Archive-specific dump IDs are then copied into owned [`ExtractionTarget`]
    /// values so the metadata borrow ends before the archive's mutable seekable source is
    /// used. Targets execute strictly in selector order and each one calls
    /// [`Archive::copy_entry_to`] with this plan's [`EntryReadLimits`]. No entry is fully
    /// buffered and no second archive handle is opened.
    ///
    /// `destination_for` supplies output policy only. An error creating a destination is
    /// reported as [`PgDumpError::EntryOutputIo`] with zero written bytes for that target.
    /// After [`Archive::copy_entry_to`] succeeds, the executor flushes the destination before
    /// reporting the target as completed. A flush failure is reported as
    /// [`PgDumpError::EntryOutputIo`] with the number of bytes accepted by the destination.
    /// The executor does not add path naming, overwrite, framing, filesystem durability
    /// (`fsync`/`sync_all`), or rollback policy.
    ///
    /// If one target fails, later destinations are never requested. Earlier successful
    /// targets remain listed in [`ExtractionExecutionError::completed`]. Bytes already
    /// accepted by the failing destination cannot be rolled back and the operation returns
    /// an error rather than reporting partial extraction as success.
    pub fn execute<R, W, F>(
        &self,
        archive: &mut Archive<R>,
        mut destination_for: F,
    ) -> Result<Vec<ExtractionOutcome>, ExtractionExecutionError>
    where
        R: Read + Seek,
        W: Write,
        F: FnMut(&ExtractionTarget) -> io::Result<W>,
    {
        let targets = {
            let resolved = self
                .preflight(archive)
                .map_err(|source| ExtractionExecutionError::Preflight { source })?;
            let mut targets = Vec::with_capacity(resolved.tables().len());

            for (selector, table) in self.selectors.iter().zip(resolved.tables()) {
                let Some(data_entry_id) = table.data_entry_id() else {
                    return Err(ExtractionExecutionError::Preflight {
                        source: PgDumpError::TableDataEntryUnavailable {
                            table_id: table.table_entry_id().as_i32(),
                        },
                    });
                };
                targets.push(ExtractionTarget {
                    selector: selector.clone(),
                    table_entry_id: table.table_entry_id(),
                    data_entry_id,
                });
            }

            targets
        };

        let mut completed = Vec::with_capacity(targets.len());
        for target in targets {
            let mut destination = match destination_for(&target) {
                Ok(destination) => destination,
                Err(source) => {
                    let error = PgDumpError::EntryOutputIo {
                        dump_id: target.data_entry_id().as_i32(),
                        written: 0,
                        source,
                    };
                    return Err(ExtractionExecutionError::Target {
                        target,
                        completed,
                        source: error,
                    });
                }
            };

            let copied_bytes = match archive.copy_entry_to(
                target.data_entry_id(),
                &mut destination,
                self.entry_read_limits,
            ) {
                Ok(copied_bytes) => copied_bytes,
                Err(source) => {
                    return Err(ExtractionExecutionError::Target {
                        target,
                        completed,
                        source,
                    });
                }
            };

            if let Err(source) = destination.flush() {
                let error = PgDumpError::EntryOutputIo {
                    dump_id: target.data_entry_id().as_i32(),
                    written: copied_bytes,
                    source,
                };
                return Err(ExtractionExecutionError::Target {
                    target,
                    completed,
                    source: error,
                });
            }

            completed.push(ExtractionOutcome {
                target,
                copied_bytes,
            });
        }

        Ok(completed)
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
