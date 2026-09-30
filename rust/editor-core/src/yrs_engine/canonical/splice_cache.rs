use super::{canonical_sha256, CanonicalArtifact, CanonicalArtifactInner};
use crate::model::Node;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::ops::Range;
use std::sync::{Arc, Weak};

#[cfg(test)]
std::thread_local! {
    static CACHE_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAILED_ALLOCATION: std::cell::Cell<Option<CacheAllocation>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAllocation {
    Buffer,
    Path,
}

#[derive(Debug)]
pub(crate) struct CanonicalSpliceCache {
    bytes: Vec<u8>,
    path: Vec<u32>,
    range: Range<usize>,
    revision: u64,
    artifact: Weak<CanonicalArtifactInner>,
}

impl CanonicalSpliceCache {
    #[cfg(test)]
    pub(crate) fn failing_allocation_for_test<T>(
        allocation: CacheAllocation,
        operation: impl FnOnce() -> T,
    ) -> T {
        struct Restore(Option<CacheAllocation>);
        impl Drop for Restore {
            fn drop(&mut self) {
                FAILED_ALLOCATION.set(self.0);
            }
        }
        let _restore = Restore(FAILED_ALLOCATION.replace(Some(allocation)));
        let result = operation();
        assert_eq!(
            FAILED_ALLOCATION.get(),
            None,
            "allocation failpoint was not reached"
        );
        result
    }

    #[cfg(test)]
    fn allocation_allowed(allocation: CacheAllocation) -> Option<()> {
        if FAILED_ALLOCATION.get() == Some(allocation) {
            FAILED_ALLOCATION.set(None);
            None
        } else {
            Some(())
        }
    }

    #[cfg(test)]
    pub(crate) fn without_for_test<T>(operation: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                CACHE_DISABLED.set(self.0);
            }
        }
        let _restore = Restore(CACHE_DISABLED.replace(true));
        operation()
    }
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        Self::fixed_bytes()?
            .checked_add(self.bytes.capacity())?
            .checked_add(
                self.path
                    .capacity()
                    .checked_mul(std::mem::size_of::<u32>())?,
            )
    }

    fn fixed_bytes() -> Option<usize> {
        std::mem::size_of::<Option<Self>>().checked_add(
            crate::model::arc_allocation_retained_bytes(
                std::mem::size_of::<CanonicalArtifactInner>(),
            )?,
        )
    }

    pub(crate) fn prepare(
        previous: Option<Self>,
        before: &CanonicalArtifact,
        after: &CanonicalArtifact,
        path: &[u32],
        revision: u64,
        next_revision: u64,
        byte_budget: usize,
    ) -> Option<Self> {
        #[cfg(test)]
        if CACHE_DISABLED.get() {
            return None;
        }
        selected_textblock(after, path)?;
        let previous_bytes = previous.as_ref().map_or(Some(0), Self::retained_bytes)?;
        let expected_len = after.serialized_len();
        let required = previous_bytes
            .checked_add(Self::fixed_bytes()?)?
            .checked_add(expected_len)?
            .checked_add(path.len().checked_mul(std::mem::size_of::<u32>())?)?;
        if required > byte_budget {
            return None;
        }
        let mut bytes = Vec::new();
        #[cfg(test)]
        Self::allocation_allowed(CacheAllocation::Buffer)?;
        bytes.try_reserve_exact(expected_len).ok()?;
        let mut owned_path = Vec::new();
        #[cfg(test)]
        Self::allocation_allowed(CacheAllocation::Path)?;
        owned_path.try_reserve_exact(path.len()).ok()?;
        owned_path.extend_from_slice(path);
        let retained = previous_bytes
            .checked_add(Self::fixed_bytes()?)?
            .checked_add(bytes.capacity())?
            .checked_add(
                owned_path
                    .capacity()
                    .checked_mul(std::mem::size_of::<u32>())?,
            )?;
        if retained > byte_budget {
            return None;
        }
        let reusable = previous.as_ref().filter(|prior| {
            prior.revision == revision
                && prior.path == path
                && prior.artifact.ptr_eq(&Arc::downgrade(&before.0))
                && before.0.schema_context.ptr_eq(&after.0.schema_context)
        });
        let replacement = reusable.and_then(|prior| {
            let node = replacement_node(
                before.0.source_document.root(),
                after.0.source_document.root(),
                path,
            )?;
            let schema = &after.0.schema_context.0.schema;
            if !schema.is_text_block(node.node_type()) {
                return None;
            }
            let replaced_len =
                expected_len.checked_sub(prior.bytes.len().checked_sub(prior.range.len())?)?;
            Some((prior, node, replaced_len))
        });
        let range = if let Some((prior, node, replaced_len)) = replacement {
            bytes.extend_from_slice(prior.bytes.get(..prior.range.start)?);
            crate::serialize::json_out::write_node_json(
                &mut bytes,
                node,
                &after.0.schema_context.0.schema,
            )
            .ok()?;
            if bytes.len().checked_sub(prior.range.start)? != replaced_len {
                return None;
            }
            let end = bytes.len();
            bytes.extend_from_slice(prior.bytes.get(prior.range.end..)?);
            prior.range.start..end
        } else {
            #[cfg(test)]
            {
                super::super::observability::record_canonical_serialization();
                super::SERIALIZATION_COUNT.set(super::SERIALIZATION_COUNT.get().saturating_add(1));
            }
            after.write_canonical_json(&mut bytes).ok()?;
            emitted_range(&bytes, path)?
        };
        if bytes.len() != expected_len {
            return None;
        }
        let candidate = Self {
            bytes,
            path: owned_path,
            range,
            revision: next_revision,
            artifact: Arc::downgrade(&after.0),
        };
        if previous_bytes.checked_add(candidate.retained_bytes()?)? > byte_budget {
            return None;
        }
        let digest = canonical_sha256(&candidate.bytes);
        #[cfg(test)]
        super::super::observability::record_canonical_hash();
        let _ = after.0.sha256.set(digest);
        debug_assert_eq!(after.0.sha256.get(), Some(&digest));
        Some(candidate)
    }
}

