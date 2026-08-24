use crate::{Archive, TableSelector, TocEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
enum NamespaceCriterion {
    Any,
    Absent,
    Exact(Vec<u8>),
}

impl Default for NamespaceCriterion {
    fn default() -> Self {
        Self::Any
    }
}

/// An owned reusable filter over already-parsed archive TOC metadata.
///
/// Schema and name criteria compare exact bytes. Object type compares the exact parsed
/// [`TocEntry::description_bytes`] value. No criterion performs UTF-8 conversion, case
/// folding, SQL identifier parsing, search-path lookup, regex/glob matching, payload reads,
/// seeking, or decompression.
///
/// All specified criteria are combined with logical AND. Omitting a criterion leaves that
/// metadata dimension unrestricted. [`MetadataFilter::with_absent_schema`] selects only
/// entries whose namespace is encoded as absent (`None`), while `with_schema(b"")` selects
/// an explicitly present empty namespace; those cases are never conflated.
///
/// [`Archive::filter_metadata`] evaluates this value against TOC entries in archive order,
/// so the returned [`MetadataMatch`] values retain deterministic TOC ordering.
///
/// # Example
///
/// ```no_run
/// use pgdumpx::{Archive, ExtractionPlan, MetadataFilter};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let archive = Archive::open_path("backup.dump")?;
/// let tables = archive.filter_metadata(
///     &MetadataFilter::new()
///         .with_schema(b"public")
///         .with_object_type(b"TABLE"),
/// );
///
/// let selectors = tables
///     .iter()
///     .filter_map(|matched| matched.table_selector())
///     .collect();
/// let plan = ExtractionPlan::new(selectors)?;
/// let resolved = plan.preflight(&archive)?;
/// assert_eq!(resolved.tables().len(), tables.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataFilter {
    namespace: NamespaceCriterion,
    object_type: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
}

impl MetadataFilter {
    /// Creates a filter with no criteria, which matches every parsed TOC entry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requires an explicitly present namespace whose bytes exactly equal `schema`.
    ///
    /// An encoded absent namespace does not match, including when `schema` is empty.
    pub fn with_schema(mut self, schema: impl AsRef<[u8]>) -> Self {
        self.namespace = NamespaceCriterion::Exact(schema.as_ref().to_vec());
        self
    }

    /// Requires the TOC namespace to be absent (`None`).
    ///
    /// This is distinct from [`MetadataFilter::with_schema`] with an empty byte string.
    pub fn with_absent_schema(mut self) -> Self {
        self.namespace = NamespaceCriterion::Absent;
        self
    }

    /// Requires the parsed TOC object type/description to equal `object_type` exactly.
    pub fn with_object_type(mut self, object_type: impl AsRef<[u8]>) -> Self {
        self.object_type = Some(object_type.as_ref().to_vec());
        self
    }

    /// Requires the parsed TOC object name to equal `name` exactly as bytes.
    pub fn with_name(mut self, name: impl AsRef<[u8]>) -> Self {
        self.name = Some(name.as_ref().to_vec());
        self
    }

    /// Returns whether one already-parsed TOC entry satisfies every specified criterion.
    ///
    /// This method inspects metadata accessors only and never opens or reads an entry
    /// payload.
    pub fn matches(&self, entry: &TocEntry) -> bool {
        let namespace_matches = match &self.namespace {
            NamespaceCriterion::Any => true,
            NamespaceCriterion::Absent => entry.namespace_bytes().is_none(),
            NamespaceCriterion::Exact(expected) => {
                entry.namespace_bytes() == Some(expected.as_slice())
            }
        };
        let object_type_matches = match &self.object_type {
            Some(expected) => entry.description_bytes() == expected.as_slice(),
            None => true,
        };
        let name_matches = match &self.name {
            Some(expected) => entry.name_bytes() == expected.as_slice(),
            None => true,
        };

        namespace_matches && object_type_matches && name_matches
    }
}

/// One metadata-only result produced by [`Archive::filter_metadata`].
///
/// The match borrows the parsed [`TocEntry`] owned by the archive. Generic metadata
/// matches remain generic: only a normal `TABLE` entry with an explicitly present
/// namespace can produce an owned [`TableSelector`] for the existing extraction-plan path.
/// A `TABLE DATA` entry is never converted or used to synthesize table identity.
#[derive(Debug, Clone, Copy)]
pub struct MetadataMatch<'a> {
    entry: &'a TocEntry,
}

impl<'a> MetadataMatch<'a> {
    /// Returns the matched parsed TOC entry.
    pub const fn entry(&self) -> &'a TocEntry {
        self.entry
    }

    /// Converts an eligible normal `TABLE` match into the existing owned selector type.
    ///
    /// `None` is returned for every non-`TABLE` object and for a `TABLE` whose namespace
    /// is absent. A present empty namespace remains a concrete byte identity and therefore
    /// can be represented by [`TableSelector`].
    pub fn table_selector(&self) -> Option<TableSelector> {
        if !self.entry.is_table() {
            return None;
        }
        let schema = self.entry.namespace_bytes()?;
        Some(TableSelector::new(schema, self.entry.name_bytes()))
    }
}

impl<R> Archive<R> {
    /// Filters already-parsed TOC metadata without seeking or decompressing payloads.
    ///
    /// Matches are returned in the same order as [`Archive::entries`]. The filter does not
    /// alter archive indexes or cache payload-derived state.
    pub fn filter_metadata<'a>(&'a self, filter: &MetadataFilter) -> Vec<MetadataMatch<'a>> {
        self.entries()
            .iter()
            .filter(|entry| filter.matches(entry))
            .map(|entry| MetadataMatch { entry })
            .collect()
    }
}
