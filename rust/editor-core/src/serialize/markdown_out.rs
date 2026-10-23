use crate::model::{Document, Node};
use crate::schema::{NodeRole, Schema};
use crate::tables::{
    roles::{
        MIN_TABLE_CELL_SPAN, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR,
        TABLE_CELL_ROWSPAN_ATTR,
    },
    TableRole,
};

mod inline;

const BLOCK_SEPARATOR: &str = "\n\n";
const CONTAINER_BOUNDARY: &str = "<!-- -->";
const BULLET_MARKER: &str = "- ";
const THEMATIC_BREAK: &str = "---";
const BLOCKQUOTE_PREFIX: &str = "> ";
const TASK_CHECKED_MARKER: &str = "[x] ";
const TASK_UNCHECKED_MARKER: &str = "[ ] ";
const TABLE_DELIMITER_CELL: &str = "---";
const MIN_CODE_FENCE_LENGTH: usize = 3;
const MAX_ORDERED_MARKER: u64 = 999_999_999;
const ORDERED_START_ATTR: &str = "start";
const CODE_LANGUAGE_ATTR: &str = "language";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MarkdownFlavor {
    #[default]
    Commonmark,
    Gfm,
}

#[derive(Clone, Copy)]
enum BlockKind {
    Document,
    Paragraph,
    Heading(usize),
    Quote,
    List(bool),
    Item,
    Code,
    Rule,
    Image,
    Table,
    Html,
}

pub fn to_markdown(document: &Document, schema: &Schema, flavor: MarkdownFlavor) -> String {
    enum Frame<'a> {
        Enter(&'a Node),
        Exit(&'a Node, BlockKind, usize),
    }
    let mut frames = vec![Frame::Enter(document.root())];
    let mut rendered = Vec::<String>::new();
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Enter(node) => {
                let kind = block_kind(node, schema, flavor);
                match kind {
                    BlockKind::Paragraph | BlockKind::Heading(_) => {
                        let content = inline::serialize(node, schema, flavor, false);
                        let block = match kind {
                            BlockKind::Heading(level) if !content.is_empty() => {
                                format!("{} {content}", "#".repeat(level))
                            }
                            _ if content.is_empty() => super::html_out::node_to_html(node, schema),
                            _ => content,
                        };
                        rendered.push(block);
                    }
                    BlockKind::Code => rendered.push(code_block(node, schema)),
                    BlockKind::Rule => rendered.push(THEMATIC_BREAK.into()),
                    BlockKind::Table => rendered.push(table(node, schema, flavor)),
                    BlockKind::Image => rendered.push(inline::image(node, schema, false)),
                    BlockKind::Html => {
                        rendered.push(super::html_out::node_to_block_html(node, schema))
                    }
                    _ => {
                        frames.push(Frame::Exit(node, kind, rendered.len()));
                        if let Some(content) = node.content() {
                            frames.extend(content.iter().rev().map(|child| Frame::Enter(child)));
                        }
                    }
                }
            }
            Frame::Exit(node, kind, start) => {
                let children = rendered.split_off(start);
                let block = match kind {
                    BlockKind::Quote => prefix_lines(
                        &join_blocks(node, children, schema, flavor),
                        BLOCKQUOTE_PREFIX,
                        BLOCKQUOTE_PREFIX,
                    ),
                    BlockKind::List(ordered) => list(node, children, ordered, schema, flavor),
                    _ => join_blocks(node, children, schema, flavor),
                };
                rendered.push(block);
            }
        }
    }
    rendered.pop().unwrap_or_default()
}

fn node_tag<'a>(node: &Node, schema: &'a Schema) -> Option<&'a str> {
    schema.node(node.node_type()).and_then(|spec| {
        spec.html_rules
            .as_ref()
            .map(|rules| rules.tag.as_str())
            .or(spec.html_tag.as_deref())
    })
}

