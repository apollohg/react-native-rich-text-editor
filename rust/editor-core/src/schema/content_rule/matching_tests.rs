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
    }
}

#[test]
fn boolean_matching_preserves_every_work_budget_boundary() {
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
    for source in [
        "",
        "a",
        "a*",
        "(a?)*",
        "(a | a b | b*)+",
        "(a? b?)*",
        "a{2,4} b?",
        "(a | b){0,3}",
    ] {
        let rule = ContentRule::parse(source).unwrap();
        for sequence in &sequences {
            assert_budget_parity(&rule, sequence);
        }
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
