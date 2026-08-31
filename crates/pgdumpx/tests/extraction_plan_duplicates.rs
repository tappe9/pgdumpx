use pgdumpx::{ExtractionPlan, ExtractionPlanError, TableSelector};

#[test]
fn early_duplicate_reports_the_first_repeated_selector() {
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"public", b"inventory");

    let error = ExtractionPlan::new(vec![orders.clone(), orders.clone(), inventory]).unwrap_err();

    assert!(matches!(
        error,
        ExtractionPlanError::DuplicateSelector { .. }
    ));
    assert_eq!(error.selector(), &orders);
}

#[test]
fn late_duplicate_reports_the_first_repeat_in_input_order() {
    let orders = TableSelector::new(b"public", b"orders");
    let inventory = TableSelector::new(b"public", b"inventory");
    let customers = TableSelector::new(b"public", b"customers");

    let error = ExtractionPlan::new(vec![
        orders.clone(),
        inventory.clone(),
        customers,
        inventory.clone(),
        orders,
    ])
    .unwrap_err();

    assert_eq!(error.selector(), &inventory);
}

#[test]
fn identical_table_names_in_distinct_schemas_remain_distinct_and_ordered() {
    let public_orders = TableSelector::new(b"public", b"orders");
    let archive_orders = TableSelector::new(b"archive", b"orders");

    let plan = ExtractionPlan::new(vec![public_orders.clone(), archive_orders.clone()]).unwrap();

    assert_eq!(plan.selectors(), &[public_orders, archive_orders]);
}

#[test]
fn large_unique_plan_preserves_every_selector_in_input_order() {
    const SELECTOR_COUNT: usize = 10_000;

    let selectors = (0..SELECTOR_COUNT)
        .map(|index| TableSelector::new(b"public", format!("table_{index:05}").into_bytes()))
        .collect::<Vec<_>>();
    let first = selectors.first().unwrap().clone();
    let middle = selectors[SELECTOR_COUNT / 2].clone();
    let last = selectors.last().unwrap().clone();

    let plan = ExtractionPlan::new(selectors).unwrap();

    assert_eq!(plan.selectors().len(), SELECTOR_COUNT);
    assert_eq!(plan.selectors().first(), Some(&first));
    assert_eq!(plan.selectors().get(SELECTOR_COUNT / 2), Some(&middle));
    assert_eq!(plan.selectors().last(), Some(&last));
}
