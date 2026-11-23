use crate::model::Document;
use crate::schema::Schema;

pub fn to_text(document: &Document, schema: &Schema, block_separator: &str) -> String {
    let mut text = String::new();
    visit_text_segments(document, schema, block_separator, |segment| {
        text.push_str(segment)
    });
    text
}

pub fn text_byte_len(document: &Document, schema: &Schema, block_separator: &str) -> usize {
    let mut length = 0usize;
    visit_text_segments(document, schema, block_separator, |segment| {
        length = length.saturating_add(segment.len())
    });
    length
}

fn visit_text_segments(
    document: &Document,
    schema: &Schema,
    block_separator: &str,
    mut emit: impl FnMut(&str),
) {
    let mut is_first_text_block = true;
    let mut pending = vec![document.root()];
    while let Some(node) = pending.pop() {
        if let Some(text) = node.text_str() {
            emit(text);
            continue;
        }
        if schema.is_text_block(node.node_type())
            && !std::mem::replace(&mut is_first_text_block, false)
        {
            emit(block_separator);
        }
        if let Some(content) = node.content() {
            pending.extend(content.iter().rev());
        }
    }
}
