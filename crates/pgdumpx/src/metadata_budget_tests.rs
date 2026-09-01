use crate::{DumpId, Limits, PgDumpError, metadata_budget::MetadataBudget};

#[test]
fn aggregate_budget_counters_reject_arithmetic_overflow_without_wrapping() {
    let limits = Limits::default();

    let mut strings = MetadataBudget::with_usage_for_test(limits, u64::MAX, 0, 0);
    assert!(matches!(
        strings.charge_string_bytes(1, 41),
        Err(PgDumpError::ArithmeticOverflow { offset: 41 })
    ));

    let mut dependencies = MetadataBudget::with_usage_for_test(limits, 0, u64::MAX, 0);
    assert!(matches!(
        dependencies.charge_dependency(DumpId::from_valid(7), 42),
        Err(PgDumpError::ArithmeticOverflow { offset: 42 })
    ));

    let mut indexes = MetadataBudget::with_usage_for_test(limits, 0, 0, u64::MAX);
    assert!(matches!(
        indexes.charge_index_bytes(1, "test index"),
        Err(PgDumpError::ArithmeticOverflow { offset: 0 })
    ));
}