fn selected_textblock<'a>(artifact: &'a CanonicalArtifact, path: &[u32]) -> Option<&'a Node> {
    let mut node = artifact.0.source_document.root();
    for &index in path {
        if node.node_type() == "__opaque_json" {
            return None;
        }
        node = node.child(usize::try_from(index).ok()?)?;
    }
    artifact
        .0
        .schema_context
        .0
        .schema
        .is_text_block(node.node_type())
        .then_some(node)
}

fn replacement_node<'a>(mut before: &Node, mut after: &'a Node, path: &[u32]) -> Option<&'a Node> {
    for &index in path {
        let index = usize::try_from(index).ok()?;
        if before.node_type() == "__opaque_json"
            || before.node_type() != after.node_type()
            || before.child_count() != after.child_count()
            || before.marks() != after.marks()
            || !crate::boundary::json_objects_equal_stack_safe(before.attrs(), after.attrs())
        {
            return None;
        }
        for sibling in 0..before.child_count() {
            if sibling != index
                && !before
                    .child(sibling)?
                    .shares_storage_with(after.child(sibling)?)
            {
                return None;
            }
        }
        before = before.child(index)?;
        after = after.child(index)?;
    }
    Some(after)
}

fn emitted_range(bytes: &[u8], path: &[u32]) -> Option<Range<usize>> {
    let mut current: &RawValue = serde_json::from_slice(bytes).ok()?;
    for &index in path {
        let mut deserializer = serde_json::Deserializer::from_str(current.get());
        deserializer.disable_recursion_limit();
        current = ContentChild(usize::try_from(index).ok()?)
            .deserialize(&mut deserializer)
            .ok()??;
        deserializer.end().ok()?;
    }
    let start = (current.get().as_ptr() as usize).checked_sub(bytes.as_ptr() as usize)?;
    let end = start.checked_add(current.get().len())?;
    (end <= bytes.len()).then_some(start..end)
}

struct ContentChild(usize);
impl<'de> DeserializeSeed<'de> for ContentChild {
    type Value = Option<&'de RawValue>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for ContentChild {
    type Value = Option<&'de RawValue>;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a canonical node object")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut child = None;
        while let Some(key) = map.next_key::<&str>()? {
            if key == "content" {
                child = map.next_value_seed(ArrayChild(self.0))?;
            } else {
                let _ = map.next_value::<&RawValue>()?;
            }
        }
        Ok(child)
    }
}
struct ArrayChild(usize);
impl<'de> DeserializeSeed<'de> for ArrayChild {
    type Value = Option<&'de RawValue>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for ArrayChild {
    type Value = Option<&'de RawValue>;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a canonical content array")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        let mut selected = None;
        let mut index = 0usize;
        while let Some(child) = sequence.next_element::<&RawValue>()? {
            if index == self.0 {
                selected = Some(child);
            }
            index += 1;
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Mark};
    use crate::schema::Schema;
    use crate::transform::{apply_step, Step};
    use crate::yrs_engine::canonical::{
        reset_canonical_artifact_counts_for_test, take_canonical_artifact_counts_for_test,
        CanonicalSchemaContext,
    };
    use std::collections::HashMap;

