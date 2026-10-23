use serde_json::{json, Value};

use crate::boundary::json_projection_values_equal_stack_safe;
use crate::schema::Schema;
use crate::tables::commands_tests::engine_with;
use crate::yrs_engine::{
    Affinity, EditorOffsetKind, HistoryPolicy, RevisionedPosition, SelectionInput, SelectionIntent,
    TransactionOrigin, TypedCommand, TypedTransaction, YrsDocumentEngine,
};

const REQUEST_ID: u64 = 1;
const ROOT_CONTEXT: &str = "root";
const FIRST_CELL_OPENING: u32 = 2;
const CELL_TEXT_OFFSET: u32 = 2;
const PARAGRAPH_TEXT_OFFSET: u32 = 1;
const BOUNDARY_BACKSPACE_ALLOWANCE: usize = 2;
const SELECTED_DELETE_PRESSES: usize = 1;
const PARTIAL_SELECTION_MARGIN: usize = 1;
const BOLD_MARK: &str = "bold";
const ITALIC_MARK: &str = "italic";

fn scope_text_start(context: &str) -> u32 {
    if context == ROOT_CONTEXT {
        PARAGRAPH_TEXT_OFFSET
    } else {
        FIRST_CELL_OPENING + CELL_TEXT_OFFSET
    }
}

fn scope_text_end(engine: &YrsDocumentEngine, context: &str) -> u32 {
    let document = engine.document().unwrap();
    if context == ROOT_CONTEXT {
        document.content_size() - PARAGRAPH_TEXT_OFFSET
    } else {
        FIRST_CELL_OPENING + document.node_at(&[0, 0, 0]).unwrap().node_size() - CELL_TEXT_OFFSET
    }
}

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../../scripts/tests/editor-command-sequences.json"
    ))
    .expect("the shared command sequence corpus parses")
}

fn descendants(node: &Value) -> Vec<&Value> {
    let mut nodes = vec![node];
    if let Some(children) = node["content"].as_array() {
        nodes.extend(children.iter().flat_map(descendants));
    }
    nodes
}

fn scope<'a>(document: &'a Value, context: &str) -> &'a Value {
    if context == ROOT_CONTEXT {
        document
    } else {
        &document["content"][0]["content"][0]["content"][0]
    }
}

fn fixture(corpus: &Value, context: &str, inline: bool) -> YrsDocumentEngine {
    let paragraph =
        |text| json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] });
    let paragraphs = if inline {
        vec![paragraph(format!(
            "{}{}",
            corpus["prefix"].as_str().unwrap(),
            corpus["suffix"].as_str().unwrap()
        ))]
    } else {
        vec![
            paragraph(corpus["prefix"].as_str().unwrap().to_owned()),
            paragraph(corpus["suffix"].as_str().unwrap().to_owned()),
        ]
    };
    let blocks = if context == ROOT_CONTEXT {
        paragraphs
    } else {
        vec![
            json!({ "type": "table", "content": [{ "type": "table_row", "content": [
            { "type": context, "content": paragraphs },
            { "type": "table_cell", "content": [{ "type": "paragraph", "content": [
                { "type": "text", "text": corpus["neighbour"] }
            ] }] }
        ] }] }),
        ]
    };
    engine_with(
        Schema::from_json(&corpus["config"]["schema"]).expect("the corpus schema parses"),
        blocks,
    )
}

fn select(engine: &mut YrsDocumentEngine, anchor: u32, head: u32, label: &str) {
    let point = |position| RevisionedPosition {
        offset: engine
            .position_map()
            .unwrap()
            .doc_to_scalar(position, engine.document().unwrap()),
        kind: EditorOffsetKind::Scalar,
        affinity: Affinity::Before,
    };
    engine
        .apply_typed_transaction(TypedTransaction {
            request_id: REQUEST_ID,
            base_document_revision: engine.revision(),
            origin: TransactionOrigin::LocalApi,
            operations: Vec::new(),
            selection_intent: SelectionIntent::Set(SelectionInput::Text {
                anchor: point(anchor),
                head: point(head),
            }),
            history_policy: HistoryPolicy::Skip,
        })
        .unwrap_or_else(|error| panic!("{label}: select {anchor}..{head}: {error:?}"));
}

fn command(engine: &mut YrsDocumentEngine, command: TypedCommand, label: &str) {
    let before = engine.document_json().unwrap();
    let selection = engine.resolved_selection().cloned();
    engine
        .apply_command(REQUEST_ID, command.clone())
        .unwrap_or_else(|error| {
            panic!("{label}: {command:?}: {error:?}; before={before}; selection={selection:?}")
        });
}

fn check(engine: &YrsDocumentEngine, corpus: &Value, context: &str, expected: &str, label: &str) {
    let document = engine.document_json().unwrap();
    let actual = descendants(scope(&document, context))
        .iter()
        .filter_map(|node| node["text"].as_str())
        .collect::<String>();
    assert_eq!(
        actual,
        expected,
        "{label}: document={document} selection={:?}",
        engine.resolved_selection()
    );
    if context != ROOT_CONTEXT {
        let cells = document["content"][0]["content"][0]["content"]
            .as_array()
            .unwrap();
        assert_eq!(cells.len(), 2, "{label}: cell count changed");
        assert_eq!(
            document["content"][0]["content"].as_array().unwrap().len(),
            1,
            "{label}: row count changed"
        );
        assert_eq!(
            cells[0]["type"], context,
            "{label}: target cell type changed"
        );
        assert_eq!(
            cells[1],
            json!({ "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": corpus["neighbour"] }] }] }),
            "{label}: neighbour changed"
        );
    }
    assert!(
        engine.resolved_selection().is_some(),
        "{label}: selection disappeared"
    );
}

