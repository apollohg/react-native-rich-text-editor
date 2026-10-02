fn apply_children<P: XmlFragment>(
    parent: &P,
    txn: &mut TransactionMut<'_>,
    old_children: &[Value],
    new_children: &[Value],
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<()> {
    stacker::maybe_grow(
        RECURSION_RED_ZONE_BYTES,
        RECURSION_STACK_SEGMENT_BYTES,
        || apply_children_inner(parent, txn, old_children, new_children, depth, budget),
    )
}

fn apply_children_inner<P: XmlFragment>(
    parent: &P,
    txn: &mut TransactionMut<'_>,
    old_children: &[Value],
    new_children: &[Value],
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<()> {
    let mut prefix = 0usize;
    while prefix < old_children.len()
        && prefix < new_children.len()
        && nodes_are_compatible(&old_children[prefix], &new_children[prefix])
    {
        apply_child_at(
            parent,
            txn,
            prefix as u32,
            &old_children[prefix],
            &new_children[prefix],
            depth,
            budget,
        )?;
        prefix += 1;
    }

    let mut old_suffix = old_children.len();
    let mut new_suffix = new_children.len();
    while old_suffix > prefix
        && new_suffix > prefix
        && nodes_are_compatible(&old_children[old_suffix - 1], &new_children[new_suffix - 1])
    {
        old_suffix -= 1;
        new_suffix -= 1;
    }

    let old_mid_len = old_suffix.saturating_sub(prefix) as u32;
    if old_mid_len > 0 {
        parent.remove_range(txn, prefix as u32, old_mid_len);
    }

    for (offset, node) in new_children[prefix..new_suffix].iter().enumerate() {
        insert_json_node(
            parent,
            txn,
            prefix as u32 + offset as u32,
            node,
            depth,
            budget,
        )?;
    }

    let suffix_len = old_children.len().saturating_sub(old_suffix);
    for offset in 0..suffix_len {
        let new_index = new_suffix + offset;
        let parent_index = (new_suffix + offset) as u32;
        apply_child_at(
            parent,
            txn,
            parent_index,
            &old_children[old_suffix + offset],
            &new_children[new_index],
            depth,
            budget,
        )?;
    }
    Ok(())
}

fn apply_child_at<P: XmlFragment>(
    parent: &P,
    txn: &mut TransactionMut<'_>,
    index: u32,
    old_node: &Value,
    new_node: &Value,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<()> {
    budget.admit_node(depth)?;
    let Some(current) = parent.get(txn, index) else {
        return Ok(());
    };

    match current {
        XmlOut::Element(element) => {
            apply_element_node(&element, txn, old_node, new_node, depth, budget)
        }
        XmlOut::Text(text) => {
            apply_text_node(&text, txn, old_node, new_node);
            Ok(())
        }
        XmlOut::Fragment(_) => Ok(()),
    }
}

fn apply_element_node(
    element: &XmlElementRef,
    txn: &mut TransactionMut<'_>,
    old_node: &Value,
    new_node: &Value,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<()> {
    let old_attrs = old_node.get("attrs").and_then(Value::as_object);
    let new_attrs = new_node.get("attrs").and_then(Value::as_object);

    if old_attrs != new_attrs {
        let existing = element
            .attributes(txn)
            .map(|(key, _)| key.to_string())
            .collect::<Vec<_>>();
        for key in existing {
            element.remove_attribute(txn, &key);
        }
        if let Some(attrs) = new_attrs {
            for (key, value) in attrs {
                element.insert_attribute(txn, key.as_str(), json_to_any(value));
            }
        }
    }

    let old_children = old_node
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let new_children = new_node
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    apply_children(element, txn, old_children, new_children, depth + 1, budget)
}

fn apply_text_node(
    text: &XmlTextRef,
    txn: &mut TransactionMut<'_>,
    old_node: &Value,
    new_node: &Value,
) {
    let old_marks = normalize_marks(old_node.get("marks").and_then(Value::as_array));
    let new_marks = normalize_marks(new_node.get("marks").and_then(Value::as_array));
    let old_text = old_node.get("text").and_then(Value::as_str).unwrap_or("");
    let new_text = new_node.get("text").and_then(Value::as_str).unwrap_or("");

    if old_marks != new_marks {
        let len = text.len(txn);
        if len > 0 {
            text.remove_range(txn, 0, len);
        }
        if !new_text.is_empty() {
            text.insert_with_attributes(txn, 0, new_text, marks_to_attrs(Some(&new_marks)));
        }
        return;
    }

    let (prefix, old_suffix, new_suffix) = shared_text_bounds(old_text, new_text);
    let prefix_utf16 = old_text[..prefix].encode_utf16().count() as u32;
    let remove_len = old_text[prefix..old_suffix].encode_utf16().count() as u32;
    if remove_len > 0 {
        text.remove_range(txn, prefix_utf16, remove_len);
    }

    let insert_text = &new_text[prefix..new_suffix];
    if !insert_text.is_empty() {
        text.insert_with_attributes(
            txn,
            prefix_utf16,
            insert_text,
            marks_to_attrs(Some(&new_marks)),
        );
    }
}

fn shared_text_bounds(old_text: &str, new_text: &str) -> (usize, usize, usize) {
    let mut prefix = 0usize;
    let mut old_iter = old_text.char_indices().peekable();
    let mut new_iter = new_text.char_indices().peekable();

    while let (Some((old_index, old_char)), Some((new_index, new_char))) =
        (old_iter.peek().copied(), new_iter.peek().copied())
    {
        if old_char != new_char || old_index != prefix || new_index != prefix {
            break;
        }
        prefix += old_char.len_utf8();
        old_iter.next();
        new_iter.next();
    }

    let mut old_suffix = old_text.len();
    let mut new_suffix = new_text.len();
    let old_tail = old_text[prefix..].chars().rev().collect::<Vec<_>>();
    let new_tail = new_text[prefix..].chars().rev().collect::<Vec<_>>();
    for (old_char, new_char) in old_tail.iter().zip(new_tail.iter()) {
        if old_char != new_char {
            break;
        }
        old_suffix -= old_char.len_utf8();
        new_suffix -= new_char.len_utf8();
    }

    (prefix, old_suffix, new_suffix)
}

fn insert_json_node<P: XmlFragment>(
    parent: &P,
    txn: &mut TransactionMut<'_>,
    index: u32,
    node: &Value,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<()> {
    let prepared = prepare_json_node(node, depth, budget)?;
    insert_prepared_node(parent, txn, index, prepared);
    Ok(())
}

fn prepare_json_node(
    node: &Value,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<PreparedXmlNode> {
    stacker::maybe_grow(
        RECURSION_RED_ZONE_BYTES,
        RECURSION_STACK_SEGMENT_BYTES,
        || prepare_json_node_inner(node, depth, budget),
    )
}

fn prepare_json_node_inner(
    node: &Value,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<PreparedXmlNode> {
    budget.admit_node(depth)?;
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
    if node_type == "text" {
        let text_value = node.get("text").and_then(Value::as_str).unwrap_or("");
        return prepare_text_node(
            text_value,
            node.get("marks").and_then(Value::as_array),
            budget,
        );
    }

    let attrs = node
        .get("attrs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let (tag, attrs) = prepare_element_attrs(node_type, attrs, budget)?;
    let children = prepare_xml_children(
        node.get("content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        depth + 1,
        budget,
        prepare_json_node,
    )?;
    Ok(PreparedXmlNode::Element {
        tag,
        attrs,
        children,
    })
}

fn prepare_model_node(
    node: &crate::model::Node,
    schema: &Schema,
    depth: usize,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<PreparedXmlNode> {
    stacker::maybe_grow(
        RECURSION_RED_ZONE_BYTES,
        RECURSION_STACK_SEGMENT_BYTES,
        || {
            use crate::serialize::json_out::{
                build_attrs_json, projected_marks, projected_node_type,
            };
            if node.node_type() == "__opaque_json" {
                return prepare_json_node(
                    node.attrs().get("original_json").unwrap_or(&Value::Null),
                    depth,
                    budget,
                );
            }
            budget.admit_node(depth)?;
            let spec = schema.node(node.node_type());
            let node_type = projected_node_type(node, spec);
            if node_type == "text" {
                let marks = if node.is_text() {
                    projected_marks(node)
                } else {
                    Vec::new()
                };
                if marks.is_empty() {
                    return prepare_text_node(node.text_str().unwrap_or(""), None, budget);
                }
                let marks = crate::boundary::StackSafeJsonValue::new(Value::Array(marks));
                return prepare_text_node(
                    node.text_str().unwrap_or(""),
                    marks.as_value().as_array(),
                    budget,
                );
            }
            let attrs = if node.is_text() {
                Map::new()
            } else {
                build_attrs_json(node, spec)
            };
            let (tag, attrs) = prepare_element_attrs(node_type, attrs, budget)?;
            let children = prepare_xml_children(
                node.content()
                    .map(|content| content.children())
                    .unwrap_or(&[]),
                depth + 1,
                budget,
                |child, depth, budget| prepare_model_node(child, schema, depth, budget),
            )?;
            Ok(PreparedXmlNode::Element {
                tag,
                attrs,
                children,
            })
        },
    )
}

fn prepare_text_node(
    text: &str,
    marks: Option<&Vec<Value>>,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<PreparedXmlNode> {
    budget.charge_output(text.len())?;
    Ok(PreparedXmlNode::Text {
        runs: vec![PreparedTextRun {
            index_utf16: 0,
            text: text.to_owned(),
            attrs: prepare_marks_to_attrs(marks, budget)?,
        }],
    })
}

fn prepare_element_attrs(
    node_type: &str,
    mut attrs: Map<String, Value>,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<(String, Vec<(String, Any)>)> {
    let tag = element_name_for_json_node(node_type, &mut attrs);
    if attrs.is_empty() {
        return Ok((tag, Vec::new()));
    }
    let attrs = crate::boundary::StackSafeJsonValue::new(Value::Object(attrs));
    let attrs = attrs
        .as_value()
        .as_object()
        .expect("prepared attrs remain an object");
    let mut prepared = Vec::with_capacity(attrs.len());
    let mut sorted_attrs = attrs.iter().collect::<Vec<_>>();
    sorted_attrs.sort_by_key(|(key, _)| *key);
    for (key, value) in sorted_attrs {
        budget.charge_output(key.len())?;
        prepared.push((key.clone(), prepare_json_value(value, budget, 1)?));
    }
    Ok((tag, prepared))
}

fn prepare_xml_children<'limits, T>(
    nodes: &[T],
    depth: usize,
    budget: &mut ConversionBudget<'limits>,
    mut prepare: impl FnMut(
        &T,
        usize,
        &mut ConversionBudget<'limits>,
    ) -> YrsEngineResult<PreparedXmlNode>,
) -> YrsEngineResult<Vec<PreparedXmlChild>> {
    let mut children = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        children.push(PreparedXmlChild {
            index: u32::try_from(index).map_err(|_| {
                YrsEngineError::new(
                    "DOCUMENT_LIMIT_EXCEEDED",
                    "prepared XML child index exceeds u32",
                )
            })?,
            node: prepare(node, depth, budget)?,
        });
    }
    Ok(children)
}

fn prepare_marks_to_attrs(
    marks: Option<&Vec<Value>>,
    budget: &mut ConversionBudget<'_>,
) -> YrsEngineResult<Attrs> {
    let mut attrs = Attrs::default();
    let Some(marks) = marks else {
        return Ok(attrs);
    };
    for mark in marks {
        let Some(mark_type) = mark.get("type").and_then(Value::as_str) else {
            continue;
        };
        budget.charge_output(mark_type.len())?;
        let value = prepare_json_value(
            mark.get("attrs").unwrap_or(&Value::Object(Map::new())),
            budget,
            1,
        )?;
        attrs.insert(mark_type.into(), value);
    }
    Ok(attrs)
}

fn prepare_json_value(
    value: &Value,
    budget: &mut ConversionBudget<'_>,
    depth: usize,
) -> YrsEngineResult<Any> {
    stacker::maybe_grow(
        RECURSION_RED_ZONE_BYTES,
        RECURSION_STACK_SEGMENT_BYTES,
        || prepare_json_value_inner(value, budget, depth),
    )
}

fn prepare_json_value_inner(
    value: &Value,
    budget: &mut ConversionBudget<'_>,
    depth: usize,
) -> YrsEngineResult<Any> {
    budget.admit_any(depth, 1)?;
    match value {
        Value::Null => Ok(Any::Null),
        Value::Bool(value) => Ok(Any::Bool(*value)),
        Value::Number(number) => wire_number_to_any(number).map_err(|error| match error {
            WireNumberError::ExceedsExactIntegerRange => YrsEngineError::new(
                "DOCUMENT_INVALID",
                format!("numeric attribute {number} exceeds the exact Yjs integer range"),
            ),
            WireNumberError::NotFinite => {
                YrsEngineError::new("DOCUMENT_INVALID", "numeric attribute must be finite")
            }
            WireNumberError::NotRepresentable => {
                YrsEngineError::new("DOCUMENT_INVALID", "numeric attribute is not representable")
            }
        }),
        Value::String(value) => {
            budget.admit_any(depth, value.len())?;
            budget.charge_output(value.len())?;
            Ok(Any::String(value.clone().into()))
        }
        Value::Array(values) => {
            budget.admit_any(depth, values.len())?;
            let mut prepared = Vec::with_capacity(values.len());
            for value in values {
                prepared.push(prepare_json_value(value, budget, depth + 1)?);
            }
            Ok(Any::Array(prepared.into()))
        }
        Value::Object(values) => {
            budget.admit_any(depth, values.len())?;
            let mut prepared = std::collections::HashMap::with_capacity(values.len());
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            for (key, value) in entries {
                budget.admit_any(depth, key.len())?;
                budget.charge_output(key.len())?;
                prepared.insert(key.clone(), prepare_json_value(value, budget, depth + 1)?);
            }
            Ok(Any::Map(Arc::new(prepared)))
        }
    }
}
