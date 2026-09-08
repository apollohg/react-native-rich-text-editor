use super::*;

#[derive(Clone, Copy, Default)]
struct InlineFormatting<'a> {
    marks: [bool; 4],
    sources: [Option<&'a scraper::node::Element>; 4],
    preserve_spaces: bool,
}

pub fn from_clipboard_html_with_limits(
    html: &str,
    schema: &Schema,
    options: &FromHtmlOptions,
    limits: &ResourceLimits,
) -> Result<Document, ParseError> {
    if html.len() > limits.max_input_bytes {
        return Err(ParseError::ResourceLimit {
            limit: limits.max_input_bytes,
            actual: html.len(),
        });
    }
    let parsed = Html::parse_fragment(html);
    let root = parsed.root_element();
    let mut depth = 0usize;
    let mut work = 0usize;
    for edge in root.traverse() {
        match edge {
            Edge::Open(_) => {
                depth += 1;
                work += 1;
                if depth > limits.max_document_depth {
                    return Err(ParseError::ResourceLimit {
                        limit: limits.max_document_depth,
                        actual: depth,
                    });
                }
                if work > limits.max_document_nodes.saturating_mul(4) {
                    return Err(ParseError::ResourceLimit {
                        limit: limits.max_document_nodes.saturating_mul(4),
                        actual: work,
                    });
                }
            }
            Edge::Close(_) => depth -= 1,
        }
    }
    let mut output = String::new();
    let block_children = root.children().any(|child| {
        child
            .value()
            .as_element()
            .is_some_and(|element| is_clipboard_block(element.name()))
    });
    for child in root.children() {
        normalize(
            child,
            schema,
            InlineFormatting::default(),
            block_children,
            &mut output,
        );
    }
    // Added semantic tags are bounded by the admitted source text and nodes.
    from_html_with_limits(&output, schema, options, limits)
}