fn probe(
    engine: &mut YrsDocumentEngine,
    corpus: &Value,
    context: &str,
    expected: &str,
    label: &str,
) {
    let before = engine.document_json().unwrap();
    let selection = engine.resolved_selection().cloned();
    let text = corpus["probe"].as_str().unwrap();
    command(
        engine,
        TypedCommand::InsertText { text: text.into() },
        label,
    );
    check(engine, corpus, context, expected, label);
    for _ in text.chars() {
        command(engine, TypedCommand::DeleteBackward, label);
    }
    assert_eq!(
        engine.document_json().unwrap(),
        before,
        "{label}: typing/backspacing did not restore the document"
    );
    assert_eq!(
        engine.resolved_selection().cloned(),
        selection,
        "{label}: typing/backspacing did not restore the caret"
    );
}

fn clear(
    engine: &mut YrsDocumentEngine,
    context: &str,
    mode: &str,
    name: &str,
    preserves_formatting: bool,
    label: &str,
) {
    let start = scope_text_start(context);
    let end = scope_text_end(engine, context);
    select(
        engine,
        if mode == "selection" { start } else { end },
        end,
        label,
    );
    let budget = if mode == "selection" {
        SELECTED_DELETE_PRESSES
    } else {
        descendants(scope(&engine.document_json().unwrap(), context)).len()
            * BOUNDARY_BACKSPACE_ALLOWANCE
            + end as usize
    };
    for _ in 0..budget {
        // Native backspace selects protected images before removing them.
        if engine
            .schema()
            .node(name)
            .is_some_and(|spec| spec.deletable_on_backspace == Some(false))
        {
            let scope_node = if context == ROOT_CONTEXT {
                engine.document().unwrap().root()
            } else {
                engine.document().unwrap().node_at(&[0, 0, 0]).unwrap()
            };
            let mut position = if context == ROOT_CONTEXT {
                0
            } else {
                FIRST_CELL_OPENING + PARAGRAPH_TEXT_OFFSET
            };
            let mut protected_range = None;
            for child in scope_node.content().unwrap().iter() {
                if child.node_type() == name {
                    protected_range = Some(position..=position + child.node_size());
                    break;
                }
                position += child.node_size();
            }
            if matches!(engine.resolved_selection(), Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head })
                if anchor.document == head.document && protected_range.as_ref().is_some_and(|range| range.contains(&head.document)))
            {
                let offset = engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(position, engine.document().unwrap());
                engine
                    .apply_typed_transaction(TypedTransaction {
                        request_id: REQUEST_ID,
                        base_document_revision: engine.revision(),
                        origin: TransactionOrigin::LocalApi,
                        operations: Vec::new(),
                        selection_intent: SelectionIntent::Set(SelectionInput::Node {
                            at: (RevisionedPosition {
                                offset,
                                kind: EditorOffsetKind::Scalar,
                                affinity: Affinity::Before,
                            })
                            .into(),
                        }),
                        history_policy: HistoryPolicy::Skip,
                    })
                    .unwrap_or_else(|error| panic!("{label}: select protected node: {error:?}"));
            }
        }
        command(engine, TypedCommand::DeleteBackward, label);
        let document = engine.document_json().unwrap();
        if !descendants(scope(&document, context))
            .iter()
            .any(|node| node["type"] == "text" || node["type"] == name)
        {
            return;
        }
        if mode == "selection" && preserves_formatting {
            if !descendants(scope(&document, context))
                .iter()
                .any(|node| node["type"] == "text")
            {
                return;
            }
        }
    }
    panic!(
        "{label}: backspace exhausted {budget} presses: {}",
        engine.document_json().unwrap()
    );
}

#[test]
fn anchored_multiline_replacement_of_marked_hard_break_content_accepts_each_line() {
    const LEFT: &str = "left";
    const RIGHT: &str = "right";
    const REPLACEMENT: &str = "one\ntwo\nthree";
    let corpus = corpus();
    for context in corpus["contexts"].as_array().unwrap() {
        let context = context.as_str().unwrap();
        let paragraphs = vec![json!({"type": "paragraph", "content": [
            {"type": "text", "text": LEFT, "marks": [{"type": BOLD_MARK}]},
            {"type": "hardBreak"}, {"type": "text", "text": RIGHT}
        ]})];
        let blocks = if context == ROOT_CONTEXT {
            paragraphs
        } else {
            vec![
                json!({"type": "table", "content": [{"type": "table_row", "content": [
                    {"type": context, "content": paragraphs},
                    {"type": "table_cell", "content": [{"type": "paragraph", "content": [
                        {"type": "text", "text": corpus["neighbour"]}
                    ]}]}
                ]}]}),
            ]
        };
        let label = format!("{context}/anchored marked multiline replacement");
        let mut engine = engine_with(
            Schema::from_json(&corpus["config"]["schema"]).unwrap(),
            blocks,
        );
        let old_caret = scope_text_start(context) + u32::try_from(LEFT.chars().count()).unwrap();
        select(&mut engine, old_caret, old_caret, &label);
        let before = engine.document_json().unwrap();
        let selection = SelectionInput::Text {
            anchor: RevisionedPosition {
                offset: engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(scope_text_start(context), engine.document().unwrap()),
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::Before,
            },
            head: RevisionedPosition {
                offset: engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(scope_text_end(&engine, context), engine.document().unwrap()),
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::Before,
            },
        };
        engine
            .apply_command_at_selection_with_outbox(
                REQUEST_ID,
                TypedCommand::ReplaceSelectionText {
                    text: REPLACEMENT.into(),
                },
                selection,
                TransactionOrigin::LocalInput,
                None,
            )
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        let after = engine.document_json().unwrap();
        assert_eq!(
            scope(&after, context)["content"],
            json!([
                {"type": "paragraph", "content": [{"type": "text", "text": "one", "marks": [{"type": BOLD_MARK}]}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "two", "marks": [{"type": BOLD_MARK}]}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "three", "marks": [{"type": BOLD_MARK}]}]}
            ]),
            "{label}: a line or mark differs"
        );
        let after_selection = engine.resolved_selection().cloned();
        let final_caret = scope_text_end(&engine, context);
        assert!(
            matches!(engine.resolved_selection(), Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head })
            if anchor.document == final_caret && head.document == final_caret),
            "{label}: wrong final caret {after_selection:?}"
        );
        assert!(
            engine.undo(REQUEST_ID).unwrap().is_some(),
            "{label}: undo unavailable"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            before,
            "{label}: undo changed original inline structure"
        );
        assert!(
            matches!(engine.resolved_selection(), Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head })
            if anchor.document == scope_text_start(context) && head.document == scope_text_end(&engine, context)),
            "{label}: undo failed to restore unsynced native selection"
        );
        assert!(
            engine.redo(REQUEST_ID).unwrap().is_some(),
            "{label}: redo unavailable"
        );
        assert_eq!(
            engine.document_json().unwrap(),
            after,
            "{label}: redo changed replacement lines"
        );
        assert_eq!(
            engine.resolved_selection().cloned(),
            after_selection,
            "{label}: redo lost final caret"
        );
        command(
            &mut engine,
            TypedCommand::InsertText {
                text: corpus["probe"].as_str().unwrap().into(),
            },
            &label,
        );
        check(
            &engine,
            &corpus,
            context,
            &format!("onetwothree{}", corpus["probe"].as_str().unwrap()),
            &label,
        );
    }
}