    fn initial(schema: &Schema) -> Document {
        crate::serialize::from_prosemirror_json(
            &serde_json::json!({"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"same"}]},
                {"type":"paragraph","content":[{"type":"text","text":"same"}]}
            ]}),
            schema,
            crate::serialize::UnknownTypeMode::Error,
        )
        .unwrap()
    }

    #[test]
    fn repeated_unicode_marked_splices_match_full_canonical_bytes_and_digest() {
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let mut document = initial(&schema);
        let mut artifact = context.derive(&document).unwrap();
        let path = [1];
        let mut cache =
            CanonicalSpliceCache::prepare(None, &artifact, &artifact, &path, 0, 0, usize::MAX);
        assert!(cache.is_some());
        let position = document.root().child(0).unwrap().node_size() + 2;
        for (index, text) in ["🙂", "e\u{301}", "\"\\\n雪"].into_iter().enumerate() {
            let (next, _) = apply_step(
                &document,
                &Step::InsertText {
                    pos: position,
                    text: text.into(),
                    marks: vec![Mark::new("strong".into(), HashMap::new())],
                },
                &schema,
            )
            .unwrap();
            let after = CanonicalArtifact::derive_localized(
                &artifact,
                &next,
                document.root().child(1).unwrap(),
                next.root().child(1).unwrap(),
            )
            .unwrap();
            reset_canonical_artifact_counts_for_test();
            cache = CanonicalSpliceCache::prepare(
                cache,
                &artifact,
                &after,
                &path,
                index as u64,
                index as u64 + 1,
                usize::MAX,
            );
            let cached = cache.as_ref().unwrap();
            assert_eq!(
                take_canonical_artifact_counts_for_test().1,
                0,
                "A certified hit serializes only the changed text block"
            );
            let expected = crate::boundary::serialize_json_value_stack_safe(
                &crate::serialize::to_prosemirror_json(&next, &schema),
                0,
            );
            assert_eq!(cached.bytes, expected, "edit={index}");
            assert_eq!(after.sha256(), canonical_sha256(&expected), "edit={index}");
            assert_eq!(cached.range, emitted_range(&expected, &path).unwrap());
            document = next;
            artifact = after;
        }
    }

    #[test]
    fn changed_cache_authority_or_path_falls_back_to_full_bytes() {
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let document = initial(&schema);
        let before = context.derive(&document).unwrap();
        for reason in ["revision", "artifact", "schema", "path", "sibling"] {
            let cache =
                CanonicalSpliceCache::prepare(None, &before, &before, &[1], 0, 0, usize::MAX);
            let position = if matches!(reason, "path" | "sibling") {
                2
            } else {
                document.root().child(0).unwrap().node_size() + 2
            };
            let (next, _) = apply_step(
                &document,
                &Step::InsertText {
                    pos: position,
                    text: "changed".into(),
                    marks: vec![],
                },
                &schema,
            )
            .unwrap();
            let after_context = if reason == "schema" {
                CanonicalSchemaContext::new(&schema)
            } else {
                context.clone()
            };
            let after = after_context.derive(&next).unwrap();
            let current = if reason == "artifact" {
                context.derive(&document).unwrap()
            } else {
                before.clone()
            };
            let path = if reason == "path" { [0] } else { [1] };
            reset_canonical_artifact_counts_for_test();
            let cached = CanonicalSpliceCache::prepare(
                cache,
                &current,
                &after,
                &path,
                u64::from(reason == "revision"),
                2,
                usize::MAX,
            )
            .unwrap();
            assert_eq!(
                take_canonical_artifact_counts_for_test().1,
                1,
                "{reason}: full fallback is required"
            );
            let mut expected = Vec::new();
            after.write_canonical_json(&mut expected).unwrap();
            assert_eq!(cached.bytes, expected, "{reason}");
            assert_eq!(after.sha256(), canonical_sha256(&expected));
        }
    }

    #[test]
    fn cache_budget_charges_both_buffers_paths_and_weak_allocations() {
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let document = initial(&schema);
        let artifact = context.derive(&document).unwrap();
        let seed = || {
            CanonicalSpliceCache::prepare(None, &artifact, &artifact, &[1], 0, 0, usize::MAX)
                .unwrap()
        };
        let charge = seed().retained_bytes().unwrap();
        assert!(
            charge
                >= artifact.serialized_len()
                    + CanonicalSpliceCache::fixed_bytes().unwrap()
                    + std::mem::size_of::<u32>()
        );
        assert!(
            CanonicalSpliceCache::prepare(None, &artifact, &artifact, &[1], 0, 0, charge).is_some()
        );
        assert!(
            CanonicalSpliceCache::prepare(None, &artifact, &artifact, &[1], 0, 0, charge - 1)
                .is_none()
        );
        assert!(CanonicalSpliceCache::prepare(
            Some(seed()),
            &artifact,
            &artifact,
            &[1],
            0,
            1,
            charge * 2
        )
        .is_some());
        assert!(CanonicalSpliceCache::prepare(
            Some(seed()),
            &artifact,
            &artifact,
            &[1],
            0,
            1,
            charge * 2 - 1
        )
        .is_none());
        let cache = seed();
        let weak = Arc::downgrade(&artifact.0);
        drop(artifact);
        assert!(
            weak.upgrade().is_none(),
            "The cache must not keep the source document artifact alive"
        );
        assert_eq!(cache.retained_bytes(), Some(charge));
    }

    #[test]
    fn cache_ranges_reject_opaque_json_paths_and_support_deep_semantic_paths() {
        const DEPTH: usize = 256;
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let mut attrs = HashMap::new();
        attrs.insert(
            "original_json".into(),
            serde_json::json!({"type":"foreign","content":[
                {"type":"paragraph","content":[{"type":"text","text":"hidden"}]}
            ]}),
        );
        let opaque = Document::new(Node::element(
            "doc".into(),
            HashMap::new(),
            crate::model::Fragment::from(vec![Node::void("__opaque_json".into(), attrs)]),
        ));
        let artifact = context.derive(&opaque).unwrap();
        reset_canonical_artifact_counts_for_test();
        assert!(CanonicalSpliceCache::prepare(
            None,
            &artifact,
            &artifact,
            &[0, 0],
            0,
            0,
            usize::MAX
        )
        .is_none());
        assert_eq!(
            take_canonical_artifact_counts_for_test().1,
            0,
            "Opaque JSON content must be rejected before allocating or serializing a cache buffer"
        );
        let mut node = initial(&schema).root().child(0).unwrap().clone();
        for _ in 0..DEPTH {
            node = Node::element(
                "blockquote".into(),
                HashMap::new(),
                crate::model::Fragment::from(vec![node]),
            );
        }
        let document = Document::new(Node::element(
            "doc".into(),
            HashMap::new(),
            crate::model::Fragment::from(vec![node]),
        ));
        let artifact = context.derive(&document).unwrap();
        let path = vec![0; DEPTH + 1];
        let cache =
            CanonicalSpliceCache::prepare(None, &artifact, &artifact, &path, 0, 0, usize::MAX)
                .unwrap();
        let expected = crate::boundary::serialize_json_value_stack_safe(
            &crate::serialize::to_prosemirror_json(&document, &schema),
            0,
        );
        assert_eq!(cache.bytes, expected);
        assert_eq!(
            &cache.bytes[cache.range.clone()],
            br#"{"content":[{"text":"same","type":"text"}],"type":"paragraph"}"#
        );
        let (next, _) = apply_step(
            &document,
            &Step::InsertText {
                pos: DEPTH as u32 + 2,
                text: "🙂".into(),
                marks: vec![],
            },
            &schema,
        )
        .unwrap();
        let after = context.derive(&next).unwrap();
        reset_canonical_artifact_counts_for_test();
        let replacement =
            CanonicalSpliceCache::prepare(Some(cache), &artifact, &after, &path, 0, 1, usize::MAX)
                .unwrap();
        assert_eq!(
            take_canonical_artifact_counts_for_test().1,
            0,
            "Deep paths must also use a certified splice"
        );
        let mut expected = Vec::new();
        after.write_canonical_json(&mut expected).unwrap();
        assert_eq!(replacement.bytes, expected);
        assert_eq!(after.sha256(), canonical_sha256(&expected));
    }
}
