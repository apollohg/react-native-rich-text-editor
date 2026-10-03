use super::{JsonParseError, UnknownTypeMode};
use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::schema::Schema;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::borrow::Cow;

#[cfg(test)]
std::thread_local! {
    static PLAIN_JSON_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn with_legacy_json_for_test<R>(action: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            PLAIN_JSON_DISABLED.set(self.0);
        }
    }
    let _restore = Restore(PLAIN_JSON_DISABLED.replace(true));
    action()
}

#[derive(Deserialize)]
#[serde(transparent)]
struct BorrowedString<'input>(#[serde(borrow)] Cow<'input, str>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlainNode<'input> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'input, str>,
    #[serde(default, borrow)]
    content: Vec<PlainNode<'input>>,
    #[serde(default, borrow)]
    text: Option<BorrowedString<'input>>,
}

pub(crate) fn try_from_plain_json(
    input: &str,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Option<Result<Document, JsonParseError>> {
    #[cfg(test)]
    if PLAIN_JSON_DISABLED.get() {
        return None;
    }
    let root: PlainNode<'_> = serde_json::from_str(input).ok()?;
    let mut known = std::collections::HashSet::new();
    let mut pending = vec![&root];
    let empty_attrs = Map::new();
    while let Some(node) = pending.pop() {
        let name = node.kind.as_ref();
        if known.insert(name)
            && (schema
                .node(name)
                .is_none_or(|spec| spec.json_projection.is_some())
                || schema.projected_nodes_for_json(name).next().is_some()
                || super::normalized_wire_json_node_type(name, &empty_attrs) != name)
        {
            return None;
        }
        pending.extend(&node.content);
    }
    let mut budget = super::ParseBudget::new(limits);
    Some(
        super::parse_node(
            Input::Plain(&root),
            schema,
            UnknownTypeMode::Preserve,
            "block",
            &mut budget,
        )
        .map(Document::new),
    )
}

#[derive(Clone, Copy)]
pub(super) enum Input<'node, 'input> {
    Value(&'node Value),
    Plain(&'node PlainNode<'input>),
}

impl<'node, 'input> Input<'node, 'input> {
    pub(super) fn node_type(self) -> Result<&'node str, JsonParseError> {
        match self {
            Self::Value(value) => value
                .as_object()
                .ok_or_else(|| {
                    JsonParseError::InvalidStructure("node must be a JSON object".into())
                })?
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    JsonParseError::InvalidStructure(
                        "node must have a string \"type\" field".into(),
                    )
                }),
            Self::Plain(node) => Ok(&node.kind),
        }
    }

    pub(super) fn attrs(self) -> Option<&'node Map<String, Value>> {
        match self {
            Self::Value(value) => value.get("attrs").and_then(Value::as_object),
            Self::Plain(_) => None,
        }
    }

    pub(super) fn text(self) -> Option<&'node str> {
        match self {
            Self::Value(value) => value.get("text").and_then(Value::as_str),
            Self::Plain(node) => node.text.as_ref().map(|text| text.0.as_ref()),
        }
    }

    pub(super) fn marks(self) -> Option<&'node Value> {
        match self {
            Self::Value(value) => value.get("marks"),
            Self::Plain(_) => None,
        }
    }

    pub(super) fn children(self) -> Result<Children<'node, 'input>, JsonParseError> {
        match self {
            Self::Value(value) => {
                let children = match value.get("content") {
                    Some(value) => value.as_array().map(Vec::as_slice).ok_or_else(|| {
                        JsonParseError::InvalidStructure("\"content\" must be an array".into())
                    })?,
                    None => &[],
                };
                Ok(Children::Values(children.iter()))
            }
            Self::Plain(node) => Ok(Children::Plain(node.content.iter())),
        }
    }

    pub(super) fn original_value(self) -> &'node Value {
        match self {
            Self::Value(value) => value,
            Self::Plain(_) => unreachable!("plain JSON only contains native schema node types"),
        }
    }
}

pub(super) enum Children<'node, 'input> {
    Values(std::slice::Iter<'node, Value>),
    Plain(std::slice::Iter<'node, PlainNode<'input>>),
}

impl<'node, 'input> Iterator for Children<'node, 'input> {
    type Item = Input<'node, 'input>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.next().map(Input::Value),
            Self::Plain(nodes) => nodes.next().map(Input::Plain),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Values(values) => values.size_hint(),
            Self::Plain(nodes) => nodes.size_hint(),
        }
    }
}

impl DoubleEndedIterator for Children<'_, '_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Values(values) => values.next_back().map(Input::Value),
            Self::Plain(nodes) => nodes.next_back().map(Input::Plain),
        }
    }
}
impl ExactSizeIterator for Children<'_, '_> {}