#[test]
fn partial_cross_paragraph_selection_before_a_hard_break_remains_editable() {
    const PREFIX: &str = "abc";
    const FIRST_SUFFIX: &str = "def";
    const LAST_TEXT: &str = "ghi";
    const REPLACEMENT: &str = "X";
    const KEPT_PREFIX: &str = "a";
    const KEPT_SUFFIX: &str = "i";
    const SELECTION_MARGIN: u32 = 1;
    let corpus = corpus();
    for context in corpus["contexts"].as_array().unwrap() {
        let context = context.as_str().unwrap();
        for replace in [false, true] {
            let paragraphs = vec![
                json!({"type": "paragraph", "content": [
                    {"type": "text", "text": PREFIX}, {"type": "hardBreak"},
                    {"type": "text", "text": FIRST_SUFFIX}
                ]}),
                json!({"type": "paragraph", "content": [{"type": "text", "text": LAST_TEXT}]}),
            ];
            let blocks = if context == ROOT_CONTEXT {
                paragraphs
            } else {
                vec![
                    json!({"type": "table", "content": [{"type": "table_row", "content": [
                        {"type": context, "content": paragraphs},
                        {"type": "table_cell", "content": [{"type": "paragraph", "content": [
                            {"type": "text", "text": corpus["neighbour"]}
                        ]}]}
                    ]}]}),
                ]
            };
            let label = format!("{context}/cross-paragraph-before-hard-break/replace={replace}");
            let mut engine = engine_with(
                Schema::from_json(&corpus["config"]["schema"]).unwrap(),
                blocks,
            );
            let start = scope_text_start(context) + SELECTION_MARGIN;
            let end = scope_text_end(&engine, context) - SELECTION_MARGIN;
            select(&mut engine, start, end, &label);
            let before = engine.document_json().unwrap();
            let before_selection = engine.resolved_selection().cloned();
            command(
                &mut engine,
                if replace {
                    TypedCommand::ReplaceSelectionText {
                        text: REPLACEMENT.into(),
                    }
                } else {
                    TypedCommand::DeleteBackward
                },
                &label,
            );
            let inserted = if replace { REPLACEMENT } else { "" };
            let expected = format!("{KEPT_PREFIX}{inserted}{KEPT_SUFFIX}");
            check(&engine, &corpus, context, &expected, &label);
            let after = engine.document_json().unwrap();
            assert_eq!(
                scope(&after, context)["content"],
                json!([
                    {"type": "paragraph", "content": [{"type": "text", "text": expected}]}
                ]),
                "{label}: selected hard break or paragraph survived"
            );
            let after_selection = engine.resolved_selection().cloned();
            let caret = start + u32::try_from(inserted.chars().count()).unwrap();
            assert!(
                matches!(engine.resolved_selection(), Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head })
                if anchor.document == caret && head.document == caret),
                "{label}: wrong surviving caret {after_selection:?}"
            );
            assert!(
                engine.undo(REQUEST_ID).unwrap().is_some(),
                "{label}: undo unavailable"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                before,
                "{label}: undo changed rich content"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                before_selection,
                "{label}: undo changed selected endpoints"
            );
            assert!(
                engine.redo(REQUEST_ID).unwrap().is_some(),
                "{label}: redo unavailable"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                after,
                "{label}: redo changed rich content"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                after_selection,
                "{label}: redo changed the caret"
            );
            probe(
                &mut engine,
                &corpus,
                context,
                &format!(
                    "{KEPT_PREFIX}{inserted}{}{KEPT_SUFFIX}",
                    corpus["probe"].as_str().unwrap()
                ),
                &label,
            );
        }
    }
}

