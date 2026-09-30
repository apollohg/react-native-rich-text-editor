use super::*;
use crate::command_planner::{apply_operations, apply_operations_mapped};
use crate::tables::commands_tests::{
    cell_openings, document_of, engine_schema, header_fixture, regular_fixture, seeded,
    tall_span_fixture, wide_span_fixture, TABLE_POSITION,
};
use crate::tables::normalize_tests::{cell, limits, row, table};
use serde_json::json;

#[derive(Clone, Copy, Debug)]
enum Axis {
    Row,
    Column,
}

fn eager_operations(
    document: &Document,
    target: &TableTarget<'_>,
    schema: &Schema,
    limits: &ResourceLimits,
    axis: Axis,
) -> Option<Vec<SemanticOperation>> {
    let rect = target.rect()?;
    let (range, extent, minimum) = match axis {
        Axis::Row => (rect.top..rect.bottom, target.rows(), MINIMUM_SURVIVING_ROWS),
        Axis::Column => (
            rect.left..rect.right,
            target.columns(),
            MINIMUM_SURVIVING_COLUMNS,
        ),
    };
    if extent.checked_sub(range.end.checked_sub(range.start)?)? < minimum {
        return None;
    }
    let mut candidate = document.clone();
    let mut operations = Vec::new();
    for position in range.rev() {
        let stage = TableTarget::resolve(
            &candidate,
            target.table_pos(),
            None,
            schema,
            limits,
            GridRequirement::Regular,
        )?;
        let step = match axis {
            Axis::Row => rows::plan_delete_one_row(&stage, position)?,
            Axis::Column => columns::plan_delete_one_column(&stage, position)?,
        };
        candidate = apply_operations(&candidate, schema, &step).ok()?;
        operations.extend(step);
    }
    Some(operations)
}

#[test]
fn deletion_reuse_matches_eager_stages_for_every_fixture_rectangle() {
    const MULTI_SIDE: usize = 4;
    let nested = json!({"type":"table_cell", "content":[table(vec![row(vec![cell("nested")])])]});
    let fixtures = [
        ("regular", regular_fixture()),
        ("rowspan", tall_span_fixture()),
        ("colspan widths", wide_span_fixture()),
        ("headers", header_fixture()),
        (
            "nested",
            vec![table(vec![
                row(vec![cell("before"), nested, cell("after")]),
                row(vec![cell("a"), cell("b"), cell("c")]),
            ])],
        ),
        (
            "multiple removals",
            vec![table(
                (0..MULTI_SIDE)
                    .map(|r| row((0..MULTI_SIDE).map(|c| cell(&format!("{r}:{c}"))).collect()))
                    .collect(),
            )],
        ),
    ];
    let mut multiple_stages = false;
    let mut refusals = false;
    for (name, fixture) in fixtures {
        let engine = seeded(fixture);
        let document = document_of(&engine);
        let schema = engine_schema(&engine);
        let limits = limits();
        let openings = cell_openings(&engine);
        for &anchor in &openings {
            for &head in &openings {
                let Some(target) = TableTarget::resolve(
                    document,
                    TABLE_POSITION,
                    Some(CellAnchorPair { anchor, head }),
                    &schema,
                    &limits,
                    GridRequirement::Regular,
                ) else {
                    continue;
                };
                for axis in [Axis::Row, Axis::Column] {
                    let eager = eager_operations(document, &target, &schema, &limits, axis);
                    crate::yrs_engine::observability::reset_full_pass_counts_for_test();
                    let actual = match axis {
                        Axis::Row => rows::plan_delete_rows(document, &target, &schema, &limits),
                        Axis::Column => {
                            columns::plan_delete_columns(document, &target, &schema, &limits)
                        }
                    };
                    let projections =
                        crate::yrs_engine::observability::take_full_pass_counts_for_test()
                            .table_projection_derivations;
                    assert_eq!(
                        actual.as_ref().map(|plan| &plan.operations),
                        eager.as_ref(),
                        "{name} {axis:?} {anchor}->{head}: exact staged operations"
                    );
                    let Some(actual) = actual else {
                        refusals = true;
                        continue;
                    };
                    assert!(matches!(
                        actual.selection_after,
                        TableSelectionAfter::Mapped
                    ));
                    let rect = target.rect().unwrap();
                    let removed = match axis {
                        Axis::Row => rect.bottom - rect.top,
                        Axis::Column => rect.right - rect.left,
                    };
                    assert_eq!(projections, (removed - ONE_SLOT) as usize, "{name} {axis:?} {anchor}->{head}: every changed candidate needs a new projection");
                    multiple_stages |= removed > ONE_SLOT;
                    let (actual_document, actual_map) =
                        apply_operations_mapped(document, &schema, &actual.operations).unwrap();
                    let (eager_document, eager_map) =
                        apply_operations_mapped(document, &schema, &eager.unwrap()).unwrap();
                    assert_eq!(
                        actual_document, eager_document,
                        "{name} {axis:?}: final document"
                    );
                    let selection = Selection::cell(anchor, head);
                    assert_eq!(
                        selection.map(&actual_map),
                        selection.map(&eager_map),
                        "{name} {axis:?}: mapped selection"
                    );
                }
            }
        }
    }
    assert!(
        multiple_stages,
        "the corpus must exercise changed-candidate resolution"
    );
    assert!(
        refusals,
        "the corpus must exercise minimum-survivor refusal"
    );
}
