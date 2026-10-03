#[cfg(test)]
use super::canonical_sha256;
use super::{hash_prefix::Sha256Prefix, CanonicalArtifact, CanonicalArtifactInner};
use crate::model::Node;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::ops::Range;
use std::sync::{Arc, Weak};

const CANONICAL_CACHE_GROWTH_BYTES: usize = 4 * 1024;

#[cfg(test)]
std::thread_local! {
    static EXACT_CAPACITY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CACHE_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAILED_ALLOCATION: std::cell::Cell<Option<CacheAllocation>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAllocation {
    Buffer,
    SpareBuffer,
    Path,
}

#[derive(Debug)]
pub(crate) struct CanonicalSpliceCache {
    bytes: Vec<u8>,
    path: Vec<u32>,
    suffix_range: Range<usize>,
    revision: u64,
    artifact: Weak<CanonicalArtifactInner>,
    hash_prefix: Sha256Prefix,
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
    #[cfg(test)]
    pub(crate) fn with_exact_capacity_for_test<T>(operation: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                EXACT_CAPACITY.set(self.0);
            }
        }
        let _restore = Restore(EXACT_CAPACITY.replace(true));
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
        mut previous: Option<Self>,
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
            let replaced_len = expected_len.checked_sub(prior.suffix_range.end)?;
            Some((prior, node, replaced_len))
        });
        if let Some((prior, node, _)) = replacement {
            let reused_charge = previous_bytes
                .checked_add(previous_bytes)
                .and_then(|charge| {
                    charge.checked_add(expected_len.saturating_sub(prior.bytes.capacity()))
                });
            if reused_charge.is_some_and(|charge| charge <= byte_budget) {
                return previous.take()?.splice(
                    after,
                    node,
                    expected_len,
                    next_revision,
                    previous_bytes,
                    byte_budget,
                );
            }
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
        let suffix_range = if let Some((prior, node, replaced_len)) = replacement {
            bytes.extend_from_slice(prior.bytes.get(..prior.suffix_range.end)?);
            crate::serialize::json_out::write_node_json(
                &mut bytes,
                node,
                &after.0.schema_context.0.schema,
            )
            .ok()?;
            if bytes.len().checked_sub(prior.suffix_range.end)? != replaced_len {
                return None;
            }
            prior.suffix_range.clone()
        } else {
            #[cfg(test)]
            {
                super::super::observability::record_canonical_serialization();
                super::SERIALIZATION_COUNT.set(super::SERIALIZATION_COUNT.get().saturating_add(1));
            }
            after.write_canonical_json(&mut bytes).ok()?;
            let selected = emitted_range(&bytes, path)?;
            let suffix_end = bytes.len().checked_sub(selected.len())?;
            // Keep immutable bytes before the editable tail so growth never shifts the suffix.
            bytes.get_mut(selected.start..)?.rotate_left(selected.len());
            selected.start..suffix_end
        };
        if bytes.len() != expected_len {
            return None;
        }
        let hash_prefix = if let Some((prior, _, _)) = replacement {
            prior.hash_prefix.clone()
        } else {
            Sha256Prefix::new(bytes.get(..suffix_range.start)?)?
        };
        let candidate = Self {
            bytes,
            path: owned_path,
            suffix_range,
            revision: next_revision,
            artifact: Arc::downgrade(&after.0),
            hash_prefix,
        };
        candidate.finish(after, previous_bytes, byte_budget)
    }

    fn splice(
        mut self,
        after: &CanonicalArtifact,
        node: &Node,
        expected_len: usize,
        next_revision: u64,
        previous_bytes: usize,
        byte_budget: usize,
    ) -> Option<Self> {
        self.bytes.get(self.suffix_range.clone())?;
        let old_len = self.bytes.len();
        if expected_len < self.suffix_range.end {
            return None;
        }
        if expected_len > self.bytes.capacity() {
            #[cfg(test)]
            Self::allocation_allowed(CacheAllocation::Buffer)?;
            let spare_capacity = expected_len
                .checked_add(CANONICAL_CACHE_GROWTH_BYTES)
                .filter(|capacity| {
                    let charge = self
                        .retained_bytes()
                        .and_then(|bytes| bytes.checked_sub(self.bytes.capacity()))
                        .and_then(|bytes| bytes.checked_add(*capacity));
                    charge.is_some_and(|charge| {
                        previous_bytes
                            .checked_add(charge)
                            .is_some_and(|total| total <= byte_budget)
                            && charge
                                .checked_mul(2)
                                .is_some_and(|total| total <= byte_budget)
                    })
                });
            #[cfg(test)]
            let spare_capacity = spare_capacity.filter(|_| {
                !EXACT_CAPACITY.get()
                    && Self::allocation_allowed(CacheAllocation::SpareBuffer).is_some()
            });
            // Spare capacity must fit both current staging and another cache of the same size.
            let reserved_spare = spare_capacity
                .is_some_and(|capacity| self.bytes.try_reserve_exact(capacity - old_len).is_ok());
            if !reserved_spare {
                self.bytes
                    .try_reserve_exact(expected_len.checked_sub(old_len)?)
                    .ok()?;
            }
        }
        // Keep the existing old-plus-new staging headroom policy.
        if previous_bytes.checked_add(self.retained_bytes()?)? > byte_budget {
            return None;
        }
        if expected_len > old_len {
            self.bytes.resize(expected_len, 0);
        }
        self.bytes.truncate(expected_len);
        let mut output = self.bytes.get_mut(self.suffix_range.end..)?;
        crate::serialize::json_out::write_node_json(
            &mut output,
            node,
            &after.0.schema_context.0.schema,
        )
        .ok()?;
        if !output.is_empty() {
            return None;
        }
        self.revision = next_revision;
        self.artifact = Arc::downgrade(&after.0);
        self.finish(after, previous_bytes, byte_budget)
    }

    fn finish(
        self,
        after: &CanonicalArtifact,
        previous_bytes: usize,
        byte_budget: usize,
    ) -> Option<Self> {
        if self.bytes.len() != after.serialized_len()
            || previous_bytes.checked_add(self.retained_bytes()?)? > byte_budget
        {
            return None;
        }
        let digest = self.hash_prefix.finish(
            self.bytes.get(self.suffix_range.end..)?,
            self.bytes.get(self.suffix_range.clone())?,
        )?;
        #[cfg(test)]
        super::super::observability::record_canonical_hash();
        let _ = after.0.sha256.set(digest);
        debug_assert_eq!(after.0.sha256.get(), Some(&digest));
        Some(self)
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
    use crate::model::{Document, Fragment, Mark};
    use crate::schema::Schema;
    use crate::transform::{apply_step, Step};
    use crate::yrs_engine::canonical::{
        reset_canonical_artifact_counts_for_test, take_canonical_artifact_counts_for_test,
        CanonicalSchemaContext,
    };
    use std::collections::HashMap;

    fn canonical_bytes(cache: &CanonicalSpliceCache) -> Vec<u8> {
        [
            &cache.bytes[..cache.suffix_range.start],
            &cache.bytes[cache.suffix_range.end..],
            &cache.bytes[cache.suffix_range.clone()],
        ]
        .concat()
    }

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
        const BLOCKS: usize = 3;
        const DELETED_SCALARS: u32 = 1;
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        for target in 0..BLOCKS {
            let block = initial(&schema).root().child(0).unwrap().clone();
            let mut document = Document::new(Node::element(
                "doc".into(),
                HashMap::new(),
                crate::model::Fragment::from(vec![block; BLOCKS]),
            ));
            let mut artifact = context.derive(&document).unwrap();
            let path = [target as u32];
            let mut cache =
                CanonicalSpliceCache::prepare(None, &artifact, &artifact, &path, 0, 0, usize::MAX);
            assert!(cache.is_some());
            let position = (0..target)
                .map(|index| document.root().child(index).unwrap().node_size())
                .sum::<u32>()
                + 2;
            let replacement = Step::ReplaceRange {
                from: position,
                to: position + DELETED_SCALARS,
                content: crate::model::Fragment::from(vec![Node::text("X".into(), vec![])]),
            };
            let steps =
                std::iter::once(replacement)
                    .chain(["🙂", "e\u{301}", "\"\\\n雪"].into_iter().map(|text| {
                        Step::InsertText {
                            pos: position,
                            text: text.into(),
                            marks: vec![Mark::new("strong".into(), HashMap::new())],
                        }
                    }))
                    .chain(std::iter::repeat_n(
                        Step::DeleteRange {
                            from: position,
                            to: position + DELETED_SCALARS,
                        },
                        3,
                    ));
            for (index, step) in steps.enumerate() {
                let (next, _) = apply_step(&document, &step, &schema).unwrap();
                let after = CanonicalArtifact::derive_localized(
                    &artifact,
                    &next,
                    document.root().child(target).unwrap(),
                    next.root().child(target).unwrap(),
                )
                .unwrap();
                let previous = cache.as_ref().unwrap();
                let prior_buffer = previous.bytes.as_ptr();
                let prior_path = previous.path.as_ptr();
                let fits_buffer = after.serialized_len() <= previous.bytes.capacity();
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
                assert_eq!(
                    canonical_bytes(&cached),
                    expected,
                    "target={target}, edit={index}"
                );
                assert_eq!(
                    after.sha256(),
                    canonical_sha256(&expected),
                    "target={target}, edit={index}"
                );
                let selected = emitted_range(&expected, &path).unwrap();
                assert_eq!(
                    cached.suffix_range,
                    selected.start..expected.len() - selected.len()
                );
                if fits_buffer {
                    assert_eq!(cached.bytes.as_ptr(), prior_buffer,
                        "target={target}, edit={index}: a certified splice that fits must reuse its owned buffer");
                }
                assert_eq!(
                    cached.path.as_ptr(),
                    prior_path,
                    "The certified path stays owned across hits"
                );
                document = next;
                artifact = after;
            }
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
            assert_eq!(canonical_bytes(&cached), expected, "{reason}");
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
        let position = document.root().child(0).unwrap().node_size() + 2;
        let (shrunk_document, _) = apply_step(
            &document,
            &Step::DeleteRange {
                from: position,
                to: position + 1,
            },
            &schema,
        )
        .unwrap();
        let shrunk = context.derive(&shrunk_document).unwrap();
        let fresh_charge =
            CanonicalSpliceCache::prepare(None, &shrunk, &shrunk, &[1], 0, 0, usize::MAX)
                .unwrap()
                .retained_bytes()
                .unwrap();
        let exact_budget = charge + fresh_charge;
        let compact = CanonicalSpliceCache::prepare(
            Some(seed()),
            &artifact,
            &shrunk,
            &[1],
            0,
            1,
            exact_budget,
        )
        .expect("Retaining old capacity must not reject a previously affordable fresh cache");
        assert_eq!(compact.retained_bytes(), Some(fresh_charge));
        assert!(CanonicalSpliceCache::prepare(
            Some(seed()),
            &artifact,
            &shrunk,
            &[1],
            0,
            1,
            exact_budget - 1
        )
        .is_none());
        let retained = CanonicalSpliceCache::prepare(
            Some(seed()),
            &artifact,
            &shrunk,
            &[1],
            0,
            1,
            charge + charge,
        )
        .unwrap();
        assert_eq!(
            retained.retained_bytes(),
            Some(charge),
            "A shrinking reused buffer is charged for its retained capacity"
        );
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
        assert_eq!(canonical_bytes(&cache), expected);
        assert_eq!(
            &cache.bytes[cache.suffix_range.end..],
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
        assert_eq!(canonical_bytes(&replacement), expected);
        assert_eq!(after.sha256(), canonical_sha256(&expected));
    }

    fn paragraph(text: &str, depth: usize) -> Node {
        let mut node = Node::element(
            "paragraph".into(),
            Default::default(),
            Fragment::from(if text.is_empty() {
                Vec::new()
            } else {
                vec![Node::text(text.into(), Vec::new())]
            }),
        );
        for _ in 0..depth {
            node = Node::element(
                "blockquote".into(),
                Default::default(),
                Fragment::from(vec![node]),
            );
        }
        node
    }

    fn document(children: &[Node]) -> Document {
        Document::new(Node::element(
            "doc".into(),
            Default::default(),
            Fragment::from(children.to_vec()),
        ))
    }

    fn assert_canonical(cache: &CanonicalSpliceCache, artifact: &CanonicalArtifact, path: &[u32]) {
        let mut expected = Vec::new();
        artifact.write_canonical_json(&mut expected).unwrap();
        let selected = emitted_range(&expected, path).unwrap();
        let immutable_end = expected.len() - selected.len();
        assert_eq!(
            &cache.bytes[..selected.start],
            &expected[..selected.start],
            "prefix changed for {path:?}"
        );
        assert_eq!(
            &cache.bytes[selected.start..immutable_end],
            &expected[selected.end..],
            "immutable suffix must precede the editable tail for {path:?}"
        );
        assert_eq!(
            &cache.bytes[immutable_end..],
            &expected[selected.clone()],
            "editable node must occupy the tail for {path:?}"
        );
        assert_eq!(cache.bytes.len(), expected.len());
        assert_eq!(
            artifact.0.sha256.get(),
            Some(&canonical_sha256(&expected)),
            "canonical hash for {path:?}"
        );
    }

    #[test]
    fn editable_tail_preserves_canonical_bytes_hashes_and_immutable_suffix() {
        const SIBLING_BYTES: usize = 512;
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        for depth in [0, 3] {
            for target in 0..3 {
                let mut children: Vec<_> = (0..3)
                    .map(|index| {
                        paragraph(
                            &format!("sibling {index} {}", "x".repeat(SIBLING_BYTES)),
                            depth,
                        )
                    })
                    .collect();
                let mut path = vec![target as u32];
                path.extend(std::iter::repeat_n(0, depth));
                let mut before = context.derive(&document(&children)).unwrap();
                let mut cache =
                    CanonicalSpliceCache::prepare(None, &before, &before, &path, 0, 1, usize::MAX)
                        .unwrap();
                assert_canonical(&cache, &before, &path);
                let fixed_end = cache.suffix_range.end;
                let unchanged = cache.bytes[..fixed_end].to_vec();
                for (index, text) in [
                    "a",
                    "é🙂\"\\\n\u{0000}",
                    "",
                    &"g".repeat(SIBLING_BYTES * 2),
                    "shrink",
                ]
                .iter()
                .enumerate()
                {
                    children[target] = paragraph(text, depth);
                    let after = context.derive(&document(&children)).unwrap();
                    cache = CanonicalSpliceCache::prepare(
                        Some(cache),
                        &before,
                        &after,
                        &path,
                        index as u64 + 1,
                        index as u64 + 2,
                        usize::MAX,
                    )
                    .unwrap();
                    assert_canonical(&cache, &after, &path);
                    assert_eq!(
                        &cache.bytes[..fixed_end],
                        unchanged,
                        "immutable suffix moved for target={target}, depth={depth}, edit={index}"
                    );
                    before = after;
                }
            }
        }
    }

    #[test]
    fn editable_tail_fresh_replacement_and_retarget_preserve_exact_budget() {
        const SPARE_BYTES: usize = 4096;
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let mut children = vec![paragraph("initial", 0), paragraph("other", 0)];
        let before = context.derive(&document(&children)).unwrap();
        let mut cache =
            CanonicalSpliceCache::prepare(None, &before, &before, &[0], 0, 1, usize::MAX).unwrap();
        cache.bytes.reserve_exact(SPARE_BYTES);
        let previous_bytes = cache.retained_bytes().unwrap();
        children[0] = paragraph("é🙂\"\\", 0);
        let after = context.derive(&document(&children)).unwrap();
        let budget = previous_bytes
            + CanonicalSpliceCache::fixed_bytes().unwrap()
            + after.serialized_len()
            + std::mem::size_of::<u32>();
        assert!(
            previous_bytes * 2 > budget,
            "must exercise fresh allocation rather than reused spare capacity"
        );
        let cache = CanonicalSpliceCache::prepare(Some(cache), &before, &after, &[0], 1, 2, budget)
            .unwrap();
        assert_canonical(&cache, &after, &[0]);
        assert_eq!(previous_bytes + cache.retained_bytes().unwrap(), budget);
        children[1] = paragraph("new target", 0);
        let retargeted = context.derive(&document(&children)).unwrap();
        let cache =
            CanonicalSpliceCache::prepare(Some(cache), &after, &retargeted, &[1], 2, 3, usize::MAX)
                .unwrap();
        assert_canonical(&cache, &retargeted, &[1]);
    }

    #[test]
    fn repeated_splice_growth_reuses_spare_capacity_within_staging_budget() {
        const EDITS: usize = 256;
        const BYTE_BUDGET: usize = 16 * 1024;
        let schema = crate::prosemirror_schema();
        let context = CanonicalSchemaContext::new(&schema);
        let mut children = vec![paragraph("initial", 0), paragraph("unchanged", 0)];
        let mut before = context.derive(&document(&children)).unwrap();
        let mut cache =
            CanonicalSpliceCache::prepare(None, &before, &before, &[0], 0, 0, BYTE_BUDGET).unwrap();
        let mut retained = None;
        for edit in 0..EDITS {
            children[0] = paragraph(&"x".repeat(edit + 8), 0);
            let after = context.derive(&document(&children)).unwrap();
            let previous_bytes = cache.retained_bytes().unwrap();
            cache = CanonicalSpliceCache::prepare(
                Some(cache),
                &before,
                &after,
                &[0],
                edit as u64,
                edit as u64 + 1,
                BYTE_BUDGET,
            )
            .unwrap();
            assert_canonical(&cache, &after, &[0]);
            assert!(
                previous_bytes + cache.retained_bytes().unwrap() <= BYTE_BUDGET,
                "edit={edit}: actual old-plus-new staging charge"
            );
            let allocation = (cache.bytes.as_ptr(), cache.bytes.capacity());
            if let Some(previous) = retained {
                assert_eq!(
                    allocation, previous,
                    "edit={edit}: repeated typing should reuse its bounded spare allocation"
                );
            } else {
                assert!(
                    cache.bytes.capacity() >= cache.bytes.len() + EDITS,
                    "the first growing edit should reserve capacity for subsequent edits"
                );
                retained = Some(allocation);
            }
            before = after;
        }
    }
}
