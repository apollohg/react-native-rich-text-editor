use crate::model::{Mark, Node};
use crate::schema::{NodeRole, Schema};

use super::{longest_run, node_tag, MarkdownFlavor};
use crate::serialize::html_out::{
    escape_inline_html_text, node_to_inline_html, serialize_mark_close, serialize_mark_open,
    HtmlOutput,
};

const STRONG_DELIMITER: &str = "**";
const EMPHASIS_DELIMITER: &str = "*";
const STRIKE_DELIMITER: &str = "~~";
const HARD_BREAK: &str = "\\\n";
const TABLE_HARD_BREAK: &str = "<br>";
const LINK_HREF_ATTR: &str = "href";
const TITLE_ATTR: &str = "title";
const IMAGE_SRC_ATTR: &str = "src";
const IMAGE_ALT_ATTR: &str = "alt";
const IMAGE_DIMENSION_ATTRS: [&str; 2] = ["width", "height"];

#[derive(Default)]
struct InlineBuffer<'a> {
    mark: Option<&'a Mark>,
    output: String,
    first: Option<char>,
    last: Option<char>,
}

impl InlineBuffer<'_> {
    fn append(&mut self, output: &str, first: Option<char>, last: Option<char>) {
        self.output.push_str(output);
        self.first = self.first.or(first);
        self.last = last.or(self.last);
    }
}

fn mark_tag<'a>(mark: &Mark, schema: &'a Schema) -> Option<&'a str> {
    schema
        .mark(mark.mark_type())
        .and_then(|spec| spec.html_tag.as_deref())
}

fn mark_order(mark: &Mark, schema: &Schema) -> usize {
    match mark_tag(mark, schema) {
        Some("a") => 0,
        Some("strong" | "b") => 2,
        Some("em" | "i") => 3,
        Some("s" | "del" | "strike") => 4,
        Some("code") => 5,
        _ => 1,
    }
}

pub(super) fn serialize(
    node: &Node,
    schema: &Schema,
    flavor: MarkdownFlavor,
    table_cell: bool,
) -> String {
    if has_crossing_marks(node, schema) {
        return node
            .content()
            .into_iter()
            .flat_map(|content| content.iter())
            .map(|child| html_in_cell(node_to_inline_html(child, schema), table_cell))
            .collect();
    }
    let mut buffers = vec![InlineBuffer::default()];
    if let Some(content) = node.content() {
        for (index, child) in content.iter().enumerate() {
            let mut marks = child.marks().iter().collect::<Vec<_>>();
            marks.sort_by_key(|mark| mark_order(mark, schema));
            let shared = marks
                .iter()
                .zip(buffers.iter().skip(1))
                .take_while(|(mark, buffer)| buffer.mark == Some(**mark))
                .count();
            while buffers.len() > shared + 1 {
                close_mark(&mut buffers, schema, flavor, table_cell);
            }
            for mark in marks.iter().skip(shared) {
                buffers.push(InlineBuffer {
                    mark: Some(mark),
                    ..Default::default()
                });
            }
            let raw = child.text_str().unwrap_or("");
            let output = if child.is_text() {
                if marks
                    .iter()
                    .any(|mark| mark_tag(mark, schema) == Some("code"))
                {
                    raw.to_owned()
                } else {
                    escape_text(raw, table_cell)
                }
            } else if schema
                .node(child.node_type())
                .is_some_and(|spec| matches!(spec.role, NodeRole::HardBreak))
            {
                if table_cell || index + 1 == node.child_count() {
                    TABLE_HARD_BREAK.into()
                } else {
                    HARD_BREAK.into()
                }
            } else if node_tag(child, schema) == Some("img") {
                image(child, schema, table_cell)
            } else {
                html_in_cell(node_to_inline_html(child, schema), table_cell)
            };
            buffers.last_mut().expect("root inline buffer").append(
                &output,
                raw.chars().next(),
                raw.chars().last(),
            );
        }
    }
    while buffers.len() > 1 {
        close_mark(&mut buffers, schema, flavor, table_cell);
    }
    let mut output = buffers.pop().expect("root inline buffer").output;
    let spaces = output.len() - output.trim_end_matches(' ').len();
    output.truncate(output.len() - spaces);
    output.push_str(&"&#32;".repeat(spaces));
    output
}

fn has_crossing_marks(node: &Node, schema: &Schema) -> bool {
    let mut previous: Vec<&Mark> = Vec::new();
    for child in node
        .content()
        .into_iter()
        .flat_map(|content| content.iter())
    {
        let mut current = child.marks().iter().collect::<Vec<_>>();
        current.sort_by_key(|mark| mark_order(mark, schema));
        let shared = previous
            .iter()
            .zip(&current)
            .take_while(|(left, right)| left == right)
            .count();
        if previous
            .iter()
            .skip(shared)
            .any(|mark| current.iter().skip(shared).any(|next| next == mark))
        {
            return true;
        }
        previous = current;
    }
    false
}

fn destination_is_literal(value: &str) -> bool {
    value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '!' | '#'
                    | '$'
                    | '%'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | '-'
                    | '.'
                    | '/'
                    | ':'
                    | ';'
                    | '='
                    | '?'
                    | '@'
                    | '_'
                    | '~'
            )
    })
}

fn close_mark(
    buffers: &mut Vec<InlineBuffer<'_>>,
    schema: &Schema,
    flavor: MarkdownFlavor,
    table_cell: bool,
) {
    let buffer = buffers.pop().expect("open mark");
    let mark = buffer.mark.expect("nonroot inline buffer");
    let output = wrap_mark(&buffer, mark, schema, flavor, table_cell);
    buffers
        .last_mut()
        .expect("parent inline buffer")
        .append(&output, buffer.first, buffer.last);
}

