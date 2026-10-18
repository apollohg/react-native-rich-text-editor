use super::{ContentRule, WorkBudget};
use std::collections::HashSet;

fn hash_set_match(rule: &ContentRule, children: &[&str], budget: &WorkBudget) -> Result<bool, ()> {
    fn closure(
        rule: &ContentRule,
        seeds: impl IntoIterator<Item = usize>,
        budget: &WorkBudget,
    ) -> Result<HashSet<usize>, ()> {
        let mut visited = HashSet::new();
        let mut pending: Vec<_> = seeds.into_iter().collect();
        while let Some(state) = pending.pop() {
            if !budget.consume() {
                return Err(());
            }
            if visited.insert(state) {
                pending.extend(rule.states[state].epsilon.iter().copied());
            }
        }
        Ok(visited)
    }
    let mut current = closure(rule, [rule.start], budget)?;
    for child in children {
        let mut next = HashSet::new();
        for state in current {
            if !budget.consume() {
                return Err(());
            }
            for (symbol, target) in &rule.states[state].transitions {
                if !budget.consume() {
                    return Err(());
                }
                if *child == symbol {
                    next.insert(*target);
                }
            }
        }
        if next.is_empty() {
            return Ok(false);
        }
        current = closure(rule, next, budget)?;
    }
    Ok(current.contains(&rule.accept))
}

fn assert_budget_parity(rule: &ContentRule, children: &[&str]) {
    let ceiling = super::DEFAULT_RUNTIME_WORK_LIMIT;
    let budget = WorkBudget::new(ceiling);
    let expected = hash_set_match(rule, children, &budget);
    assert!(
        expected.is_ok(),
        "Oracle ceiling insufficient for {:?}: {children:?}",
        rule.source()
    );
    let completed_work = budget.consumed(ceiling);
    for limit in 0..=completed_work + 1 {
        let reference_budget = WorkBudget::new(limit);
        let actual_budget = WorkBudget::new(limit);
        let reference = hash_set_match(rule, children, &reference_budget);
        let actual =
            rule.matches_with_budget(children, |child, symbol| *child == symbol, &actual_budget);
        assert_eq!(
            actual,
            reference,
            "rule={:?} states={} children={children:?} limit={limit}",
            rule.source(),
            rule.states.len()
        );
        assert_eq!(
            actual_budget.consumed(limit),
            reference_budget.consumed(limit),
            "Work charge changed: rule={:?} children={children:?} limit={limit}",
            rule.source()
        );
        let keyed_budget = WorkBudget::new(limit);
        let keyed = rule.matches_keyed_with_budget(
            children,
            |child| *child,
            |child, symbol| *child == symbol,
            &keyed_budget,
        );
        assert_eq!(
            (keyed, keyed_budget.consumed(limit)),
            (reference, reference_budget.consumed(limit)),
            "keyed matching changed the result or work charge: rule={:?} children={children:?} limit={limit}",
            rule.source()
        );
    }
}

fn short_sequences() -> Vec<Vec<&'static str>> {
    let mut sequences = vec![vec![]];
    const MAX_SEQUENCE_LENGTH: usize = 3;
    for length in 1..=MAX_SEQUENCE_LENGTH {
        for prefix in sequences
            .clone()
            .into_iter()
            .filter(|prefix| prefix.len() + 1 == length)
        {
            for symbol in ["a", "b", "x"] {
                let mut sequence = prefix.clone();
                sequence.push(symbol);
                sequences.push(sequence);
            }
        }
    }
    sequences
}

const MATCHING_RULES: [&str; 8] = [
    "",
    "a",
    "a*",
    "(a?)*",
    "(a | a b | b*)+",
    "(a? b?)*",
    "a{2,4} b?",
    "(a | b){0,3}",
];

#[test]
fn boolean_matching_preserves_every_work_budget_boundary() {
    let sequences = short_sequences();
    for source in MATCHING_RULES {
        let rule = ContentRule::parse(source).unwrap();
        for sequence in &sequences {
            assert_budget_parity(&rule, sequence);
        }
    }
}

fn assert_insertion_symbols(rule: &ContentRule, prefix: &[&str], suffix: &[&str]) {
    let symbols = rule
        .insertion_symbols_with_budget(
            prefix,
            suffix,
            |child, symbol| *child == symbol,
            &WorkBudget::new(super::DEFAULT_RUNTIME_WORK_LIMIT),
        )
        .unwrap();
    for candidate in ["a", "b", "x"] {
        let full = prefix
            .iter()
            .copied()
            .chain(std::iter::once(candidate))
            .chain(suffix.iter().copied())
            .collect::<Vec<_>>();
        let expected = hash_set_match(
            rule,
            &full,
            &WorkBudget::new(super::DEFAULT_RUNTIME_WORK_LIMIT),
        )
        .unwrap();
        assert_eq!(
            symbols.contains(&candidate),
            expected,
            "rule={:?} states={} prefix={prefix:?} candidate={candidate} suffix={suffix:?}",
            rule.source(),
            rule.states.len()
        );
    }
    assert!(
        rule.insertion_symbols_with_budget(
            prefix,
            suffix,
            |child, symbol| *child == symbol,
            &WorkBudget::new(0)
        )
        .is_err(),
        "an exhausted budget must stop the shared insertion proof"
    );
}

