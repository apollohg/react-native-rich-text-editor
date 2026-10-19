use proptest::prelude::*;
use serde_json::{json, Value};

const MAX_TOP_LEVEL_BLOCKS: usize = 5;

pub(crate) fn text_strategy() -> impl Strategy<Value = Value> {
    (
        prop::sample::select(vec!["a", "bc", "dé", "🦀x", "wxyz"]),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(text, bold, italic)| {
            let marks = [(bold, "bold"), (italic, "italic")]
                .into_iter()
                .filter(|(enabled, _)| *enabled)
                .map(|(_, mark)| json!({ "type": mark }))
                .collect::<Vec<_>>();
            if marks.is_empty() {
                json!({ "type": "text", "text": text })
            } else {
                json!({ "type": "text", "text": text, "marks": marks })
            }
        })
}

pub(crate) fn hard_break_json() -> Value {
    json!({ "type": "hardBreak" })
}

fn inline_content_strategy(inline: BoxedStrategy<Value>) -> impl Strategy<Value = Vec<Value>> {
    prop::collection::vec(inline, 0..4)
}

fn paragraph_strategy(inline: BoxedStrategy<Value>) -> impl Strategy<Value = Value> {
    inline_content_strategy(inline)
        .prop_map(|content| json!({ "type": "paragraph", "content": content }))
}

fn textblock_strategy(inline: BoxedStrategy<Value>) -> impl Strategy<Value = Value> {
    (
        prop::sample::select(vec!["paragraph", "paragraph", "h1", "h2"]),
        inline_content_strategy(inline),
    )
        .prop_map(|(kind, content)| json!({ "type": kind, "content": content }))
}

fn flow_block_strategy(inline: BoxedStrategy<Value>) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        6 => textblock_strategy(inline.clone()),
        1 => Just(json!({ "type": "horizontalRule" })),
        1 => Just(json!({ "type": "image", "attrs": { "src": "split.png" } })),
    ];
    leaf.prop_recursive(2, 16, 3, move |inner| {
        prop_oneof![
            (
                prop::sample::select(vec!["bulletList", "orderedList"]),
                prop::collection::vec(
                    (
                        paragraph_strategy(inline.clone()),
                        prop::collection::vec(inner.clone(), 0..2)
                    ),
                    1..3,
                ),
            )
                .prop_map(|(list, items)| {
                    let items = items
                        .into_iter()
                        .map(|(first, rest)| {
                            let mut content = vec![first];
                            content.extend(rest);
                            json!({ "type": "listItem", "content": content })
                        })
                        .collect::<Vec<_>>();
                    json!({ "type": list, "content": items })
                }),
            prop::collection::vec(inner, 1..3)
                .prop_map(|content| json!({ "type": "blockquote", "content": content })),
        ]
    })
}

fn table_strategy(inline: BoxedStrategy<Value>) -> impl Strategy<Value = Value> {
    (1usize..3, 1usize..3).prop_flat_map(move |(rows, columns)| {
        prop::collection::vec(
            prop::collection::vec(
                prop::collection::vec(flow_block_strategy(inline.clone()), 1..3),
                columns,
            ),
            rows,
        )
        .prop_map(|rows| {
            let rows = rows
                .into_iter()
                .map(|cells| {
                    let cells = cells
                        .into_iter()
                        .map(|content| json!({ "type": "tableCell", "content": content }))
                        .collect::<Vec<_>>();
                    json!({ "type": "tableRow", "content": cells })
                })
                .collect::<Vec<_>>();
            json!({ "type": "table", "content": rows })
        })
    })
}

pub(crate) fn document_strategy() -> impl Strategy<Value = Value> {
    document_strategy_with_inline(
        prop_oneof![4 => text_strategy(), 1 => Just(hard_break_json())].boxed(),
    )
}

pub(crate) fn document_strategy_with_inline(
    inline: BoxedStrategy<Value>,
) -> impl Strategy<Value = Value> {
    prop::collection::vec(
        prop_oneof![4 => flow_block_strategy(inline.clone()), 1 => table_strategy(inline)],
        1..MAX_TOP_LEVEL_BLOCKS,
    )
    .prop_map(|content| json!({ "type": "doc", "content": content }))
}