#[test]
fn every_ordered_rich_node_pair_can_be_inserted_edited_removed_and_replaced() {
    let corpus = corpus();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for mode in corpus["deletionModes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
        {
            for first in corpus["fragments"].as_array().unwrap() {
                for second in corpus["fragments"].as_array().unwrap() {
                    let label = format!("{context}/{mode}/{} -> {}", first["name"], second["name"]);
                    let prefix = corpus["prefix"].as_str().unwrap();
                    let suffix = corpus["suffix"].as_str().unwrap();
                    let mut engine = fixture(&corpus, context, first["inline"] == true);
                    let start = scope_text_start(context);
                    select(
                        &mut engine,
                        start + prefix.len() as u32,
                        start + prefix.len() as u32,
                        &label,
                    );
                    for (index, fragment) in [first, second].iter().enumerate() {
                        let label = format!("{label}/phase {index}");
                        command(
                            &mut engine,
                            TypedCommand::InsertContentJson {
                                json: json!({ "type": "doc", "content": [fragment["node"]] }),
                            },
                            &label,
                        );
                        let middle = fragment["text"].as_str().unwrap();
                        let (left, right) = if index == 0 {
                            (prefix, suffix)
                        } else {
                            ("", "")
                        };
                        check(
                            &engine,
                            &corpus,
                            context,
                            &format!("{left}{middle}{right}"),
                            &label,
                        );
                        assert!(
                            descendants(scope(&engine.document_json().unwrap(), context))
                                .iter()
                                .any(|node| node["type"] == fragment["name"]),
                            "{label}: inserted node missing"
                        );
                        probe(
                            &mut engine,
                            &corpus,
                            context,
                            &format!("{left}{middle}{}{right}", corpus["probe"].as_str().unwrap()),
                            &label,
                        );
                        clear(
                            &mut engine,
                            context,
                            mode,
                            fragment["name"].as_str().unwrap(),
                            fragment["preservesFormatting"] == true,
                            &label,
                        );
                        check(&engine, &corpus, context, "", &label);
                    }
                    command(
                        &mut engine,
                        TypedCommand::InsertText {
                            text: corpus["recovery"].as_str().unwrap().into(),
                        },
                        &label,
                    );
                    check(
                        &engine,
                        &corpus,
                        context,
                        corpus["recovery"].as_str().unwrap(),
                        &label,
                    );
                }
            }
        }
    }
}

#[test]
fn a_selection_ending_inside_a_table_clears_each_side_without_joining_the_cell() {
    let corpus = corpus();
    let mut engine = engine_with(
        Schema::from_json(&corpus["config"]["schema"]).unwrap(),
        vec![
            json!({ "type": "paragraph", "content": [{ "type": "text", "text": "before" }] }),
            json!({ "type": "table", "content": [{ "type": "table_row", "content": [{
            "type": "table_cell", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "cell" }] }]
        }] }] }),
        ],
    );
    let mut expected = engine.document_json().unwrap();
    let opening = crate::tables::commands_tests::cell_openings(&engine)[0];
    let end = opening + CELL_TEXT_OFFSET + "cell".len() as u32;
    select(
        &mut engine,
        PARAGRAPH_TEXT_OFFSET,
        end,
        "partial table selection",
    );
    engine
        .apply_command(REQUEST_ID, TypedCommand::DeleteBackward)
        .expect("a selection reaching into a cell deletes the covered content on each side");
    expected["content"][0]
        .as_object_mut()
        .unwrap()
        .remove("content");
    expected["content"][1]["content"][0]["content"][0]["content"][0]
        .as_object_mut()
        .unwrap()
        .remove("content");
    assert_eq!(
        engine.document_json().unwrap(),
        expected,
        "the paragraph and the cell keep their own blocks and the table survives"
    );
}

#[test]
fn deleting_all_text_keeps_a_custom_root_content_rule() {
    let corpus = corpus();
    let mut schema = corpus["config"]["schema"].clone();
    schema["nodes"][0]["content"] = json!("codeBlock+");
    let mut engine = engine_with(
        Schema::from_json(&schema).unwrap(),
        vec![
            json!({ "type": "codeBlock", "content": [{ "type": "text", "text": "one" }] }),
            json!({ "type": "codeBlock", "content": [{ "type": "text", "text": "two" }] }),
        ],
    );
    let end = engine.document().unwrap().content_size() - PARAGRAPH_TEXT_OFFSET;
    select(
        &mut engine,
        PARAGRAPH_TEXT_OFFSET,
        end,
        "custom root selection",
    );
    command(
        &mut engine,
        TypedCommand::DeleteBackward,
        "custom root selection",
    );
    assert_eq!(
        engine.document_json().unwrap(),
        json!({ "type": "doc", "content": [{ "type": "codeBlock" }] })
    );
    command(
        &mut engine,
        TypedCommand::InsertText {
            text: "again".into(),
        },
        "custom root recovery",
    );
    assert_eq!(
        engine.document_json().unwrap()["content"][0]["content"][0]["text"],
        "again"
    );
}

#[test]
fn unicode_scalar_deletion_sequences_preserve_every_caret_and_neighbour() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let suffix = corpus["suffix"].as_str().unwrap();
    let original = format!("{prefix}{suffix}");
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for offset in [0, prefix.len(), original.len()] {
            for sample in corpus["unicode"].as_array().unwrap() {
                let label = format!("{context}/unicode {}/offset {offset}", sample["name"]);
                let mut engine = fixture(&corpus, context, true);
                let start = scope_text_start(context);
                select(
                    &mut engine,
                    start + offset as u32,
                    start + offset as u32,
                    &label,
                );
                let (left, right) = original.split_at(offset);
                command(
                    &mut engine,
                    TypedCommand::InsertText {
                        text: sample["text"].as_str().unwrap().into(),
                    },
                    &label,
                );
                check(
                    &engine,
                    &corpus,
                    context,
                    &format!("{left}{}{right}", sample["text"].as_str().unwrap()),
                    &label,
                );
                for remaining in sample["codePointBackspaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                {
                    command(&mut engine, TypedCommand::DeleteBackward, &label);
                    check(
                        &engine,
                        &corpus,
                        context,
                        &format!("{left}{remaining}{right}"),
                        &label,
                    );
                    let Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head }) =
                        engine.resolved_selection()
                    else {
                        panic!("{label}: text caret lost")
                    };
                    let expected = start + offset as u32 + remaining.chars().count() as u32;
                    assert_eq!(
                        (anchor.document, head.document),
                        (expected, expected),
                        "{label}: caret moved to the wrong Unicode boundary"
                    );
                }
                let recovery = corpus["recovery"].as_str().unwrap();
                command(
                    &mut engine,
                    TypedCommand::InsertText {
                        text: recovery.into(),
                    },
                    &label,
                );
                check(
                    &engine,
                    &corpus,
                    context,
                    &format!("{left}{recovery}{right}"),
                    &label,
                );
            }
        }
    }
}