#[test]
fn insertion_symbols_match_complete_sequence_oracle_for_ambiguous_paths_and_suffixes() {
    let sequences = short_sequences();
    for source in MATCHING_RULES {
        let rule = ContentRule::parse(source).unwrap();
        for prefix in &sequences {
            for suffix in &sequences {
                assert_insertion_symbols(&rule, prefix, suffix);
            }
        }
    }
    for (source, prefix, suffix) in [
        ("a{29} b", vec!["a"; 28], vec!["b"]),
        ("a{31}", vec!["a"; 15], vec!["a"; 15]),
        ("a{30} b", vec!["a"; 29], vec!["b"]),
    ] {
        assert_insertion_symbols(&ContentRule::parse(source).unwrap(), &prefix, &suffix);
    }
}

#[test]
fn boolean_matching_preserves_state_capacity_boundary() {
    for (source, states, mut children) in [
        ("a{29} b", 63, vec!["a"; 29]),
        ("a{31}", 64, vec!["a"; 31]),
        ("a{30} b", 65, vec!["a"; 30]),
    ] {
        if source.ends_with('b') {
            children.push("b");
        }
        let rule = ContentRule::parse(source).unwrap();
        assert_eq!(
            rule.states.len(),
            states,
            "Fixture must straddle the compact state-set boundary"
        );
        assert_budget_parity(&rule, &children);
        *children.last_mut().unwrap() = "x";
        assert_budget_parity(&rule, &children);
    }
}

#[test]
fn repeated_epsilon_edges_preserve_charges_when_pending_storage_spills() {
    let mut rule = ContentRule::parse("").unwrap();
    const REPEATED_EDGES: usize = 128;
    rule.states[rule.start].epsilon = vec![rule.accept; REPEATED_EDGES];
    assert_budget_parity(&rule, &[]);
    assert_budget_parity(&rule, &["x"]);
}

const INSERTION_ALPHABET: [&str; 4] = ["a", "b", "c", "x"];
const INSERTION_GROUP: &str = "g";
const INSERTION_GROUP_MEMBERS: [&str; 2] = ["a", "b"];
const MAX_RANDOM_CHILDREN: usize = 12;
const CORPUS_ROOT_REPEATS: usize = 24;

fn grouped_symbol_matches(child: &&str, symbol: &str) -> bool {
    *child == symbol || (symbol == INSERTION_GROUP && INSERTION_GROUP_MEMBERS.contains(child))
}

fn assert_compact_insertion_matches_reference<T: PartialEq + std::fmt::Debug>(
    rule: &ContentRule,
    prefix: &[T],
    suffix: &[T],
    symbol_matches: impl Fn(&T, &str) -> bool,
    context: &str,
) {
    let reference = rule.insertion_symbols_with_states::<HashSet<usize>, _, _>(
        prefix,
        suffix,
        &symbol_matches,
        &WorkBudget::new(super::DEFAULT_RUNTIME_WORK_LIMIT),
    );
    let compact = rule.insertion_symbols_with_budget(
        prefix,
        suffix,
        &symbol_matches,
        &WorkBudget::new(super::DEFAULT_RUNTIME_WORK_LIMIT),
    );
    assert!(
        reference.is_ok(),
        "{context}: reference exhausted its budget for rule={:?}",
        rule.source()
    );
    assert_eq!(
        compact,
        reference,
        "{context}: rule={:?} states={} prefix={prefix:?} suffix={suffix:?}",
        rule.source(),
        rule.states.len()
    );
}

fn random_rule_source() -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::prelude::*;
    let leaf = prop::sample::select(vec!["a", "b", "c", INSERTION_GROUP]).prop_map(str::to_owned);
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..4).prop_map(|parts| parts.join(" ")),
            prop::collection::vec(inner.clone(), 2..4)
                .prop_map(|parts| format!("({})", parts.join(" | "))),
            (
                inner,
                prop::sample::select(vec!["*", "+", "?", "{2}", "{0,2}", "{1,3}"])
            )
                .prop_map(|(part, repeat)| format!("({part}){repeat}")),
        ]
    })
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

    #[test]
    fn compact_insertion_symbols_equal_the_reference_for_random_rules_and_splits(
        source in random_rule_source(),
        prefix in proptest::collection::vec(proptest::sample::select(INSERTION_ALPHABET.to_vec()), 0..MAX_RANDOM_CHILDREN),
        suffix in proptest::collection::vec(proptest::sample::select(INSERTION_ALPHABET.to_vec()), 0..MAX_RANDOM_CHILDREN),
    ) {
        let rule = ContentRule::parse(&source).unwrap();
        proptest::prop_assume!(rule.states.len() <= super::COMPACT_MATCH_STATES);
        assert_compact_insertion_matches_reference(
            &rule,
            &prefix,
            &suffix,
            grouped_symbol_matches,
            "random",
        );
    }
}

