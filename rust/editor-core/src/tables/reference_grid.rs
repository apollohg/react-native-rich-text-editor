use crate::command_planner::default_attrs;
use crate::model::{Fragment, Node};
use crate::schema::Schema;
use crate::tables::projection::{
    column_width, span_attribute, CellRect, ProjectedTable, SyntheticRegion, TableGridBudget,
};
use crate::tables::roles::{
    TableRoles, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_COLWIDTH_ATTR, TABLE_CELL_ROWSPAN_ATTR,
};
use crate::tables::types::{try_resize, TableError};
use crate::tables::widths::ColumnWidthResolver;

pub(crate) enum ReferenceFailure {
    Unsafe(TableError),
    Unsupported(&'static str),
}

impl From<TableError> for ReferenceFailure {
    fn from(error: TableError) -> Self {
        Self::Unsafe(error)
    }
}

type Result<T> = std::result::Result<T, ReferenceFailure>;

#[derive(Clone)]
struct VirtualCell {
    source: Option<usize>,
    node: Node,
    colspan: u32,
    rowspan: u32,
    widths: Vec<u32>,
}

impl VirtualCell {
    fn new(node: &Node, source: Option<usize>, budget: &mut TableGridBudget) -> Result<Self> {
        let colspan = span_attribute(node, TABLE_CELL_COLSPAN_ATTR)?;
        budget.spend(colspan as usize)?;
        let mut widths = Vec::new();
        try_resize(&mut widths, colspan as usize, 0)?;
        for (offset, width) in widths.iter_mut().enumerate() {
            *width = column_width(node, offset as u32)?;
        }
        Ok(Self {
            source,
            node: node.clone(),
            colspan,
            rowspan: span_attribute(node, TABLE_CELL_ROWSPAN_ATTR)?,
            widths,
        })
    }

    fn effective_node(&self) -> Node {
        let mut attrs = self.node.attrs().clone();
        attrs.insert(TABLE_CELL_COLSPAN_ATTR.into(), self.colspan.into());
        attrs.insert(TABLE_CELL_ROWSPAN_ATTR.into(), self.rowspan.into());
        attrs.insert(
            TABLE_CELL_COLWIDTH_ATTR.into(),
            if self.widths.iter().any(|width| *width != 0) {
                serde_json::json!(self.widths)
            } else {
                serde_json::Value::Null
            },
        );
        Node::element(
            self.node.node_type().into(),
            attrs,
            self.node.content().cloned().unwrap_or_else(Fragment::empty),
        )
    }
}

enum Problem {
    Collision { cell: usize, row: usize, count: u32 },
    Missing { row: usize, count: u32 },
    Overlong { cell: usize, rowspan: u32 },
    Width { cell: usize, widths: Vec<u32> },
}

struct ReferenceMap {
    columns: usize,
    slots: Vec<Option<usize>>,
    widths: Vec<Option<u32>>,
    problems: Vec<Problem>,
}

fn checked_add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(TableError::Allocation.into())
}

fn extent(rows: usize, columns: usize) -> Result<usize> {
    rows.checked_mul(columns)
        .ok_or(TableError::Allocation.into())
}

fn admit_virtual(budget: &TableGridBudget, slots: usize, raw_charge: usize) -> Result<()> {
    if budget.admits_virtual(slots, raw_charge) {
        Ok(())
    } else {
        Err(ReferenceFailure::Unsupported("virtual-grid-limit"))
    }
}