fn wrap_mark(
    buffer: &InlineBuffer<'_>,
    mark: &Mark,
    schema: &Schema,
    flavor: MarkdownFlavor,
    table_cell: bool,
) -> String {
    if buffer.output.is_empty() {
        return String::new();
    }
    let tag = mark_tag(mark, schema);
    if tag == Some("code") {
        let text = &buffer.output;
        if !mark.attrs().is_empty()
            || text.contains(['\n', '\r'])
            || text.chars().all(|ch| ch == ' ')
        {
            let mut escaped = String::new();
            escape_inline_html_text(text, &mut escaped);
            return html_mark(mark, schema, &escaped, table_cell);
        }
        let fence = "`".repeat(longest_run(text, '`') + 1);
        let padding = if text.starts_with(['`', ' ']) || text.ends_with(['`', ' ']) {
            " "
        } else {
            ""
        };
        let text = if table_cell {
            text.replace('|', "\\|")
        } else {
            text.clone()
        };
        return format!("{fence}{padding}{text}{padding}{fence}");
    }
    if tag == Some("a") {
        if let Some(href) = mark
            .attrs()
            .get(LINK_HREF_ATTR)
            .and_then(serde_json::Value::as_str)
            .filter(|href| destination_is_literal(href))
        {
            if mark
                .attrs()
                .keys()
                .all(|name| matches!(name.as_str(), LINK_HREF_ATTR | TITLE_ATTR))
            {
                return format!(
                    "[{}](<{}>{})",
                    buffer.output,
                    escape_destination(href, table_cell),
                    title(
                        mark.attrs()
                            .get(TITLE_ATTR)
                            .and_then(serde_json::Value::as_str),
                        table_cell
                    )
                );
            }
        }
        return html_mark(mark, schema, &buffer.output, table_cell);
    }
    let delimiter = match tag {
        Some("strong" | "b") => Some(STRONG_DELIMITER),
        Some("em" | "i") => Some(EMPHASIS_DELIMITER),
        Some("s" | "del" | "strike") if flavor == MarkdownFlavor::Gfm => Some(STRIKE_DELIMITER),
        _ => None,
    };
    if let Some(delimiter) = delimiter.filter(|_| mark.attrs().is_empty()) {
        // Whitespace and punctuation at mark edges can invalidate delimiter flanking.
        if [buffer.first, buffer.last]
            .into_iter()
            .flatten()
            .all(|ch| ch.is_alphanumeric())
        {
            return format!("{delimiter}{}{delimiter}", buffer.output);
        }
    }
    html_mark(mark, schema, &buffer.output, table_cell)
}

fn html_mark(mark: &Mark, schema: &Schema, content: &str, table_cell: bool) -> String {
    let mut output = String::new();
    serialize_mark_open(mark, schema, &mut output, HtmlOutput::MarkdownInline);
    output.push_str(content);
    serialize_mark_close(mark, schema, &mut output);
    html_in_cell(output, table_cell)
}

fn html_in_cell(html: String, table_cell: bool) -> String {
    if table_cell {
        html.replace('|', "&#124;")
            .replace('\n', "&#10;")
            .replace('\r', "&#13;")
    } else {
        html
    }
}

fn escape_text(text: &str, table_cell: bool) -> String {
    let mut output = String::new();
    let mut line_start = true;
    for ch in text.chars() {
        match ch {
            '\n' => {
                output.push_str("&#10;");
                line_start = true;
                continue;
            }
            '\r' => output.push_str("&#13;"),
            '\t' => output.push_str("&#9;"),
            ' ' if line_start => {
                output.push_str("&#32;");
                continue;
            }
            ':' => output.push_str("&#58;"),
            '@' => output.push_str("&#64;"),
            '.' => output.push_str("&#46;"),
            '|' if table_cell => output.push_str("\\|"),
            ch if ch.is_ascii_punctuation() => {
                output.push('\\');
                output.push(ch);
            }
            _ => output.push(ch),
        }
        line_start = false;
    }
    output
}

fn escape_destination(value: &str, table_cell: bool) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '|' if table_cell => output.push_str("&#124;"),
            '<' | '>' | '\\' => {
                output.push('\\');
                output.push(ch);
            }
            _ => output.push(ch),
        }
    }
    output
}

fn title(value: Option<&str>, table_cell: bool) -> String {
    value
        .map(|value| {
            format!(
                " \"{}\"",
                escape_destination(value, table_cell)
                    .replace('"', "\\\"")
                    .replace('\n', "&#10;")
                    .replace('\r', "&#13;")
            )
        })
        .unwrap_or_default()
}

pub(super) fn image(node: &Node, schema: &Schema, table_cell: bool) -> String {
    if IMAGE_DIMENSION_ATTRS.iter().any(|attr| {
        node.attrs()
            .get(*attr)
            .is_some_and(|value| !value.is_null())
    }) || node.attrs().keys().any(|name| {
        !matches!(
            name.as_str(),
            IMAGE_SRC_ATTR | IMAGE_ALT_ATTR | TITLE_ATTR | "width" | "height"
        )
    }) {
        return html_in_cell(node_to_inline_html(node, schema), table_cell);
    }
    let Some(src) = node
        .attrs()
        .get(IMAGE_SRC_ATTR)
        .and_then(serde_json::Value::as_str)
        .filter(|src| destination_is_literal(src))
    else {
        return html_in_cell(node_to_inline_html(node, schema), table_cell);
    };
    let alt = node
        .attrs()
        .get(IMAGE_ALT_ATTR)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    format!(
        "![{}](<{}>{})",
        escape_text(alt, table_cell),
        escape_destination(src, table_cell),
        title(
            node.attrs()
                .get(TITLE_ATTR)
                .and_then(serde_json::Value::as_str),
            table_cell
        )
    )
}