fn corpus_document() -> serde_json::Value {
    use serde_json::json;
    let paragraph =
        |text: &str| json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] });
    let mut root = Vec::new();
    for repeat in 0..CORPUS_ROOT_REPEATS {
        root.push(paragraph(&format!("paragraph {repeat}")));
        root.push(json!({ "type": "heading", "attrs": { "level": 2 }, "content": [{ "type": "text", "text": "heading" }] }));
        root.push(json!({ "type": "bullet_list", "content": [
            { "type": "list_item", "content": [
                paragraph("item"),
                { "type": "bullet_list", "content": [{ "type": "list_item", "content": [paragraph("nested")] }] },
                { "type": "blockquote", "content": [paragraph("quoted item")] }
            ] },
            { "type": "list_item", "content": [paragraph("second")] }
        ] }));
        root.push(
            json!({ "type": "ordered_list", "attrs": { "order": 1 }, "content": [
            { "type": "list_item", "content": [paragraph("ordered")] }
        ] }),
        );
        root.push(json!({ "type": "blockquote", "content": [paragraph("quote"), paragraph("quote two")] }));
        root.push(json!({ "type": "horizontal_rule" }));
        root.push(json!({ "type": "paragraph", "content": [
            { "type": "text", "text": "before" }, { "type": "hard_break" }, { "type": "text", "text": "after" }
        ] }));
        root.push(json!({ "type": "paragraph" }));
    }
    let tables = crate::test_support::large_table_fixture::two_table_document();
    root.extend(tables["content"].as_array().unwrap().iter().cloned());
    json!({ "type": "doc", "content": root })
}

fn assert_every_parent_split_matches_reference(
    node: &crate::model::Node,
    schema: &crate::schema::Schema,
    compared: &mut usize,
) {
    let Some(content) = node.content() else {
        return;
    };
    let spec = schema.node(node.node_type()).unwrap();
    let children = content
        .iter()
        .map(crate::model::Node::node_type)
        .collect::<Vec<_>>();
    for split in 0..=children.len() {
        assert_compact_insertion_matches_reference(
            &spec.content,
            &children[..split],
            &children[split..],
            |child: &&str, symbol: &str| schema.node_matches_symbol(child, symbol),
            &format!("{} split {split} of {}", node.node_type(), children.len()),
        );
        *compared += 1;
    }
    for child in content.iter() {
        assert_every_parent_split_matches_reference(child, schema, compared);
    }
}

#[test]
fn compact_insertion_symbols_equal_the_reference_at_every_split_of_the_schema_corpus() {
    for document in [
        corpus_document(),
        crate::test_support::large_table_fixture::two_table_document(),
        crate::test_support::large_table_fixture::multi_paragraph_cell_document(),
    ] {
        let session = crate::test_support::large_table_fixture::session_with_document(&document);
        let mut compared = 0;
        assert_every_parent_split_matches_reference(
            session.engine.document().unwrap().root(),
            session.engine.schema(),
            &mut compared,
        );
        assert!(compared > 0, "the corpus document must contain parents");
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

    #[test]
    fn keyed_compact_matching_preserves_results_and_work_charges_for_random_rules(
        source in random_rule_source(),
        children in proptest::collection::vec(proptest::sample::select(INSERTION_ALPHABET.to_vec()), 0..MAX_RANDOM_CHILDREN * 3),
        limit_fraction in 0.0f64..1.2,
    ) {
        let rule = ContentRule::parse(&source).unwrap();
        proptest::prop_assume!(rule.states.len() <= super::COMPACT_MATCH_STATES);
        let full = WorkBudget::new(super::DEFAULT_RUNTIME_WORK_LIMIT);
        let reference_result = rule.matches_with_states::<HashSet<usize>, _, _>(
            &children,
            rule.start,
            grouped_symbol_matches,
            &full,
        );
        let limit = (full.consumed(super::DEFAULT_RUNTIME_WORK_LIMIT) as f64 * limit_fraction) as usize;
        let reference_budget = WorkBudget::new(limit);
        let reference = rule.matches_with_states::<HashSet<usize>, _, _>(
            &children,
            rule.start,
            grouped_symbol_matches,
            &reference_budget,
        );
        let keyed_budget = WorkBudget::new(limit);
        let keyed = rule.matches_keyed_with_budget(
            &children,
            |child| *child,
            grouped_symbol_matches,
            &keyed_budget,
        );
        proptest::prop_assert!(reference_result.is_ok());
        proptest::prop_assert_eq!(
            (keyed, keyed_budget.consumed(limit)),
            (reference, reference_budget.consumed(limit)),
            "rule={:?} states={} children={:?} limit={}",
            rule.source(),
            rule.states.len(),
            children,
            limit
        );
    }
}