fn compute_map(
    rows: &[Vec<usize>],
    cells: &[VirtualCell],
    budget: &mut TableGridBudget,
    raw_charge: usize,
) -> Result<ReferenceMap> {
    let height = rows.len();
    let mut row_widths = Vec::new();
    try_resize(&mut row_widths, height, 0usize)?;
    for (row, indices) in rows.iter().enumerate() {
        budget.spend(1)?;
        for &index in indices {
            let cell = &cells[index];
            if cell.colspan == 0 || cell.rowspan == 0 {
                return Err(ReferenceFailure::Unsupported(
                    "zero-span-after-reference-pass",
                ));
            }
            let end = checked_add(row, cell.rowspan as usize)?.min(height);
            for width in &mut row_widths[row..end] {
                budget.spend(1)?;
                *width = checked_add(*width, cell.colspan as usize)?;
            }
        }
    }
    let columns = row_widths.into_iter().max().unwrap_or(0);
    if columns == 0 || height == 0 {
        return Err(ReferenceFailure::Unsupported("empty-reference-surface"));
    }
    let size = extent(height, columns)?;
    admit_virtual(budget, size, raw_charge)?;
    let mut slots = Vec::new();
    try_resize(&mut slots, size, None)?;
    let mut widths = ColumnWidthResolver::new();
    let mut problems = Vec::new();
    let mut cursor = 0usize;
    for (row, indices) in rows.iter().enumerate() {
        for index in indices
            .iter()
            .copied()
            .map(Some)
            .chain(std::iter::once(None))
        {
            while cursor < size && slots[cursor].is_some() {
                budget.spend(1)?;
                cursor += 1;
            }
            let Some(index) = index else { break };
            let cell = &cells[index];
            for h in 0..cell.rowspan as usize {
                if checked_add(row, h)? >= height {
                    problems.push(Problem::Overlong {
                        cell: index,
                        rowspan: h as u32,
                    });
                    break;
                }
                let start = checked_add(cursor, extent(h, columns)?)?;
                for w in 0..cell.colspan as usize {
                    budget.spend(1)?;
                    let slot = checked_add(start, w)?;
                    match slots.get_mut(slot) {
                        Some(owner @ None) => *owner = Some(index),
                        _ => problems.push(Problem::Collision {
                            cell: index,
                            row,
                            count: cell.colspan - w as u32,
                        }),
                    }
                    widths.contribute_at(slot % columns, cell.widths[w])?;
                }
            }
            cursor = checked_add(cursor, cell.colspan as usize)?;
        }
        let expected = extent(row + 1, columns)?;
        let mut missing = 0u32;
        while cursor < expected {
            budget.spend(1)?;
            if slots[cursor].is_none() {
                missing = missing.checked_add(1).ok_or(TableError::Allocation)?;
            }
            cursor += 1;
        }
        if missing != 0 {
            problems.push(Problem::Missing {
                row,
                count: missing,
            });
        }
    }
    let bad_widths = widths.has_unconfirmed(height);
    let widths = widths.finish(columns)?;
    if bad_widths {
        let mut seen = Vec::new();
        try_resize(&mut seen, cells.len(), false)?;
        let mut width_problems = Vec::new();
        for (slot, owner) in slots.iter().enumerate() {
            budget.spend(1)?;
            let Some(index) = *owner else { continue };
            if seen[index] {
                continue;
            }
            seen[index] = true;
            let cell = &cells[index];
            let mut updated = None;
            for w in 0..cell.colspan as usize {
                budget.spend(1)?;
                if let Some(width) = widths[(slot + w) % columns] {
                    if cell.widths[w] != width {
                        updated.get_or_insert_with(|| cell.widths.clone())[w] = width;
                    }
                }
            }
            if let Some(widths) = updated {
                width_problems.push(Problem::Width {
                    cell: index,
                    widths,
                });
            }
        }
        width_problems.reverse();
        width_problems.extend(problems);
        problems = width_problems;
    }
    Ok(ReferenceMap {
        columns,
        slots,
        widths,
        problems,
    })
}

pub(crate) fn project_reference(
    table: &Node,
    schema: &Schema,
    raw: &ProjectedTable,
    budget: &mut TableGridBudget,
    raw_charge: usize,
) -> Result<ProjectedTable> {
    let roles = TableRoles::resolve(schema)?.ok_or(TableError::InvalidStructure)?;
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    for row in table
        .content()
        .into_iter()
        .flat_map(|content| content.iter())
    {
        if row.node_type() != roles.row {
            return Err(ReferenceFailure::Unsupported("unsupported-row-role"));
        }
        let mut indices = Vec::new();
        for cell in row.content().into_iter().flat_map(|content| content.iter()) {
            if cell.node_type() != roles.cell && cell.node_type() != roles.header_cell {
                return Err(ReferenceFailure::Unsupported("unsupported-cell-role"));
            }
            let index = cells.len();
            cells.push(VirtualCell::new(cell, Some(index), budget)?);
            indices.push(index);
        }
        rows.push(indices);
    }
    let original_count = cells.len();
    if original_count != raw.cells.len() {
        return Err(ReferenceFailure::Unsupported("ambiguous-source-map"));
    }
    let map = compute_map(&rows, &cells, budget, raw_charge)?;
    let originals = cells.clone();
    let mut additions = Vec::new();
    try_resize(&mut additions, rows.len(), 0usize)?;
    let raw_irregular = raw.irregular || !map.problems.is_empty();
    for problem in map.problems {
        budget.spend(1)?;
        match problem {
            Problem::Collision { cell, row, count } => {
                let original = &originals[cell];
                let end = checked_add(row, original.rowspan as usize)?.min(rows.len());
                for missing in &mut additions[row..end] {
                    budget.spend(1)?;
                    *missing = checked_add(*missing, count as usize)?;
                }
                // Each pinned setNodeMarkup replacement starts from raw attributes.
                cells[cell] = original.clone();
                cells[cell].colspan = original.colspan - count;
                let colspan = cells[cell].colspan as usize;
                cells[cell].widths.truncate(colspan);
            }
            Problem::Missing { row, count } => {
                additions[row] = checked_add(additions[row], count as usize)?;
            }
            Problem::Overlong { cell, rowspan } => {
                cells[cell] = originals[cell].clone();
                cells[cell].rowspan = rowspan;
            }
            Problem::Width { cell, widths } => {
                cells[cell] = originals[cell].clone();
                cells[cell].widths = widths;
            }
        }
    }
    let first = additions.iter().position(|count| *count != 0);
    let last = additions.iter().rposition(|count| *count != 0);
    for (row, &count) in additions.iter().enumerate() {
        if count == 0 {
            continue;
        }
        admit_virtual(budget, checked_add(cells.len(), count)?, raw_charge)?;
        budget.spend(count)?;
        let row_node = table.child(row).ok_or(TableError::InvalidStructure)?;
        let node = filler_cell(row_node, schema)
            .ok_or(ReferenceFailure::Unsupported("unsupported-gap-default"))?;
        let cell = VirtualCell::new(&node, None, budget)?;
        let mut inserted = Vec::new();
        inserted
            .try_reserve_exact(count)
            .map_err(|_| TableError::Allocation)?;
        for _ in 0..count {
            inserted.push(cells.len());
            cells.push(cell.clone());
        }
        let at_start = (row == 0 || row.checked_sub(1) == first) && last == Some(row);
        if at_start {
            inserted.append(&mut rows[row]);
            rows[row] = inserted;
        } else {
            rows[row].extend(inserted);
        }
    }
    let mapped = compute_map(&rows, &cells, budget, raw_charge)?;
    materialize(schema, raw, raw_irregular, cells, mapped, budget)
}

