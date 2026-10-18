use std::collections::HashMap;
use std::sync::LazyLock;

static EMPTY_ATTRS: LazyLock<HashMap<String, serde_json::Value>> = LazyLock::new(HashMap::new);

#[derive(Debug)]
pub(crate) struct Attrs(Option<Box<HashMap<String, serde_json::Value>>>);

impl Attrs {
    pub(crate) fn new(attrs: HashMap<String, serde_json::Value>) -> Self {
        Self((!attrs.is_empty()).then(|| Box::new(attrs)))
    }

    pub(crate) fn empty() -> &'static HashMap<String, serde_json::Value> {
        &EMPTY_ATTRS
    }

    pub(crate) fn get(&self) -> &HashMap<String, serde_json::Value> {
        self.0.as_deref().unwrap_or(Self::empty())
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let Some(attrs) = self.0.as_deref() else {
            return Some(0);
        };
        let table = std::mem::size_of::<HashMap<String, serde_json::Value>>().checked_add(
            super::hash_table_retained_bytes::<String, serde_json::Value>(attrs.capacity())?,
        )?;
        attrs.iter().try_fold(table, |total, (key, value)| {
            total
                .checked_add(key.capacity())?
                .checked_add(super::json_value_retained_bytes(value)?)
        })
    }
}

impl Clone for Attrs {
    fn clone(&self) -> Self {
        Self(
            self.0
                .as_deref()
                .map(|attrs| Box::new(crate::boundary::clone_json_object_stack_safe(attrs))),
        )
    }
}

impl PartialEq for Attrs {
    fn eq(&self, other: &Self) -> bool {
        crate::boundary::json_objects_equal_stack_safe(self.get(), other.get())
    }
}

impl Eq for Attrs {}

impl Drop for Attrs {
    fn drop(&mut self) {
        if let Some(attrs) = self.0.as_deref_mut() {
            crate::boundary::drop_json_object_values_stack_safe(attrs);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Mark, Node};

    const DEEP_ATTRIBUTE_DEPTH: usize = 100_000;
    const SMALL_STACK_BYTES: usize = 128 * 1024;

    fn deep_attrs() -> std::collections::HashMap<String, serde_json::Value> {
        let mut value = serde_json::Value::Null;
        for _ in 0..DEEP_ATTRIBUTE_DEPTH {
            value = serde_json::Value::Array(vec![value]);
        }
        std::collections::HashMap::from([("deep".to_owned(), value)])
    }

    #[test]
    fn deep_node_and_mark_attributes_clone_compare_and_drop_on_a_small_stack() {
        let void = Node::void("image".to_owned(), deep_attrs());
        let text = Node::text(
            "deep".to_owned(),
            vec![Mark::new("link".to_owned(), deep_attrs())],
        );
        std::thread::Builder::new()
            .name("deep-attribute-payloads".to_owned())
            .stack_size(SMALL_STACK_BYTES)
            .spawn(move || {
                let mark = text.marks()[0].clone();
                assert_eq!(
                    mark,
                    text.marks()[0],
                    "cloned mark attributes compare equal"
                );
                assert_eq!(void.attrs().len(), 1, "void attributes stay attached");
                drop(mark);
                drop(text);
                drop(void);
            })
            .unwrap()
            .join()
            .expect("deep attribute payloads must not overflow a small stack");
    }
}