#[test]
fn partial_replacement_history_and_split_join_sequences_keep_input_live() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let suffix = corpus["suffix"].as_str().unwrap();
    let original = format!("{prefix}{suffix}");
    let from = prefix.len() - PARTIAL_SELECTION_MARGIN;
    let to = prefix.len() + PARTIAL_SELECTION_MARGIN;
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for sample in corpus["unicode"].as_array().unwrap() {
            let replacement = sample["text"].as_str().unwrap();
            let expected = format!("{}{replacement}{}", &original[..from], &original[to..]);
            for reverse in [false, true] {
                for split_offset in [0, prefix.chars().count(), expected.chars().count()] {
                    let label = format!(
                        "{context}/replace {}/reverse {reverse}/split {split_offset}",
                        sample["name"]
                    );
                    let mut engine = fixture(&corpus, context, true);
                    let start = scope_text_start(context);
                    let original_document = engine.document_json().unwrap();
                    let (anchor, head) = if reverse { (to, from) } else { (from, to) };
                    select(
                        &mut engine,
                        start + anchor as u32,
                        start + head as u32,
                        &label,
                    );
                    command(
                        &mut engine,
                        TypedCommand::ReplaceSelectionText {
                            text: replacement.into(),
                        },
                        &label,
                    );
                    check(&engine, &corpus, context, &expected, &label);
                    let replaced_document = engine.document_json().unwrap();
                    let replaced_selection = engine.resolved_selection().cloned();
                    assert!(
                        engine.undo(REQUEST_ID).unwrap().is_some(),
                        "{label}: replacement cannot undo"
                    );
                    assert_eq!(
                        engine.document_json().unwrap(),
                        original_document,
                        "{label}: undo replacement changed neighbours or formatting"
                    );
                    check(&engine, &corpus, context, &original, &label);
                    assert!(
                        engine.redo(REQUEST_ID).unwrap().is_some(),
                        "{label}: replacement cannot redo"
                    );
                    assert_eq!(
                        engine.document_json().unwrap(),
                        replaced_document,
                        "{label}: redo replacement changed neighbours or formatting"
                    );
                    assert_eq!(
                        engine.resolved_selection().cloned(),
                        replaced_selection,
                        "{label}: redo replacement lost its caret"
                    );
                    select(
                        &mut engine,
                        start + split_offset as u32,
                        start + split_offset as u32,
                        &label,
                    );
                    let joined_selection = engine.resolved_selection().cloned();
                    command(&mut engine, TypedCommand::SplitBlock, &label);
                    check(&engine, &corpus, context, &expected, &label);
                    let split_document = engine.document_json().unwrap();
                    let blocks = scope(&split_document, context)["content"]
                        .as_array()
                        .unwrap();
                    let characters: Vec<_> = expected.chars().collect();
                    let left: String = characters[..split_offset].iter().collect();
                    let right: String = characters[split_offset..].iter().collect();
                    let block_text = |node| {
                        descendants(node)
                            .iter()
                            .filter_map(|child| child["text"].as_str())
                            .collect::<String>()
                    };
                    assert_eq!(
                        blocks.len(),
                        2,
                        "{label}: Return did not split exactly one paragraph"
                    );
                    assert_eq!(
                        block_text(&blocks[0]),
                        left,
                        "{label}: wrong text before Return"
                    );
                    assert_eq!(
                        block_text(&blocks[1]),
                        right,
                        "{label}: wrong text after Return"
                    );
                    command(&mut engine, TypedCommand::DeleteBackward, &label);
                    assert_eq!(
                        engine.document_json().unwrap(),
                        replaced_document,
                        "{label}: immediate Backspace did not restore the paragraph"
                    );
                    assert_eq!(
                        engine.resolved_selection().cloned(),
                        joined_selection,
                        "{label}: joining lost the insertion position"
                    );
                    probe(
                        &mut engine,
                        &corpus,
                        context,
                        &format!("{left}{}{right}", corpus["probe"].as_str().unwrap()),
                        &label,
                    );
                }
            }
        }
    }
}

