use std::collections::HashMap;

use crate::schema::content_rule::ContentRule;
use crate::schema::{AttrSpec, MarkSpec, NodeJsonProjection, NodeRole, NodeSpec, Schema};

/// Build the Tiptap-compatible schema using camelCase node names.
///
/// Node names: doc, paragraph, blockquote, codeBlock, bulletList, orderedList,
///             listItem, hardBreak, horizontalRule, image, text.
/// Mark names: bold, italic, underline, strike, code, link.
pub fn tiptap_schema() -> Schema {
    build_schema(NamingConvention::CamelCase)
}

/// Build the default schema using ProseMirror names, including upstream `codeBlock`.
pub fn default_schema() -> Schema {
    prosemirror_schema()
}

/// Build the standard ProseMirror schema, including upstream `codeBlock`.
///
/// Node names: doc, paragraph, blockquote, codeBlock, bullet_list, ordered_list,
///             list_item, hard_break, horizontal_rule, image, text.
/// Mark names: bold, italic, underline, strike, code, link.
pub fn prosemirror_schema() -> Schema {
    build_schema(NamingConvention::SnakeCase)
}

#[cfg(any(feature = "table-interop", test))]
pub(crate) use tabled::{prosemirror_table_schema, tiptap_table_schema};

#[cfg(any(feature = "table-interop", test))]
mod tabled {
    use std::collections::HashMap;

    use super::{build_schema_parts, name, NamingConvention};
    use crate::schema::content_rule::ContentRule;
    use crate::schema::{AttrSpec, NodeRole, NodeSpec, Schema};
    use crate::tables::roles::{
        MIN_TABLE_CELL_SPAN, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR,
        TABLE_CELL_ROWSPAN_ATTR,
    };
    use crate::tables::TableRole;

    const TABLE_NODE_GROUP: &str = "block";
    const TABLE_CELL_CONTENT: &str = "block+";
    const NUMBER_ATTR_TYPE: &str = "number";
    const TABLE_NODE_NAME: &str = "table";
    const TABLE_HTML_TAG: &str = "table";
    const ROW_HTML_TAG: &str = "tr";
    const CELL_HTML_TAG: &str = "td";
    const HEADER_CELL_HTML_TAG: &str = "th";

    pub(crate) fn prosemirror_table_schema() -> Schema {
        build_tabled_schema(NamingConvention::SnakeCase)
    }

    pub(crate) fn tiptap_table_schema() -> Schema {
        build_tabled_schema(NamingConvention::CamelCase)
    }

    struct TableNodeNames {
        table: String,
        row: String,
        cell: String,
        header_cell: String,
    }

    fn table_node_names(convention: &NamingConvention) -> TableNodeNames {
        TableNodeNames {
            table: TABLE_NODE_NAME.to_string(),
            row: name(convention, "tableRow", "table_row"),
            cell: name(convention, "tableCell", "table_cell"),
            header_cell: name(convention, "tableHeader", "table_header"),
        }
    }

    fn table_cell_span_attr() -> AttrSpec {
        let mut spec = AttrSpec {
            default: Some(serde_json::json!(MIN_TABLE_CELL_SPAN)),
            has_default: true,
            ..AttrSpec::default()
        };
        spec.constraints
            .insert("type".to_string(), serde_json::json!(NUMBER_ATTR_TYPE));
        spec.constraints
            .insert("min".to_string(), serde_json::json!(MIN_TABLE_CELL_SPAN));
        spec
    }

    fn table_cell_attrs() -> HashMap<String, AttrSpec> {
        let mut attrs = HashMap::new();
        attrs.insert(TABLE_CELL_COLSPAN_ATTR.to_string(), table_cell_span_attr());
        attrs.insert(TABLE_CELL_ROWSPAN_ATTR.to_string(), table_cell_span_attr());
        attrs.insert(
            TABLE_CELL_COLWIDTH_ATTR.to_string(),
            AttrSpec {
                default: Some(serde_json::Value::Null),
                has_default: true,
                ..AttrSpec::default()
            },
        );
        attrs
    }

