use super::*;
use serde_json::Value;

const REGRESSION_REQUEST: u64 = 72_100;

fn imported_table_engine(document: &Value) -> YrsDocumentEngine {
    let mut engine = YrsDocumentEngine::new(YrsEngineConfig {
        schema: crate::schema::presets::tiptap_table_schema(),
        ..transaction_engine_config(crate::yrs_engine::EditingLimits::default())
    })
    .unwrap();
    engine
        .import_json(&document.to_string(), TransactionOrigin::DocumentImport)
        .unwrap();
    hydrate_import_for_compile_test(&mut engine);
    engine
}

fn scalar_offset_of(engine: &YrsDocumentEngine, needle: &str) -> u32 {
    let rendered = &engine.derived_state.as_ref().unwrap().rendered_text;
    let byte = rendered
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not rendered in {rendered:?}"));
    u32::try_from(rendered[..byte].chars().count()).unwrap()
}

fn apply_at(engine: &mut YrsDocumentEngine, offset: u32, command: TypedCommand) {
    select_text(engine, REGRESSION_REQUEST, offset, offset);
    let before = engine.document_json().unwrap();
    engine
        .apply_command(REGRESSION_REQUEST + 1, command.clone())
        .unwrap_or_else(|error| panic!("{command:?} at {offset} in {before} failed: {error:?}"));
}

#[test]
fn return_at_the_start_of_a_later_paragraph_in_a_list_item_splits_the_item() {
    let mut engine = imported_table_engine(&json!({
        "type": "doc",
        "content": [{ "type": "bulletList", "content": [{ "type": "listItem", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "one" }] },
            { "type": "paragraph", "content": [{ "type": "text", "text": "tail" }] },
            { "type": "paragraph", "content": [{ "type": "text", "text": "rest" }] }
        ] }] }]
    }));
    let offset = scalar_offset_of(&engine, "tail");
    apply_at(&mut engine, offset, TypedCommand::SplitBlock);
    assert_eq!(
        engine.document_json().unwrap(),
        json!({
            "type": "doc",
            "content": [{ "type": "bulletList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "one" }] },
                    { "type": "paragraph" }
                ] },
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": "tail" }] },
                    { "type": "paragraph", "content": [{ "type": "text", "text": "rest" }] }
                ] }
            ] }]
        })
    );
}

#[test]
fn backspace_at_a_rule_joins_blockquotes_inside_a_table_cell() {
    const RULE_AND_SEPARATOR_SCALARS: u32 = 2;
    let cell = |content: Value| {
        json!({
            "type": "doc",
            "content": [{ "type": "table", "content": [{ "type": "tableRow", "content": [{
                "type": "tableCell",
                "content": content
            }] }] }]
        })
    };
    let mut engine = imported_table_engine(&cell(json!([
        { "type": "blockquote", "content": [{ "type": "horizontalRule" }] },
        { "type": "blockquote", "content": [
            { "type": "horizontalRule" },
            { "type": "paragraph", "content": [{ "type": "text", "text": "kw" }] }
        ] }
    ])));
    let second_rule = scalar_offset_of(&engine, "kw") - RULE_AND_SEPARATOR_SCALARS;
    apply_at(&mut engine, second_rule, TypedCommand::DeleteBackward);
    assert_eq!(
        engine.document_json().unwrap(),
        cell(json!([{ "type": "blockquote", "content": [
            { "type": "horizontalRule" },
            { "type": "horizontalRule" },
            { "type": "paragraph", "content": [{ "type": "text", "text": "kw" }] }
        ] }]))
    );
}

#[test]
fn nested_textblock_edits_keep_the_installed_history_charge_exact() {
    const NESTED_EDITS: usize = 12;
    let mut engine = imported_table_engine(&json!({
        "type": "doc",
        "content": [{ "type": "table", "content": [{ "type": "tableRow", "content": [{
            "type": "tableCell",
            "content": [{ "type": "blockquote", "content": [{ "type": "orderedList", "content": [
                { "type": "listItem", "content": [
                    { "type": "paragraph", "content": [
                        { "type": "text", "text": "bold", "marks": [{ "type": "bold" }] },
                        { "type": "text", "text": "plain" }
                    ] },
                    { "type": "paragraph", "content": [{ "type": "text", "text": "second" }] }
                ] }
            ] }] }]
        }] }] }]
    }));
    for edit in 0..NESTED_EDITS {
        let offset = scalar_offset_of(&engine, "plain");
        let command = if edit % 3 == 2 {
            TypedCommand::DeleteBackward
        } else {
            TypedCommand::InsertText { text: "n".into() }
        };
        apply_at(&mut engine, offset, command);
        let installed = &engine.derived_state.as_ref().unwrap().canonical_artifact;
        let fresh = crate::yrs_engine::canonical::CanonicalSchemaContext::new(&engine.schema)
            .derive(installed.source_document())
            .unwrap();
        assert_eq!(
            installed.history_snapshot_retained_charge(),
            fresh.history_snapshot_retained_charge(),
            "edit {edit}: the installed artifact's history charge must equal a fresh derivation \
             of its document {}",
            engine.document_json().unwrap()
        );
    }
}

#[test]
fn return_before_a_paragraph_holding_only_an_inline_atom_moves_the_atom() {
    let mut engine = imported_table_engine(&json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "bc" }] },
            { "type": "paragraph", "content": [{ "type": "hardBreak" }] }
        ]
    }));
    let offset = scalar_offset_of(&engine, "bc") + u32::try_from("bc\n".chars().count()).unwrap();
    apply_at(&mut engine, offset, TypedCommand::SplitBlock);
    assert_eq!(
        engine.document_json().unwrap(),
        json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "bc" }] },
                { "type": "paragraph" },
                { "type": "paragraph", "content": [{ "type": "hardBreak" }] }
            ]
        })
    );
}

#[test]
fn return_before_a_trailing_hard_break_moves_it_into_the_new_block() {
    let mut engine = imported_table_engine(&json!({
        "type": "doc",
        "content": [
            { "type": "paragraph", "content": [
                { "type": "text", "text": "ab" },
                { "type": "hardBreak" }
            ] },
            { "type": "bulletList", "content": [{ "type": "listItem", "content": [
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "cd" },
                    { "type": "hardBreak" }
                ] }
            ] }] }
        ]
    }));
    let inside_text = scalar_offset_of(&engine, "ab") + 1;
    apply_at(&mut engine, inside_text, TypedCommand::SplitBlock);
    let before_list_break = scalar_offset_of(&engine, "cd") + 2;
    apply_at(&mut engine, before_list_break, TypedCommand::SplitBlock);
    assert_eq!(
        engine.document_json().unwrap(),
        json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "a" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "b" },
                    { "type": "hardBreak" }
                ] },
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "cd" }] }
                    ] },
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "hardBreak" }] }
                    ] }
                ] }
            ]
        })
    );
}
