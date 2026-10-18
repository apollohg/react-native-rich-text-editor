use super::*;
use crate::yrs_engine::observability::{
    reset_full_pass_counts_for_test, take_full_pass_counts_for_test,
};

const CERTIFIED_VALIDATION_REQUEST: u64 = 72_300;
const PARAGRAPHS: usize = 6;

#[test]
fn command_planning_certifies_root_window_previews_instead_of_validating_the_document() {
    let mut engine = transaction_engine();
    let paragraphs = (0..PARAGRAPHS)
        .map(|index| json!({ "type": "paragraph", "content": [{ "type": "text", "text": format!("p{index}") }] }))
        .collect::<Vec<_>>();
    engine
        .import_json(
            &json!({ "type": "doc", "content": paragraphs }).to_string(),
            TransactionOrigin::DocumentImport,
        )
        .unwrap();
    hydrate_import_for_compile_test(&mut engine);
    select_text(&mut engine, CERTIFIED_VALIDATION_REQUEST, 1, 1);
    reset_full_pass_counts_for_test();
    crate::yrs_engine::derived_state::take_root_window_certifications_for_test();
    let plan = engine.plan_command(
        CERTIFIED_VALIDATION_REQUEST + 1,
        TypedCommand::InsertText { text: "x".into() },
    );
    let counts = take_full_pass_counts_for_test();
    let certifications =
        crate::yrs_engine::derived_state::take_root_window_certifications_for_test();
    assert!(plan.is_ok(), "planning failed: {plan:?}");
    assert_eq!(
        (counts.document_validations, certifications > 0),
        (0, true),
        "planning must certify the changed root window instead of validating the document: \
         {certifications} certifications, {counts:?}"
    );
}