    fn table_cell_node(name: String, html_tag: &str, table_role: TableRole) -> NodeSpec {
        NodeSpec {
            name,
            content: ContentRule::parse(TABLE_CELL_CONTENT).unwrap(),
            group: None,
            attrs: table_cell_attrs(),
            role: NodeRole::Block,
            html_tag: Some(html_tag.to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: Some(table_role),
        }
    }

    fn table_node_specs(convention: &NamingConvention) -> Vec<NodeSpec> {
        let names = table_node_names(convention);
        vec![
            NodeSpec {
                name: names.table.clone(),
                content: ContentRule::parse(&format!("{}+", names.row)).unwrap(),
                group: Some(TABLE_NODE_GROUP.to_string()),
                attrs: HashMap::new(),
                role: NodeRole::Block,
                html_tag: Some(TABLE_HTML_TAG.to_string()),
                html_rules: None,
                json_projection: None,
                is_void: false,
                deletable_on_backspace: None,
                allow_undeclared_attrs: false,
                table_role: Some(TableRole::Table),
            },
            NodeSpec {
                name: names.row.clone(),
                content: ContentRule::parse(&format!("({} | {})*", names.cell, names.header_cell))
                    .unwrap(),
                group: None,
                attrs: HashMap::new(),
                role: NodeRole::Block,
                html_tag: Some(ROW_HTML_TAG.to_string()),
                html_rules: None,
                json_projection: None,
                is_void: false,
                deletable_on_backspace: None,
                allow_undeclared_attrs: false,
                table_role: Some(TableRole::Row),
            },
            table_cell_node(names.cell, CELL_HTML_TAG, TableRole::Cell),
            table_cell_node(
                names.header_cell,
                HEADER_CELL_HTML_TAG,
                TableRole::HeaderCell,
            ),
        ]
    }

    fn build_tabled_schema(convention: NamingConvention) -> Schema {
        let (mut nodes, marks) = build_schema_parts(&convention);
        nodes.extend(table_node_specs(&convention));
        Schema::new(nodes, marks)
    }
}

enum NamingConvention {
    CamelCase,
    SnakeCase,
}

/// Resolve a node name based on the naming convention.
///
/// The `camel` parameter is the camelCase name, `snake` is the snake_case
/// alternative. For names that are identical in both conventions (e.g.
/// "paragraph", "doc", "text"), pass the same value for both.
fn name(convention: &NamingConvention, camel: &str, snake: &str) -> String {
    match convention {
        NamingConvention::CamelCase => camel.to_string(),
        NamingConvention::SnakeCase => snake.to_string(),
    }
}

fn build_schema(convention: NamingConvention) -> Schema {
    let (nodes, marks) = build_schema_parts(&convention);
    Schema::new(nodes, marks)
}

fn build_schema_parts(convention: &NamingConvention) -> (Vec<NodeSpec>, Vec<MarkSpec>) {
    let list_item_name = name(convention, "listItem", "list_item");
    let mut nodes = vec![
        NodeSpec {
            name: "doc".to_string(),
            content: ContentRule::parse("block+").unwrap(),
            group: None,
            attrs: HashMap::new(),
            role: NodeRole::Doc,
            html_tag: None,
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: "paragraph".to_string(),
            content: ContentRule::parse("inline*").unwrap(),
            group: Some("block".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::TextBlock,
            html_tag: Some("p".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
    ];

    for level in 1..=6 {
        nodes.push(NodeSpec {
            name: format!("h{level}"),
            content: ContentRule::parse("inline*").unwrap(),
            group: Some("block heading".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::TextBlock,
            html_tag: Some(format!("h{level}")),
            html_rules: None,
            json_projection: Some(NodeJsonProjection {
                node_type: "heading".to_string(),
                attrs: HashMap::from([(
                    "level".to_string(),
                    serde_json::Value::Number(level.into()),
                )]),
            }),
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        });
    }

    nodes.extend([
        NodeSpec {
            name: "blockquote".to_string(),
            content: ContentRule::parse("block+").unwrap(),
            group: Some("block".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::Block,
            html_tag: Some("blockquote".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: "codeBlock".to_string(),
            content: ContentRule::parse("text*").unwrap(),
            group: Some("block".to_string()),
            attrs: HashMap::from([(
                "language".to_string(),
                AttrSpec {
                    default: Some(serde_json::Value::Null),
                    has_default: true,
                    ..AttrSpec::default()
                },
            )]),
            role: NodeRole::TextBlock,
            html_tag: Some("pre".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: name(&convention, "bulletList", "bullet_list"),
            content: ContentRule::parse(&format!("{list_item_name}+")).unwrap(),
            group: Some("block".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::List { ordered: false },
            html_tag: Some("ul".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: name(&convention, "orderedList", "ordered_list"),
            content: ContentRule::parse(&format!("{list_item_name}+")).unwrap(),
            group: Some("block".to_string()),
            attrs: {
                let mut attrs = HashMap::new();
                attrs.insert(
                    "start".to_string(),
                    AttrSpec {
                        default: Some(serde_json::Value::Number(1.into())),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
                attrs
            },
            role: NodeRole::List { ordered: true },
            html_tag: Some("ol".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: list_item_name,
            content: ContentRule::parse("paragraph block*").unwrap(),
            group: None,
            attrs: HashMap::new(),
            role: NodeRole::ListItem,
            html_tag: Some("li".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: name(&convention, "hardBreak", "hard_break"),
            content: ContentRule::parse("").unwrap(),
            group: Some("inline".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::HardBreak,
            html_tag: Some("br".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: true,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: name(&convention, "horizontalRule", "horizontal_rule"),
            content: ContentRule::parse("").unwrap(),
            group: Some("block".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::Block,
            html_tag: Some("hr".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: true,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: "image".to_string(),
            content: ContentRule::parse("").unwrap(),
            group: Some("block".to_string()),
            attrs: {
                let mut attrs = HashMap::new();
                attrs.insert(
                    "src".to_string(),
                    AttrSpec {
                        default: None,
                        has_default: false,
                        ..AttrSpec::default()
                    },
                );
                attrs.insert(
                    "alt".to_string(),
                    AttrSpec {
                        default: Some(serde_json::Value::Null),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
                attrs.insert(
                    "title".to_string(),
                    AttrSpec {
                        default: Some(serde_json::Value::Null),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
                attrs.insert(
                    "width".to_string(),
                    AttrSpec {
                        default: Some(serde_json::Value::Null),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
                attrs.insert(
                    "height".to_string(),
                    AttrSpec {
                        default: Some(serde_json::Value::Null),
                        has_default: true,
                        ..AttrSpec::default()
                    },
                );
                attrs
            },
            role: NodeRole::Block,
            html_tag: Some("img".to_string()),
            html_rules: None,
            json_projection: None,
            is_void: true,
            deletable_on_backspace: Some(false),
            allow_undeclared_attrs: false,
            table_role: None,
        },
        NodeSpec {
            name: "text".to_string(),
            content: ContentRule::parse("").unwrap(),
            group: Some("inline".to_string()),
            attrs: HashMap::new(),
            role: NodeRole::Text,
            html_tag: None,
            html_rules: None,
            json_projection: None,
            is_void: false,
            deletable_on_backspace: None,
            allow_undeclared_attrs: false,
            table_role: None,
        },
    ]);

    let marks = vec![
        MarkSpec {
            name: "bold".to_string(),
            html_tag: Some("strong".to_string()),
            attrs: HashMap::new(),
            excludes: None,
            allow_undeclared_attrs: false,
        },
        MarkSpec {
            name: "italic".to_string(),
            html_tag: Some("em".to_string()),
            attrs: HashMap::new(),
            excludes: None,
            allow_undeclared_attrs: false,
        },
        MarkSpec {
            name: "underline".to_string(),
            html_tag: Some("u".to_string()),
            attrs: HashMap::new(),
            excludes: None,
            allow_undeclared_attrs: false,
        },
        MarkSpec {
            name: "strike".to_string(),
            html_tag: Some("s".to_string()),
            attrs: HashMap::new(),
            excludes: None,
            allow_undeclared_attrs: false,
        },
        MarkSpec {
            name: "code".to_string(),
            html_tag: Some("code".to_string()),
            attrs: HashMap::new(),
            excludes: None,
            allow_undeclared_attrs: false,
        },
        MarkSpec {
            name: "link".to_string(),
            html_tag: Some("a".to_string()),
            attrs: {
                let mut attrs = HashMap::new();
                attrs.insert(
                    "href".to_string(),
                    AttrSpec {
                        default: None,
                        has_default: false,
                        ..AttrSpec::default()
                    },
                );
                attrs
            },
            excludes: None,
            allow_undeclared_attrs: false,
        },
    ];

    (nodes, marks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiptap_has_all_expected_nodes() {
        let schema = tiptap_schema();
        let expected = [
            "doc",
            "paragraph",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "blockquote",
            "codeBlock",
            "bulletList",
            "orderedList",
            "listItem",
            "hardBreak",
            "horizontalRule",
            "image",
            "text",
        ];
        for name in &expected {
            assert!(
                schema.node(name).is_some(),
                "tiptap schema missing node '{name}'"
            );
        }
    }

    #[test]
    fn test_prosemirror_has_all_expected_nodes() {
        let schema = prosemirror_schema();
        let expected = [
            "doc",
            "paragraph",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "blockquote",
            "codeBlock",
            "bullet_list",
            "ordered_list",
            "list_item",
            "hard_break",
            "horizontal_rule",
            "image",
            "text",
        ];
        for name in &expected {
            assert!(
                schema.node(name).is_some(),
                "prosemirror schema missing node '{name}'"
            );
        }
    }

    #[test]
    fn test_both_schemas_have_all_marks() {
        for schema in &[tiptap_schema(), prosemirror_schema()] {
            for mark_name in &["bold", "italic", "underline", "strike", "code", "link"] {
                assert!(
                    schema.mark(mark_name).is_some(),
                    "schema missing mark '{mark_name}'"
                );
            }
        }
    }

    #[test]
    fn test_ordered_list_has_start_attr() {
        let schema = tiptap_schema();
        let ol = schema.node("orderedList").unwrap();
        let start_attr = ol
            .attrs
            .get("start")
            .expect("orderedList should have 'start' attr");
        assert_eq!(
            start_attr.default,
            Some(serde_json::Value::Number(1.into()))
        );
    }

    #[test]
    fn test_block_group_membership() {
        let schema = tiptap_schema();
        let block_nodes = schema.nodes_in_group("block");
        let block_names: Vec<&str> = block_nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(block_names.contains(&"paragraph"));
        assert!(block_names.contains(&"blockquote"));
        assert!(block_names.contains(&"codeBlock"));
        assert!(block_names.contains(&"bulletList"));
        assert!(block_names.contains(&"orderedList"));
        assert!(block_names.contains(&"horizontalRule"));
        assert!(!block_names.contains(&"doc"));
        assert!(!block_names.contains(&"text"));
    }

    #[test]
    fn heading_variants_retain_the_public_heading_group() {
        for schema in [tiptap_schema(), prosemirror_schema()] {
            for level in 1..=6 {
                let heading = schema.node(&format!("h{level}")).unwrap();
                assert!(heading
                    .group
                    .as_deref()
                    .is_some_and(|group| group.split_whitespace().any(|name| name == "heading")));
            }
        }
    }

    #[test]
    fn test_inline_group_membership() {
        let schema = tiptap_schema();
        let inline_nodes = schema.nodes_in_group("inline");
        let inline_names: Vec<&str> = inline_nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(inline_names.contains(&"text"));
        assert!(inline_names.contains(&"hardBreak"));
        assert!(!inline_names.contains(&"paragraph"));
    }
}