fn block_kind(node: &Node, schema: &Schema, flavor: MarkdownFlavor) -> BlockKind {
    let Some(spec) = schema.node(node.node_type()) else {
        return BlockKind::Html;
    };
    if spec.table_role == Some(TableRole::Table) {
        return if flavor == MarkdownFlavor::Gfm && pipe_table_eligible(node, schema) {
            BlockKind::Table
        } else {
            BlockKind::Html
        };
    }
    if node
        .content()
        .is_some_and(|content| content.iter().any(|child| child.node_type() == "__opaque"))
    {
        return BlockKind::Html;
    }
    // Explicit HTML rules carry custom semantics and attributes.
    if spec.html_rules.is_some() && !super::is_tiptap_task_item(spec) && !is_task_list(node, schema)
    {
        return BlockKind::Html;
    }
    match spec.role {
        NodeRole::Doc => BlockKind::Document,
        NodeRole::List { ordered } => {
            let start = ordered_start(node);
            let custom_attrs = node.attrs().keys().any(|name| name != ORDERED_START_ATTR)
                || node.content().is_some_and(|items| {
                    items.iter().any(|item| {
                        item.attrs().keys().any(|name| {
                            name != super::TASK_CHECKED_ATTR
                                || !schema
                                    .node(item.node_type())
                                    .is_some_and(super::is_tiptap_task_item)
                        })
                    })
                });
            if custom_attrs
                || (flavor == MarkdownFlavor::Commonmark && is_task_list(node, schema))
                || start > MAX_ORDERED_MARKER
                || start.saturating_add(node.child_count().saturating_sub(1) as u64)
                    > MAX_ORDERED_MARKER
            {
                BlockKind::Html
            } else {
                BlockKind::List(ordered)
            }
        }
        NodeRole::ListItem => BlockKind::Item,
        NodeRole::TextBlock => match node_tag(node, schema) {
            Some("p") if text_block_attrs_inherit(node) => BlockKind::Paragraph,
            Some("pre") => BlockKind::Code,
            Some(tag)
                if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
                    && text_block_attrs_inherit(node)
                    && !node.content().is_some_and(|content| {
                        content.iter().any(|child| {
                            schema
                                .node(child.node_type())
                                .is_some_and(|spec| matches!(spec.role, NodeRole::HardBreak))
                        })
                    }) =>
            {
                BlockKind::Heading(usize::from(tag.as_bytes()[1] - b'0'))
            }
            _ => BlockKind::Html,
        },
        _ => match node_tag(node, schema) {
            Some("blockquote") if node.attrs().is_empty() => BlockKind::Quote,
            Some("hr") if node.attrs().is_empty() => BlockKind::Rule,
            Some("img") => BlockKind::Image,
            _ => BlockKind::Html,
        },
    }
}

fn join_blocks(
    node: &Node,
    children: Vec<String>,
    schema: &Schema,
    flavor: MarkdownFlavor,
) -> String {
    let mut output = String::new();
    for (index, child) in children.into_iter().enumerate() {
        if index > 0 {
            output.push_str(BLOCK_SEPARATOR);
            let previous = block_kind(
                node.child(index - 1).expect("previous child"),
                schema,
                flavor,
            );
            let current = block_kind(node.child(index).expect("current child"), schema, flavor);
            if matches!(
                (previous, current),
                (BlockKind::List(_), BlockKind::List(_)) | (BlockKind::Quote, BlockKind::Quote)
            ) {
                output.push_str(CONTAINER_BOUNDARY);
                output.push_str(BLOCK_SEPARATOR);
            }
        }
        output.push_str(&child);
    }
    output
}

fn is_task_list(node: &Node, schema: &Schema) -> bool {
    node.content().is_some_and(|content| {
        content.iter().any(|item| {
            schema
                .node(item.node_type())
                .is_some_and(super::is_tiptap_task_item)
        })
    })
}

fn ordered_start(node: &Node) -> u64 {
    node.attrs()
        .get(ORDERED_START_ATTR)
        .and_then(crate::model::integral_unsigned)
        .unwrap_or(u64::from(crate::render::DEFAULT_ORDERED_LIST_START))
}

fn list(
    node: &Node,
    children: Vec<String>,
    ordered: bool,
    schema: &Schema,
    flavor: MarkdownFlavor,
) -> String {
    let loose = node
        .content()
        .is_some_and(|content| content.iter().any(|item| item.child_count() > 1));
    children
        .into_iter()
        .enumerate()
        .map(|(index, content)| {
            let marker = if ordered {
                format!("{}. ", ordered_start(node) + index as u64)
            } else {
                BULLET_MARKER.into()
            };
            let item = node.child(index).expect("rendered list item");
            let task = flavor == MarkdownFlavor::Gfm
                && schema
                    .node(item.node_type())
                    .is_some_and(super::is_tiptap_task_item);
            let task_marker = if !task {
                ""
            } else if item
                .attrs()
                .get(super::TASK_CHECKED_ATTR)
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                TASK_CHECKED_MARKER
            } else {
                TASK_UNCHECKED_MARKER
            };
            let continuation = " ".repeat(marker.len());
            prefix_lines(&content, &format!("{marker}{task_marker}"), &continuation)
        })
        .collect::<Vec<_>>()
        .join(if loose { BLOCK_SEPARATOR } else { "\n" })
}

