#[cfg(test)]
mod lookup_entries_tests {
    use proptest::prelude::*;
    use yrs::block::ID;
    use yrs::ClientID;

    use super::*;

    const ENTRY_KEYS: u64 = 160;
    const ENTRY_CLIENT: u64 = 41;
    const ENTRY_REQUEST: u64 = 74_000;
    const MAX_OPERATION_ENTRIES: usize = 9;

    fn entry_key(index: u64) -> BranchID {
        BranchID::Nested(ID::new(
            ClientID::new(ENTRY_CLIENT),
            u32::try_from(index).unwrap(),
        ))
    }

    fn reference_replace(
        original: &HashMap<BranchID, usize>,
        before: &HashMap<BranchID, usize>,
        after: &HashMap<BranchID, usize>,
    ) -> Option<HashMap<BranchID, usize>> {
        let mut replaced = HashMap::new();
        for (key, value) in original {
            match before.get(key) {
                Some(expected) if expected != value => return None,
                Some(_) => {}
                None => {
                    replaced.insert(key.clone(), *value);
                }
            }
        }
        if before.keys().any(|key| !original.contains_key(key)) {
            return None;
        }
        for (key, value) in after {
            if replaced.insert(key.clone(), *value).is_some() {
                return None;
            }
        }
        Some(replaced)
    }

    fn reference_promote(
        original: &HashMap<BranchID, usize>,
        updates: &[(BranchID, usize, usize)],
    ) -> Option<HashMap<BranchID, usize>> {
        let mut entries = original.clone();
        for (target, old_work, new_work) in updates {
            if entries.get(target) != Some(old_work) {
                return None;
            }
            entries.insert(target.clone(), *new_work);
        }
        Some(entries)
    }

    #[derive(Debug, Clone)]
    enum EntryOperation {
        Replace {
            before: Vec<(u64, bool)>,
            after: Vec<(u64, u8)>,
        },
        Promote {
            updates: Vec<(u64, bool, u8)>,
        },
    }

    fn entry_operation() -> impl Strategy<Value = EntryOperation> {
        let key = 0..ENTRY_KEYS;
        prop_oneof![
            (
                prop::collection::vec(
                    (key.clone(), prop::bool::weighted(0.97)),
                    0..MAX_OPERATION_ENTRIES
                ),
                prop::collection::vec((key.clone(), any::<u8>()), 0..MAX_OPERATION_ENTRIES),
            )
                .prop_map(|(before, after)| EntryOperation::Replace { before, after }),
            prop::collection::vec(
                (key, prop::bool::weighted(0.97), any::<u8>()),
                1..MAX_OPERATION_ENTRIES
            )
            .prop_map(|updates| EntryOperation::Promote { updates }),
        ]
    }