fn materialize(
    schema: &Schema,
    raw: &ProjectedTable,
    irregular: bool,
    cells: Vec<VirtualCell>,
    mapped: ReferenceMap,
    budget: &mut TableGridBudget,
) -> Result<ProjectedTable> {
    let mut projected = raw.clone();
    projected.columns = u32::try_from(mapped.columns).map_err(|_| TableError::Allocation)?;
    projected.widths = mapped.widths;
    projected.irregular = irregular;
    projected.slots.clear();
    try_resize(&mut projected.slots, mapped.slots.len(), None)?;
    let mut anchors = Vec::new();
    try_resize(&mut anchors, cells.len(), None)?;
    let mut covered = Vec::new();
    try_resize(&mut covered, cells.len(), 0usize)?;
    for (slot, owner) in mapped.slots.iter().enumerate() {
        budget.spend(1)?;
        if let Some(index) = owner {
            anchors[*index].get_or_insert(slot);
            covered[*index] += 1;
        }
    }
    for (index, cell) in cells.iter().enumerate() {
        if covered[index] != extent(cell.rowspan as usize, cell.colspan as usize)? {
            return Err(ReferenceFailure::Unsupported("overlapping-reference-cells"));
        }
        let start =
            anchors[index].ok_or(ReferenceFailure::Unsupported("unmapped-reference-cell"))?;
        let row = start / mapped.columns;
        let column = start % mapped.columns;
        if checked_add(row, cell.rowspan as usize)? > projected.rows as usize
            || checked_add(column, cell.colspan as usize)? > mapped.columns
        {
            return Err(ReferenceFailure::Unsupported(
                "nonrectangular-reference-cell",
            ));
        }
        for r in row..row + cell.rowspan as usize {
            for c in column..column + cell.colspan as usize {
                budget.spend(1)?;
                let slot = r * mapped.columns + c;
                if mapped.slots[slot] != Some(index) {
                    return Err(ReferenceFailure::Unsupported("overlapping-reference-cells"));
                }
                projected.slots[slot] = cell.source;
            }
        }
        let rect = CellRect {
            row: row as u32,
            column: column as u32,
            rowspan: cell.rowspan,
            colspan: cell.colspan,
        };
        if let Some(source) = cell.source {
            projected.cells[source].rect = rect;
        } else {
            projected.synthetic.push(SyntheticRegion {
                rect,
                node: cell.effective_node(),
            });
        }
    }
    let roles = TableRoles::resolve(schema)?.ok_or(TableError::InvalidStructure)?;
    for (slot, owner) in mapped.slots.iter().enumerate() {
        if owner.is_some() {
            continue;
        }
        budget.spend(1)?;
        projected.synthetic.push(SyntheticRegion {
            rect: CellRect {
                row: (slot / mapped.columns) as u32,
                column: (slot % mapped.columns) as u32,
                rowspan: 1,
                colspan: 1,
            },
            node: default_cell(&roles.cell, schema)
                .ok_or(ReferenceFailure::Unsupported("unsupported-gap-default"))?,
        });
    }
    Ok(projected)
}

fn default_cell(cell_type: &str, schema: &Schema) -> Option<Node> {
    let block = crate::tables::commands::default_text_block_node(schema)?;
    Some(Node::element(
        cell_type.into(),
        default_attrs(schema, cell_type)?,
        Fragment::from(vec![block]),
    ))
}

pub(crate) fn filler_cell(row: &Node, schema: &Schema) -> Option<Node> {
    let roles = TableRoles::resolve(schema).ok()??;
    let cell_type = row
        .content()
        .and_then(|content| {
            content.iter().find(|child| {
                child.node_type() == roles.cell || child.node_type() == roles.header_cell
            })
        })
        .map(Node::node_type)
        .unwrap_or(&roles.cell);
    default_cell(cell_type, schema)
}