fn prefix_lines(content: &str, first: &str, continuation: &str) -> String {
    content
        .split('\n')
        .enumerate()
        .map(|(index, line)| format!("{}{line}", if index == 0 { first } else { continuation }))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn longest_run(text: &str, delimiter: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for ch in text.chars() {
        current = if ch == delimiter { current + 1 } else { 0 };
        longest = longest.max(current);
    }
    longest
}

fn code_block(node: &Node, schema: &Schema) -> String {
    let text = node.text_content();
    let language = node
        .attrs()
        .get(CODE_LANGUAGE_ATTR)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if (!text.is_empty() && !text.ends_with('\n'))
        || text.contains('\r')
        || node.attrs().keys().any(|name| name != CODE_LANGUAGE_ATTR)
        || language.chars().any(|ch| ch.is_whitespace() || ch == '`')
        || node.content().is_some_and(|content| {
            content
                .iter()
                .any(|child| !child.is_text() || !child.marks().is_empty())
        })
    {
        return super::html_out::node_to_html(node, schema);
    }
    let fence = "`".repeat(MIN_CODE_FENCE_LENGTH.max(longest_run(&text, '`') + 1));
    format!("{fence}{language}\n{text}{fence}")
}

fn pipe_table_eligible(node: &Node, schema: &Schema) -> bool {
    let Some(rows) = node.content() else {
        return false;
    };
    let width = node.child(0).map_or(0, Node::child_count);
    let standard_rules = |node: &Node| {
        schema
            .node(node.node_type())
            .is_some_and(|spec| spec.html_rules.is_none())
    };
    node.attrs().is_empty()
        && standard_rules(node)
        && width > 0
        && rows.iter().enumerate().all(|(index, row)| {
            row.attrs().is_empty()
                && standard_rules(row)
                && row.child_count() == width
                && row.content().is_some_and(|cells| {
                    cells.iter().all(|cell| {
                        let expected = if index == 0 {
                            TableRole::HeaderCell
                        } else {
                            TableRole::Cell
                        };
                        standard_rules(cell)
                            && cell.attrs().keys().all(|name| {
                                matches!(
                                    name.as_str(),
                                    TABLE_CELL_COLSPAN_ATTR
                                        | TABLE_CELL_ROWSPAN_ATTR
                                        | TABLE_CELL_COLWIDTH_ATTR
                                )
                            })
                            && schema
                                .node(cell.node_type())
                                .is_some_and(|spec| spec.table_role == Some(expected))
                            && [TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_ROWSPAN_ATTR]
                                .iter()
                                .all(|attr| {
                                    cell.attrs()
                                        .get(*attr)
                                        .and_then(crate::model::integral_unsigned)
                                        .unwrap_or(MIN_TABLE_CELL_SPAN)
                                        == MIN_TABLE_CELL_SPAN
                                })
                            && cell
                                .attrs()
                                .get(TABLE_CELL_COLWIDTH_ATTR)
                                .is_none_or(serde_json::Value::is_null)
                            && cell.child_count() == 1
                            && cell.child(0).is_some_and(|child| {
                                matches!(
                                    block_kind(child, schema, MarkdownFlavor::Gfm),
                                    BlockKind::Paragraph
                                )
                            })
                    })
                })
        })
}

fn table(node: &Node, schema: &Schema, flavor: MarkdownFlavor) -> String {
    let mut lines = Vec::new();
    for (index, row) in node.content().expect("eligible table").iter().enumerate() {
        let cells = row
            .content()
            .expect("eligible row")
            .iter()
            .map(|cell| {
                inline::serialize(cell.child(0).expect("eligible cell"), schema, flavor, true)
            })
            .collect::<Vec<_>>();
        lines.push(format!("| {} |", cells.join(" | ")));
        if index == 0 {
            lines.push(format!(
                "| {} |",
                vec![TABLE_DELIMITER_CELL; cells.len()].join(" | ")
            ));
        }
    }
    lines.join("\n")
}

fn text_block_attrs_inherit(node: &Node) -> bool {
    node.attrs()
        .iter()
        .all(|(name, value)| name == crate::alignment::TEXT_ALIGN_ATTR && value.is_null())
}
