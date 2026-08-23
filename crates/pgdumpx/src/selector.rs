use crate::{Archive, TableRef};

/// An owned exact-byte selector for one PostgreSQL table identity.
///
/// The selector stores `(schema, table)` as owned bytes and does not borrow an
/// [`Archive`]. Construction performs no UTF-8 conversion, case folding, SQL identifier
/// parsing, or search-path lookup. Equality compares both byte strings exactly.
///
/// A selector can therefore be cloned, stored, and reused against multiple archives;
/// resolution always goes through the target archive's existing table index.
///
/// # Example
///
/// ```
/// use pgdumpx::TableSelector;
///
/// let schema = [0xfe_u8, b's'];
/// let table = [0xff_u8, b't'];
/// let selector = TableSelector::new(&schema, &table);
///
/// assert_eq!(selector.schema(), schema);
/// assert_eq!(selector.name(), table);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSelector {
    schema: Vec<u8>,
    name: Vec<u8>,
}

impl TableSelector {
    /// Creates an owned selector from exact schema and table-name bytes.
    pub fn new(schema: impl AsRef<[u8]>, name: impl AsRef<[u8]>) -> Self {
        Self {
            schema: schema.as_ref().to_vec(),
            name: name.as_ref().to_vec(),
        }
    }

    /// Returns the exact schema bytes stored by this selector.
    pub fn schema(&self) -> &[u8] {
        &self.schema
    }

    /// Returns the exact table-name bytes stored by this selector.
    pub fn name(&self) -> &[u8] {
        &self.name
    }
}

impl<R> Archive<R> {
    /// Resolves an owned selector through the existing metadata-only table index.
    ///
    /// This is equivalent to calling [`Archive::table`] with the selector's exact bytes.
    /// It performs no seek or decompression and preserves the same missing-table and
    /// ambiguity behavior as the low-level byte-oriented lookup.
    pub fn resolve_table(&self, selector: &TableSelector) -> Option<TableRef<'_>> {
        self.table(selector.schema(), selector.name())
    }
}