fn normalize<'a>(
    node: SNodeRef<'a>,
    schema: &Schema,
    formatting: InlineFormatting<'a>,
    block_children: bool,
    output: &mut String,
) {
    let InlineFormatting {
        mut marks,
        mut sources,
        preserve_spaces,
    } = formatting;
    if let Some(text) = node.value().as_text() {
        let at_block_boundary = [node.prev_sibling(), node.next_sibling()]
            .iter()
            .any(|sibling| {
                sibling.is_none_or(|sibling| {
                    sibling
                        .value()
                        .as_element()
                        .is_some_and(|element| is_clipboard_block(element.name()))
                })
            });
        if text.trim().is_empty() && block_children && at_block_boundary && !preserve_spaces {
            return;
        }
        let tags = semantic_mark_tags(schema);
        for (index, (tag, enabled)) in tags.iter().zip(marks).enumerate() {
            if let Some(tag) = tag.filter(|_| enabled) {
                write_open_tag(output, tag, sources[index]);
            }
        }
        if preserve_spaces {
            escape_html_to(text, output);
        } else {
            let mut collapsed = String::with_capacity(text.len());
            let mut space = false;
            for character in text.chars() {
                if matches!(character, ' ' | '\n' | '\r' | '\t' | '\u{000c}') {
                    if !space {
                        collapsed.push(' ');
                    }
                    space = true;
                } else {
                    collapsed.push(character);
                    space = false;
                }
            }
            escape_html_to(&collapsed, output);
        }
        for (tag, enabled) in tags.iter().zip(marks).rev() {
            if let Some(tag) = tag.filter(|_| enabled) {
                output.push_str(&format!("</{tag}>"));
            }
        }
        return;
    }
    let Some(element) = node.value().as_element() else {
        return;
    };
    let tag = element.name();
    if matches!(
        tag,
        "head" | "title" | "meta" | "link" | "style" | "script" | "noscript" | "iframe" | "object"
    ) {
        return;
    }
    let semantic_index = match tag {
        "b" | "strong" => Some(0),
        "i" | "em" => Some(1),
        "u" => Some(2),
        "s" | "strike" | "del" => Some(3),
        _ => None,
    };
    if let Some(index) = semantic_index {
        marks[index] = true;
        sources[index] = Some(element);
    }
    let mut preserve_spaces = preserve_spaces || tag == "pre";
    if let Some(style) = element_attr(element, "style") {
        for declaration in style.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();
            let value = value.trim_end_matches("!important").trim();
            match property.as_str() {
                "font-weight" => {
                    if matches!(value, "normal" | "lighter") {
                        marks[0] = false;
                    } else if matches!(value, "bold" | "bolder") {
                        marks[0] = true;
                    } else if let Ok(weight) = value.parse::<u16>() {
                        marks[0] = weight >= 600;
                    }
                }
                "font-style" => {
                    if value == "normal" {
                        marks[1] = false;
                    } else if value == "italic" || value.starts_with("oblique") {
                        marks[1] = true;
                    }
                }
                "text-decoration" | "text-decoration-line" => {
                    if value == "none" {
                        marks[2] = false;
                        marks[3] = false;
                    } else {
                        marks[2] |= value.split_whitespace().any(|value| value == "underline");
                        marks[3] |= value
                            .split_whitespace()
                            .any(|value| value == "line-through");
                    }
                }
                "white-space" if matches!(value, "pre" | "pre-wrap" | "break-spaces") => {
                    preserve_spaces = true
                }
                _ => {}
            }
        }
    }
    let semantic_mark = matches!(
        tag,
        "b" | "strong" | "i" | "em" | "u" | "s" | "strike" | "del"
    );
    let known = schema.node_by_html_tag(tag).is_some()
        || schema.mark_by_html_tag(tag).is_some()
        || mark_from_element(tag, element, schema).is_some()
        || build_mention_node(node, element, schema).is_some()
        || build_html_rules_node(tag, element, schema).is_some();
    let block_wrapper = !known && is_clipboard_block(tag);
    let contains_blocks = node.children().any(|child| {
        child
            .value()
            .as_element()
            .is_some_and(|element| is_clipboard_block(element.name()))
    });
    let emitted_tag = if semantic_mark {
        None
    } else if known {
        Some(tag)
    } else if block_wrapper && !contains_blocks {
        Some("p")
    } else {
        None
    };
    if let Some(emitted) = emitted_tag {
        write_open_tag(output, emitted, known.then_some(element));
    }
    for child in node.children() {
        normalize(
            child,
            schema,
            InlineFormatting {
                marks,
                sources,
                preserve_spaces,
            },
            contains_blocks,
            output,
        );
    }
    if let Some(emitted) = emitted_tag.filter(|tag| !is_void_html_element(tag)) {
        output.push_str("</");
        output.push_str(emitted);
        output.push('>');
    }
}

fn is_clipboard_block(tag: &str) -> bool {
    is_block_html_element(tag) || matches!(tag, "tbody" | "thead" | "tfoot" | "tr" | "td" | "th")
}

fn semantic_mark_tags(schema: &Schema) -> [Option<&'static str>; 4] {
    [
        ("bold", "strong"),
        ("italic", "em"),
        ("underline", "u"),
        ("strike", "s"),
    ]
    .map(|(name, tag)| {
        (schema.mark(name).is_some() || schema.mark_by_html_tag(tag).is_some()).then_some(tag)
    })
}

fn write_open_tag(output: &mut String, tag: &str, source: Option<&scraper::node::Element>) {
    output.push('<');
    output.push_str(tag);
    if let Some(source) = source {
        for (key, value) in source.attrs() {
            if key == "style" || key.starts_with("on") {
                continue;
            }
            output.push(' ');
            output.push_str(key);
            output.push_str("=\"");
            escape_html_to(value, output);
            output.push('"');
        }
    }
    output.push('>');
}