    fn assert_matches_reference(
        entries: &LookupEntries,
        reference: &HashMap<BranchID, usize>,
        context: &str,
    ) -> Result<(), TestCaseError> {
        prop_assert_eq!(entries.len(), reference.len(), "{}: len", context);
        prop_assert_eq!(
            &entries.materialize().unwrap(),
            reference,
            "{}: entries",
            context
        );
        for index in 0..ENTRY_KEYS {
            let key = entry_key(index);
            prop_assert_eq!(
                entries.get(&key),
                reference.get(&key),
                "{}: key {}",
                context,
                index
            );
        }
        prop_assert!(
            entries.overrides.len() <= entries.compaction_limit(),
            "{}: {} overrides exceed the compaction limit {}",
            context,
            entries.overrides.len(),
            entries.compaction_limit()
        );
        prop_assert!(
            *entries == LookupEntries::new(reference.clone()),
            "{}: equality",
            context
        );
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(192))]

        #[test]
        fn lookup_entry_overlays_equal_full_map_replacement(
            initial in prop::collection::hash_map(0..ENTRY_KEYS, any::<u8>(), 0..ENTRY_KEYS as usize),
            operations in prop::collection::vec(entry_operation(), 1..96),
        ) {
            let mut reference: HashMap<BranchID, usize> = initial
                .into_iter()
                .map(|(key, value)| (entry_key(key), usize::from(value)))
                .collect();
            let mut entries = LookupEntries::new(reference.clone());
            for (step, operation) in operations.iter().enumerate() {
                let context = format!("step {step} {operation:?}");
                let retained = entries.materialize().unwrap();
                let operation_entries = match operation {
                    EntryOperation::Replace { before, after } => before.len() + after.len(),
                    EntryOperation::Promote { updates } => updates.len(),
                };
                let (result, expected) = match operation {
                    EntryOperation::Replace { before, after } => {
                        let before = before
                            .iter()
                            .map(|(key, matches)| {
                                let key = entry_key(*key);
                                let value = reference.get(&key).copied().unwrap_or_default();
                                (key, if *matches { value } else { value + 1 })
                            })
                            .collect::<HashMap<_, _>>();
                        let after = after
                            .iter()
                            .map(|(key, value)| (entry_key(*key), usize::from(*value)))
                            .collect::<HashMap<_, _>>();
                        (
                            entries.replaced(
                                ENTRY_REQUEST,
                                &LookupEntries::new(before.clone()),
                                &LookupEntries::new(after.clone()),
                            ),
                            reference_replace(&reference, &before, &after),
                        )
                    }
                    EntryOperation::Promote { updates } => {
                        let mut working = reference.clone();
                        let updates = updates
                            .iter()
                            .map(|(key, matches, value)| {
                                let key = entry_key(*key);
                                let old = working.get(&key).copied().unwrap_or_default();
                                let old = if *matches { old } else { old + 1 };
                                working.insert(key.clone(), usize::from(*value));
                                (key, old, usize::from(*value))
                            })
                            .collect::<Vec<_>>();
                        let promotion = MutationLookupPromotion {
                            request_id: ENTRY_REQUEST,
                            source: MutationLookupPromotionSource::ExistingFormat,
                            materialization_work_updates: updates.clone(),
                            next_pending_traversal_work: 0,
                        };
                        (entries.promoted(&promotion), reference_promote(&reference, &updates))
                    }
                };
                prop_assert_eq!(&entries.materialize().unwrap(), &retained, "{}: source mutated", context);
                match (result, expected) {
                    (Ok(next), Some(expected)) => {
                        assert_matches_reference(&next, &expected, &context)?;
                        if !Arc::ptr_eq(&next.base, &entries.base) {
                            prop_assert!(next.overrides.is_empty(), "{}: a new base is compacted", context);
                            prop_assert!(
                                entries.overrides.len() + operation_entries > entries.compaction_limit(),
                                "{}: compacted with {} overrides and {} changed entries under the limit {}",
                                context,
                                entries.overrides.len(),
                                operation_entries,
                                entries.compaction_limit()
                            );
                        }
                        entries = next;
                        reference = expected;
                    }
                    (Err(error), None) => {
                        prop_assert_eq!(error.code, "ENGINE_INVARIANT_FAILED", "{}", context);
                    }
                    (result, expected) => {
                        prop_assert!(
                            false,
                            "{}: overlay {:?} disagrees with the full-map reference {:?}",
                            context,
                            result.map(|next| next.materialize().unwrap()),
                            expected
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn overlay_compaction_reports_allocation_failure_without_changing_the_source() {
        let base = (0..ENTRY_KEYS)
            .map(|index| (entry_key(index), 1usize))
            .collect::<HashMap<_, _>>();
        let entries = LookupEntries::new(base.clone());
        let limit = entries.compaction_limit();
        let updates = (0..u64::try_from(limit).unwrap() + 1)
            .map(|index| (entry_key(index), 1usize, 2usize))
            .collect::<Vec<_>>();
        let promotion = MutationLookupPromotion {
            request_id: ENTRY_REQUEST,
            source: MutationLookupPromotionSource::ExistingFormat,
            materialization_work_updates: updates[..limit].to_vec(),
            next_pending_traversal_work: 0,
        };
        let at_limit = entries.promoted(&promotion).unwrap();
        assert!(
            Arc::ptr_eq(&at_limit.base, &entries.base) && at_limit.overrides.len() == limit,
            "{limit} overrides stay layered over the shared base"
        );
        let promotion = MutationLookupPromotion {
            materialization_work_updates: updates.clone(),
            ..promotion
        };
        set_lookup_seed_hydration_failpoint_for_test(Some(
            LookupSeedHydrationFailpoint::PromotionMapReservation,
        ));
        let refused = entries.promoted(&promotion);
        set_lookup_seed_hydration_failpoint_for_test(None);
        let error = refused.unwrap_err();
        assert_eq!(error.code, "OPERATION_RESOURCE_EXHAUSTED", "{error:?}");
        assert_eq!(
            entries.materialize().unwrap(),
            base,
            "a refused compaction keeps the source"
        );
        let compacted = entries.promoted(&promotion).unwrap();
        assert!(
            !Arc::ptr_eq(&compacted.base, &entries.base) && compacted.overrides.is_empty(),
            "one override past the limit compacts into a new base"
        );
        assert_eq!(compacted.get(&entry_key(0)), Some(&2));
        assert_eq!(compacted.get(&entry_key(ENTRY_KEYS - 1)), Some(&1));
    }
}
