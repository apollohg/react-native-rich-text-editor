use crate::tables::widths::ColumnWidthResolver;

const FIRST_COLUMN: usize = 0;
const SECOND_COLUMN: usize = 1;
const UNSET_WIDTH: u32 = 0;

fn resolve_single_column(contributions: &[u32]) -> Option<u32> {
    let mut resolver = ColumnWidthResolver::new();
    for width in contributions {
        resolver.contribute_at(FIRST_COLUMN, *width);
    }
    resolver
        .finish(1)
        .first()
        .copied()
        .expect("one column resolves to one width")
}

#[test]
fn the_first_nonzero_contribution_becomes_the_candidate() {
    assert_eq!(resolve_single_column(&[100]), Some(100));
}

#[test]
fn a_candidate_counted_once_is_replaced_by_a_different_width() {
    assert_eq!(
        resolve_single_column(&[100, 140]),
        Some(140),
        "prosemirror-tables 1.8.5 replaces a candidate whose count is still one"
    );
}

#[test]
fn a_candidate_counted_twice_survives_a_different_width() {
    assert_eq!(
        resolve_single_column(&[100, 100, 140]),
        Some(100),
        "prosemirror-tables 1.8.5 refuses to replace a candidate once its count exceeds one"
    );
}

#[test]
fn a_confirmed_candidate_survives_any_number_of_later_disagreements() {
    assert_eq!(resolve_single_column(&[100, 100, 140, 140, 180]), Some(100));
}

#[test]
fn repeated_replacements_keep_the_last_disagreeing_width() {
    assert_eq!(resolve_single_column(&[100, 140, 180]), Some(180));
}

#[test]
fn zero_widths_never_contribute() {
    assert_eq!(resolve_single_column(&[UNSET_WIDTH]), None);
    assert_eq!(
        resolve_single_column(&[UNSET_WIDTH, 140, UNSET_WIDTH]),
        Some(140)
    );
    assert_eq!(
        resolve_single_column(&[100, UNSET_WIDTH, 140]),
        Some(140),
        "an unset width must not confirm the candidate it sits between"
    );
}

#[test]
fn columns_without_contributions_resolve_to_no_width() {
    let mut resolver = ColumnWidthResolver::new();
    resolver.contribute_at(SECOND_COLUMN, 120);

    assert_eq!(resolver.finish(3), vec![None, Some(120), None]);
}

#[test]
fn columns_accumulate_independently() {
    let mut resolver = ColumnWidthResolver::new();
    for (column, width) in [
        (FIRST_COLUMN, 100),
        (SECOND_COLUMN, 100),
        (FIRST_COLUMN, 100),
        (SECOND_COLUMN, 140),
        (FIRST_COLUMN, 140),
        (SECOND_COLUMN, 180),
    ] {
        resolver.contribute_at(column, width);
    }

    assert_eq!(resolver.finish(2), vec![Some(100), Some(180)]);
}

#[test]
fn finishing_narrows_to_the_projected_column_count() {
    let mut resolver = ColumnWidthResolver::new();
    resolver.contribute_at(FIRST_COLUMN, 100);
    resolver.contribute_at(SECOND_COLUMN, 140);

    assert_eq!(resolver.finish(1), vec![Some(100)]);
}