#[test]
fn rich_node_removal_history_restores_content_and_allows_immediate_input() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let suffix = corpus["suffix"].as_str().unwrap();
    let recovery = corpus["recovery"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for fragment in corpus["fragments"].as_array().unwrap() {
            for redo in [false, true] {
                let label = format!("{context}/history {}/redo {redo}", fragment["name"]);
                eprintln!("{label}");
                let mut engine = fixture(&corpus, context, fragment["inline"] == true);
                let start = scope_text_start(context) + prefix.chars().count() as u32;
                select(&mut engine, start, start, &label);
                command(
                    &mut engine,
                    TypedCommand::InsertContentJson {
                        json: json!({ "type": "doc", "content": [fragment["node"]] }),
                    },
                    &label,
                );
                let inserted = engine.document_json().unwrap();
                let selected_range = (scope_text_start(context), scope_text_end(&engine, context));
                clear(
                    &mut engine,
                    context,
                    "selection",
                    fragment["name"].as_str().unwrap(),
                    fragment["preservesFormatting"] == true,
                    &label,
                );
                check(&engine, &corpus, context, "", &label);
                let cleared = engine.document_json().unwrap();
                let cleared_selection = engine.resolved_selection().cloned();
                assert!(
                    engine.undo(REQUEST_ID).unwrap().is_some(),
                    "{label}: deletion cannot undo"
                );
                let restored = engine.document_json().unwrap();
                assert!(json_projection_values_equal_stack_safe(&restored, &inserted),
                    "{label}: undo deletion lost rich content; expected={inserted} actual={restored}");
                check(
                    &engine,
                    &corpus,
                    context,
                    &format!("{prefix}{}{suffix}", fragment["text"].as_str().unwrap()),
                    &label,
                );
                let Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head }) =
                    engine.resolved_selection()
                else {
                    panic!("{label}: undo lost text selection")
                };
                assert_eq!(
                    (anchor.document, head.document),
                    selected_range,
                    "{label}: undo did not restore the selected range"
                );
                if redo {
                    assert!(
                        engine.redo(REQUEST_ID).unwrap().is_some(),
                        "{label}: deletion cannot redo"
                    );
                    assert_eq!(
                        engine.document_json().unwrap(),
                        cleared,
                        "{label}: redo deletion lost its empty scope"
                    );
                    assert_eq!(
                        engine.resolved_selection().cloned(),
                        cleared_selection,
                        "{label}: redo deletion lost its caret"
                    );
                }
                command(
                    &mut engine,
                    TypedCommand::ReplaceSelectionText {
                        text: recovery.into(),
                    },
                    &label,
                );
                check(&engine, &corpus, context, recovery, &label);
                let recovered = engine.document_json().unwrap();
                probe(
                    &mut engine,
                    &corpus,
                    context,
                    &format!("{recovery}{}", corpus["probe"].as_str().unwrap()),
                    &label,
                );
                assert_eq!(
                    engine.document_json().unwrap(),
                    recovered,
                    "{label}: recovered content stopped accepting input"
                );
            }
        }
    }
}

fn check_mark_runs(
    engine: &YrsDocumentEngine,
    context: &str,
    expected: &[(&str, &[&str])],
    label: &str,
) {
    let document = engine.document_json().unwrap();
    let actual: Vec<_> = descendants(scope(&document, context))
        .iter()
        .filter_map(|node| {
            let text = node["text"].as_str()?;
            let mut marks: Vec<_> = node["marks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|mark| mark["type"].as_str().unwrap())
                .collect();
            marks.sort_unstable();
            Some(
                text.chars()
                    .map(move |character| (character, marks.clone())),
            )
        })
        .flatten()
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .flat_map(|(text, marks)| {
            let mut marks = marks.to_vec();
            marks.sort_unstable();
            text.chars()
                .map(move |character| (character, marks.clone()))
        })
        .collect();
    assert_eq!(
        actual, expected,
        "{label}: text inherited the wrong marks; document={document}"
    );
}

#[test]
fn stored_mark_history_sequences_keep_each_run_and_continued_typing_exact() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let suffix = corpus["suffix"].as_str().unwrap();
    let original = format!("{prefix}{suffix}");
    let probe_text = corpus["probe"].as_str().unwrap();
    let recovery = corpus["recovery"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for offset in [0, prefix.len(), original.len()] {
            let label = format!("{context}/stored marks/offset {offset}");
            let mut engine = fixture(&corpus, context, true);
            let cursor = scope_text_start(context) + offset as u32;
            select(&mut engine, cursor, cursor, &label);
            let (left, right) = original.split_at(offset);
            let mut text = left.to_owned();
            for mark in [BOLD_MARK, ITALIC_MARK, BOLD_MARK, ITALIC_MARK] {
                command(
                    &mut engine,
                    TypedCommand::ToggleMark {
                        mark_type: mark.into(),
                    },
                    &label,
                );
                command(
                    &mut engine,
                    TypedCommand::InsertText {
                        text: probe_text.into(),
                    },
                    &label,
                );
                text.push_str(probe_text);
                check(&engine, &corpus, context, &format!("{text}{right}"), &label);
            }
            let plain_tail = format!("{probe_text}{right}");
            check_mark_runs(
                &engine,
                context,
                &[
                    (left, &[]),
                    (probe_text, &[BOLD_MARK]),
                    (probe_text, &[BOLD_MARK, ITALIC_MARK]),
                    (probe_text, &[ITALIC_MARK]),
                    (&plain_tail, &[]),
                ],
                &label,
            );
            let complete = engine.document_json().unwrap();
            let complete_selection = engine.resolved_selection().cloned();
            assert!(
                engine.undo(REQUEST_ID).unwrap().is_some(),
                "{label}: plain typing cannot undo"
            );
            check_mark_runs(
                &engine,
                context,
                &[
                    (left, &[]),
                    (probe_text, &[BOLD_MARK]),
                    (probe_text, &[BOLD_MARK, ITALIC_MARK]),
                    (probe_text, &[ITALIC_MARK]),
                    (right, &[]),
                ],
                &label,
            );
            assert!(
                engine.redo(REQUEST_ID).unwrap().is_some(),
                "{label}: plain typing cannot redo"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                complete,
                "{label}: redo damaged mark runs"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                complete_selection,
                "{label}: redo misplaced caret"
            );
            command(
                &mut engine,
                TypedCommand::InsertText {
                    text: recovery.into(),
                },
                &label,
            );
            let continued_tail = format!("{probe_text}{recovery}{right}");
            check_mark_runs(
                &engine,
                context,
                &[
                    (left, &[]),
                    (probe_text, &[BOLD_MARK]),
                    (probe_text, &[BOLD_MARK, ITALIC_MARK]),
                    (probe_text, &[ITALIC_MARK]),
                    (&continued_tail, &[]),
                ],
                &label,
            );
            check(
                &engine,
                &corpus,
                context,
                &format!("{text}{recovery}{right}"),
                &label,
            );
        }
    }
}

