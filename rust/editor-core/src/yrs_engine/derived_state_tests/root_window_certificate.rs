#[derive(Debug, Clone)]
enum CertifiedEdit {
    Split(u32),
    Delete(u32, u32),
}

fn certified_edit_strategy() -> impl proptest::strategy::Strategy<Value = CertifiedEdit> {
    use proptest::prelude::*;
    const MAX_DELETED_POSITIONS: u32 = 7;
    prop_oneof![
        any::<u32>().prop_map(CertifiedEdit::Split),
        (any::<u32>(), 1..MAX_DELETED_POSITIONS)
            .prop_map(|(from, len)| CertifiedEdit::Delete(from, len)),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    #[test]
    fn a_root_window_certificate_accepts_exactly_the_candidates_full_validation_accepts(
        source in crate::test_support::document_strategy::document_strategy(),
        edits in proptest::collection::vec(certified_edit_strategy(), 1..6),
        node_slack in 0usize..6,
        depth_slack in 0usize..2,
    ) {
        let schema = crate::schema::presets::tiptap_table_schema();
        let document = from_prosemirror_json(&source, &schema, UnknownTypeMode::Preserve).unwrap();
        let base_stats =
            DocumentValidator::validate(&document, &schema, &ResourceLimits::default()).unwrap();
        let limits = ResourceLimits {
            max_document_nodes: base_stats.node_count + node_slack,
            max_document_depth: base_stats.max_depth + depth_slack,
            ..ResourceLimits::default()
        };
        let context = super::super::canonical::CanonicalSchemaContext::new(&schema);
        let fingerprint = context.schema_fingerprint().to_owned();
        let certificate = DocumentValidationCertificate::mint(
            &document,
            &context.derive(&document).unwrap(),
            &schema,
            &limits,
            &fingerprint,
            0,
            0,
            0,
        )
        .expect("the generated base document is valid");
        let mut candidate = document.clone();
        for (index, edit) in edits.into_iter().enumerate() {
            let bound = candidate.content_size() + 1;
            let step = match edit {
                CertifiedEdit::Split(position) => crate::transform::Step::SplitBlock {
                    pos: position % bound,
                    node_type: "paragraph".into(),
                    attrs: std::collections::HashMap::new(),
                },
                CertifiedEdit::Delete(from, len) => {
                    let from = from % bound;
                    crate::transform::Step::DeleteRange {
                        from,
                        to: (from + len).min(bound - 1).max(from),
                    }
                }
            };
            let Ok((next, _)) = crate::transform::apply_step(&candidate, &step, &schema) else {
                continue;
            };
            candidate = next;
            let certified =
                certificate.certifies_root_window(&document, &candidate, &schema, &limits);
            let full = DocumentValidator::validate(&candidate, &schema, &limits);
            let marks = crate::transform::validate_canonical_marks(&candidate, &schema);
            let context_label = format!(
                "edit {index} {step:?} with limits nodes {} depth {} on {}",
                limits.max_document_nodes,
                limits.max_document_depth,
                crate::serialize::to_prosemirror_json(&candidate, &schema)
            );
            proptest::prop_assert_eq!(
                certified,
                full.is_ok() && marks.is_ok(),
                "{}: certification must agree with full validation ({:?})",
                context_label,
                full.err()
            );
        }
    }
}