#[test]
fn anchored_mark_toggles_apply_to_the_native_caret_before_selection_sync() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let original = format!("{prefix}{}", corpus["suffix"].as_str().unwrap());
    let text = corpus["probe"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for offset in [0, prefix.len(), original.len()] {
            for previous_mark in [false, true] {
                let label = format!(
                    "{context}/anchored marks/offset {offset}/previous mark {previous_mark}"
                );
                let mut engine = fixture(&corpus, context, true);
                let original_document = engine.document_json().unwrap();
                if previous_mark {
                    let previous_offset = if offset == 0 { original.len() } else { 0 };
                    let previous_cursor = scope_text_start(context) + previous_offset as u32;
                    select(&mut engine, previous_cursor, previous_cursor, &label);
                    command(
                        &mut engine,
                        TypedCommand::ToggleMark {
                            mark_type: BOLD_MARK.into(),
                        },
                        &label,
                    );
                }
                let point = RevisionedPosition {
                    offset: engine.position_map().unwrap().doc_to_scalar(
                        scope_text_start(context) + offset as u32,
                        engine.document().unwrap(),
                    ),
                    kind: EditorOffsetKind::Scalar,
                    affinity: Affinity::Before,
                };
                let selection = SelectionInput::Text {
                    anchor: point,
                    head: point,
                };
                for command in [
                    TypedCommand::ToggleMark {
                        mark_type: BOLD_MARK.into(),
                    },
                    TypedCommand::ToggleMark {
                        mark_type: ITALIC_MARK.into(),
                    },
                    TypedCommand::InsertText { text: text.into() },
                ] {
                    assert!(
                        engine
                            .apply_command_at_selection_with_outbox(
                                REQUEST_ID,
                                command.clone(),
                                selection.clone(),
                                TransactionOrigin::LocalInput,
                                None
                            )
                            .unwrap_or_else(|error| panic!("{label}: {command:?}: {error:?}"))
                            .is_some(),
                        "{label}: command was rejected"
                    );
                }
                let (left, right) = original.split_at(offset);
                check(
                    &engine,
                    &corpus,
                    context,
                    &format!("{left}{text}{right}"),
                    &label,
                );
                check_mark_runs(
                    &engine,
                    context,
                    &[(left, &[]), (text, &[BOLD_MARK, ITALIC_MARK]), (right, &[])],
                    &label,
                );
                let typed_document = engine.document_json().unwrap();
                assert!(engine
                    .undo(REQUEST_ID)
                    .unwrap_or_else(|error| panic!("{label}: undo native input: {error:?}"))
                    .is_some());
                assert_eq!(
                    engine.document_json().unwrap(),
                    original_document,
                    "{label}: undo native input changed the document"
                );
                assert!(engine
                    .redo(REQUEST_ID)
                    .unwrap_or_else(|error| panic!("{label}: redo native input: {error:?}"))
                    .is_some());
                assert_eq!(
                    engine.document_json().unwrap(),
                    typed_document,
                    "{label}: redo native input changed its mark run"
                );
            }
        }
    }
}

#[test]
fn anchored_rich_range_deletion_history_restores_the_submitted_native_selection() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let recovery = corpus["recovery"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for fragment in corpus["fragments"].as_array().unwrap() {
            let label = format!("{context}/anchored rich deletion/{}", fragment["name"]);
            let mut engine = fixture(&corpus, context, fragment["inline"] == true);
            let cursor = scope_text_start(context) + prefix.chars().count() as u32;
            select(&mut engine, cursor, cursor, &label);
            command(
                &mut engine,
                TypedCommand::InsertContentJson {
                    json: json!({"type": "doc", "content": [fragment["node"]]}),
                },
                &label,
            );
            let inserted = engine.document_json().unwrap();
            let previous_selection = engine.resolved_selection().cloned();
            let selected_range = (scope_text_start(context), scope_text_end(&engine, context));
            let point = |document_position| RevisionedPosition {
                offset: engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(document_position, engine.document().unwrap()),
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::Before,
            };
            let anchor = point(selected_range.0);
            let head = point(selected_range.1);
            assert!(engine
                .apply_command_at_selection_with_outbox(
                    REQUEST_ID,
                    TypedCommand::DeleteRange {
                        range: crate::yrs_engine::RevisionedRange {
                            from: anchor.clone(),
                            to: head.clone(),
                        },
                    },
                    SelectionInput::Text { anchor, head },
                    TransactionOrigin::LocalInput,
                    None,
                )
                .unwrap_or_else(|error| panic!("{label}: delete native range: {error:?}"))
                .is_some());
            check(&engine, &corpus, context, "", &label);
            let cleared = engine.document_json().unwrap();
            let cleared_selection = engine.resolved_selection().cloned();
            assert!(
                engine.undo(REQUEST_ID).unwrap().is_some(),
                "{label}: undo refused"
            );
            assert!(
                json_projection_values_equal_stack_safe(
                    &engine.document_json().unwrap(),
                    &inserted
                ),
                "{label}: undo changed rich nodes or neighbours"
            );
            let Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head }) =
                engine.resolved_selection()
            else {
                panic!("{label}: undo lost the native text selection")
            };
            assert_eq!(
                (anchor.document, head.document),
                selected_range,
                "{label}: undo restored the old engine caret instead of the submitted native range; previous={previous_selection:?}"
            );
            assert!(
                engine.redo(REQUEST_ID).unwrap().is_some(),
                "{label}: redo refused"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                cleared,
                "{label}: redo changed the empty scope"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                cleared_selection,
                "{label}: redo changed its caret"
            );
            command(
                &mut engine,
                TypedCommand::InsertText {
                    text: recovery.into(),
                },
                &label,
            );
            check(&engine, &corpus, context, recovery, &label);
        }
    }
}

#[test]
fn empty_input_and_boundary_backspace_keep_cleared_scopes_editable() {
    let corpus = corpus();
    let unicode = corpus["unicode"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["name"] == "zwj")
        .expect("the corpus includes a joined emoji")["text"]
        .as_str()
        .unwrap();
    let recovery = corpus["recovery"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        let label = format!("{context}/empty input and boundary backspace");
        let mut engine = fixture(&corpus, context, true);
        clear(&mut engine, context, "selection", "paragraph", true, &label);
        check(&engine, &corpus, context, "", &label);
        let empty_document = engine.document_json().unwrap();
        let empty_selection = engine.resolved_selection().cloned();
        let empty_revision = engine.revision();
        for operation in [
            TypedCommand::InsertText {
                text: String::new(),
            },
            TypedCommand::DeleteBackward,
        ] {
            command(&mut engine, operation, &label);
            assert_eq!(
                engine.document_json().unwrap(),
                empty_document,
                "{label}: no-op input changed the empty scope or neighbour"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                empty_selection,
                "{label}: no-op input moved the beginning caret"
            );
            assert_eq!(
                engine.revision(),
                empty_revision,
                "{label}: no-op input advanced the document revision"
            );
        }
        command(
            &mut engine,
            TypedCommand::InsertText {
                text: unicode.into(),
            },
            &label,
        );
        check(&engine, &corpus, context, unicode, &label);
        let joined_document = engine.document_json().unwrap();
        let joined_selection = engine.resolved_selection().cloned();
        command(&mut engine, TypedCommand::SplitBlock, &label);
        command(&mut engine, TypedCommand::DeleteBackward, &label);
        assert_eq!(
            engine.document_json().unwrap(),
            joined_document,
            "{label}: immediate split/backspace did not restore the joined emoji"
        );
        assert_eq!(
            engine.resolved_selection().cloned(),
            joined_selection,
            "{label}: immediate split/backspace did not restore its caret"
        );
        probe(
            &mut engine,
            &corpus,
            context,
            &format!("{unicode}{}", corpus["probe"].as_str().unwrap()),
            &label,
        );
        clear(&mut engine, context, "selection", "paragraph", true, &label);
        command(
            &mut engine,
            TypedCommand::InsertText {
                text: recovery.into(),
            },
            &label,
        );
        check(&engine, &corpus, context, recovery, &label);
    }
}

#[test]
fn anchored_typing_history_does_not_restore_marks_from_the_previous_native_caret() {
    let corpus = corpus();
    let prefix = corpus["prefix"].as_str().unwrap();
    let original = format!("{prefix}{}", corpus["suffix"].as_str().unwrap());
    let typed = corpus["probe"].as_str().unwrap();
    let recovery = corpus["recovery"].as_str().unwrap();
    for context in corpus["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
    {
        for offset in [0, prefix.len(), original.len()] {
            let label = format!("{context}/anchored typing history/offset {offset}");
            let mut engine = fixture(&corpus, context, true);
            let previous_offset = if offset == 0 { original.len() } else { 0 };
            let previous_cursor = scope_text_start(context) + previous_offset as u32;
            select(&mut engine, previous_cursor, previous_cursor, &label);
            let previous_marks = [BOLD_MARK, ITALIC_MARK];
            for mark_type in previous_marks {
                command(
                    &mut engine,
                    TypedCommand::ToggleMark {
                        mark_type: mark_type.into(),
                    },
                    &label,
                );
            }
            assert_eq!(
                engine.stored_marks().unwrap().len(),
                previous_marks.len(),
                "{label}: old caret marks missing"
            );
            let original_document = engine.document_json().unwrap();
            let requested_cursor = scope_text_start(context) + offset as u32;
            let point = RevisionedPosition {
                offset: engine
                    .position_map()
                    .unwrap()
                    .doc_to_scalar(requested_cursor, engine.document().unwrap()),
                kind: EditorOffsetKind::Scalar,
                affinity: Affinity::Before,
            };
            engine
                .apply_command_at_selection_with_outbox(
                    REQUEST_ID,
                    TypedCommand::InsertText { text: typed.into() },
                    SelectionInput::Text {
                        anchor: point,
                        head: point,
                    },
                    TransactionOrigin::LocalInput,
                    None,
                )
                .unwrap_or_else(|error| panic!("{label}: native typing failed: {error:?}"));
            let (left, right) = original.split_at(offset);
            check_mark_runs(
                &engine,
                context,
                &[(left, &[]), (typed, &[]), (right, &[])],
                &label,
            );
            let typed_document = engine.document_json().unwrap();
            let typed_selection = engine.resolved_selection().cloned();
            assert!(
                engine.undo(REQUEST_ID).unwrap().is_some(),
                "{label}: undo refused"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                original_document,
                "{label}: undo changed the original document"
            );
            let Some(crate::yrs_engine::ResolvedSelection::Text { anchor, head }) =
                engine.resolved_selection()
            else {
                panic!("{label}: undo lost the requested native caret")
            };
            assert_eq!(
                (anchor.document, head.document),
                (requested_cursor, requested_cursor),
                "{label}: undo restored the previous caret"
            );
            assert!(
                engine.stored_marks().is_none(),
                "{label}: undo restored marks from the previous caret"
            );
            assert!(
                engine.redo(REQUEST_ID).unwrap().is_some(),
                "{label}: redo refused"
            );
            assert_eq!(
                engine.document_json().unwrap(),
                typed_document,
                "{label}: redo changed plain native input"
            );
            assert_eq!(
                engine.resolved_selection().cloned(),
                typed_selection,
                "{label}: redo moved the native caret"
            );
            command(
                &mut engine,
                TypedCommand::InsertText {
                    text: recovery.into(),
                },
                &label,
            );
            check(
                &engine,
                &corpus,
                context,
                &format!("{left}{typed}{recovery}{right}"),
                &label,
            );
            check_mark_runs(
                &engine,
                context,
                &[
                    (left, &[]),
                    (&format!("{typed}{recovery}"), &[]),
                    (right, &[]),
                ],
                &label,
            );
        }
    }
}
