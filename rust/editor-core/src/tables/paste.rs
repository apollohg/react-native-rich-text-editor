use crate::boundary::ResourceLimits;
use crate::clipboard::{normalized_line_breaks, LINE_BREAK};
use crate::model::{Fragment, Node};
use crate::schema::Schema;
use crate::tables::commands::{attrs_with_removed_columns, attrs_with_row_span, fresh_cell_node};
use crate::tables::projection::span_attribute;
use crate::tables::roles::{TableRoles, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_ROWSPAN_ATTR};
use crate::tables::types::{try_resize, TableError};

const FIELD_SEPARATOR: char = '\t';
const QUOTE: char = '"';
const NEXT_CHARACTER: usize = 1;
const NEXT_CELL: usize = 1;
const ONE_LEVEL: usize = 1;
const ONLY_CHILD: usize = 1;
const FIRST_CHILD: usize = 0;
const CLOSED_DEPTH: usize = 0;
const ONE_SPAN: u32 = 1;
const UNCOVERED: u32 = 0;
const EMPTY_WIDTH: u32 = 0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MatrixCell {
    pub node: Node,
    pub colspan: u32,
    pub rowspan: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TableMatrix {
    pub width: u32,
    pub height: u32,
    pub rows: Vec<Vec<MatrixCell>>,
}

impl MatrixCell {
    fn read(node: Node) -> Result<Self, TableError> {
        Ok(Self {
            colspan: span_attribute(&node, TABLE_CELL_COLSPAN_ATTR)?,
            rowspan: span_attribute(&node, TABLE_CELL_ROWSPAN_ATTR)?,
            node,
        })
    }

    fn with_attrs(
        &self,
        attrs: std::collections::HashMap<String, serde_json::Value>,
        colspan: u32,
        rowspan: u32,
    ) -> Option<Self> {
        Some(Self {
            node: Node::element(
                self.node.node_type().to_owned(),
                attrs,
                self.node.content().cloned()?,
            ),
            colspan,
            rowspan,
        })
    }

    fn without_trailing_columns(&self, count: u32) -> Option<Self> {
        self.with_attrs(
            attrs_with_removed_columns(&self.node, self.colspan, count)?,
            self.colspan.checked_sub(count)?,
            self.rowspan,
        )
    }

    fn with_row_span(&self, rowspan: u32) -> Option<Self> {
        self.with_attrs(
            attrs_with_row_span(&self.node, rowspan),
            self.colspan,
            rowspan,
        )
    }
}

impl TableMatrix {
    pub(crate) fn cell_columns(&self) -> Option<Vec<Vec<u32>>> {
        let mut covered_until: Vec<u32> = vec![UNCOVERED; self.width as usize];
        let mut placed = Vec::with_capacity(self.rows.len());
        for (row, cells) in (0u32..).zip(self.rows.iter()) {
            let mut column = 0u32;
            let mut columns = Vec::with_capacity(cells.len());
            for cell in cells {
                while covered_until
                    .get(column as usize)
                    .is_some_and(|until| *until > row)
                {
                    column = column.checked_add(ONE_SPAN)?;
                }
                let end = column.checked_add(cell.colspan)?;
                if end > self.width {
                    return None;
                }
                let until = row.checked_add(cell.rowspan)?;
                for slot in covered_until.get_mut(column as usize..end as usize)? {
                    *slot = until;
                }
                columns.push(column);
                column = end;
            }
            placed.push(columns);
        }
        Some(placed)
    }
}

pub(crate) fn matrix_from_slice(
    content: &Fragment,
    open_start: usize,
    open_end: usize,
    roles: &TableRoles,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Option<TableMatrix>, TableError> {
    let mut content = content;
    let mut open_start = open_start;
    let mut open_end = open_end;
    while content.child_count() == ONLY_CHILD {
        let Some(only) = content.child(FIRST_CHILD) else {
            return Ok(None);
        };
        let open_on_both_sides = open_start > CLOSED_DEPTH && open_end > CLOSED_DEPTH;
        if !open_on_both_sides && only.node_type() != roles.table {
            break;
        }
        let Some(inner) = only.content() else {
            return Ok(None);
        };
        open_start = open_start.saturating_sub(ONE_LEVEL);
        open_end = open_end.saturating_sub(ONE_LEVEL);
        content = inner;
    }
    let Some(first) = content.child(FIRST_CHILD) else {
        return Ok(None);
    };
    let is_cell =
        |node: &Node| node.node_type() == roles.cell || node.node_type() == roles.header_cell;
    let rows = if first.node_type() == roles.row {
        let mut rows = Vec::with_capacity(content.child_count());
        for row in content.iter() {
            let Some(cells) = row.content().filter(|_| row.node_type() == roles.row) else {
                return Ok(None);
            };
            if !cells.iter().all(is_cell) {
                return Ok(None);
            }
            rows.push(cells.children().to_vec());
        }
        rows
    } else if is_cell(first) {
        if !content.iter().all(is_cell) {
            return Ok(None);
        }
        vec![content.children().to_vec()]
    } else {
        return Ok(None);
    };
    matrix_from_rows(rows, roles, schema, limits)
}

pub(crate) fn matrix_from_rows(
    rows: Vec<Vec<Node>>,
    roles: &TableRoles,
    schema: &Schema,
    limits: &ResourceLimits,
) -> Result<Option<TableMatrix>, TableError> {
    let mut cells = Vec::with_capacity(rows.len());
    for row in rows {
        cells.push(
            row.into_iter()
                .map(MatrixCell::read)
                .collect::<Result<Vec<_>, _>>()?,
        );
    }

    let mut widths: Vec<u32> = Vec::new();
    for (row, row_cells) in cells.iter().enumerate() {
        for cell in row_cells {
            let covered = row
                .checked_add(cell.rowspan as usize)
                .ok_or(TableError::Allocation)?;
            if covered > limits.max_table_grid_slots {
                return Err(TableError::GridLimit {
                    limit: limits.max_table_grid_slots,
                    actual: covered,
                });
            }
            if widths.len() < covered {
                try_resize(&mut widths, covered, EMPTY_WIDTH)?;
            }
            for width in widths.get_mut(row..covered).ok_or(TableError::Allocation)? {
                *width = width
                    .checked_add(cell.colspan)
                    .ok_or(TableError::Allocation)?;
            }
        }
    }
    let width = widths.iter().copied().max().unwrap_or(EMPTY_WIDTH);
    let height = widths.len().max(cells.len());
    if width == EMPTY_WIDTH {
        return Ok(None);
    }
    let slots = (width as usize)
        .checked_mul(height)
        .ok_or(TableError::Allocation)?;
    if slots > limits.max_table_grid_slots {
        return Err(TableError::GridLimit {
            limit: limits.max_table_grid_slots,
            actual: slots,
        });
    }

    let filler = fresh_cell_node(schema, &roles.cell)
        .map(MatrixCell::read)
        .transpose()?
        .ok_or(TableError::InvalidStructure)?;
    cells.resize_with(height, Vec::new);
    for (row, row_cells) in cells.iter_mut().enumerate() {
        let occupied = widths.get(row).copied().unwrap_or(EMPTY_WIDTH);
        for _ in occupied..width {
            row_cells.push(filler.clone());
        }
    }
    Ok(Some(TableMatrix {
        width,
        height: u32::try_from(height).map_err(|_| TableError::Allocation)?,
        rows: cells,
    }))
}

pub(crate) fn clip_matrix(matrix: &TableMatrix, width: u32, height: u32) -> Option<TableMatrix> {
    let mut rows = matrix.rows.clone();
    if matrix.width != width {
        let mut added: Vec<u32> = vec![UNCOVERED; rows.len()];
        let mut clipped_rows = Vec::with_capacity(rows.len());
        for (row, source) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut column = added.get(row).copied()?;
            let mut index = 0usize;
            while column < width {
                let repeated = source.get(index.checked_rem(source.len())?)?;
                let overhang = column.checked_add(repeated.colspan)?.saturating_sub(width);
                let cell = if overhang > EMPTY_WIDTH {
                    repeated.without_trailing_columns(overhang)?
                } else {
                    repeated.clone()
                };
                column = column.checked_add(cell.colspan)?;
                for below in ONE_SPAN..cell.rowspan {
                    if let Some(slot) = added.get_mut(row.checked_add(below as usize)?) {
                        *slot = slot.checked_add(cell.colspan)?;
                    }
                }
                cells.push(cell);
                index = index.checked_add(NEXT_CELL)?;
            }
            clipped_rows.push(cells);
        }
        rows = clipped_rows;
    }
    if matrix.height != height {
        let source_height = rows.len();
        let mut repeated_rows = Vec::with_capacity(height as usize);
        for row in 0..height {
            let source = rows.get((row as usize).checked_rem(source_height)?)?;
            let mut cells = Vec::with_capacity(source.len());
            for cell in source {
                let fits = row.checked_add(cell.rowspan)? <= height;
                cells.push(if fits {
                    cell.clone()
                } else {
                    cell.with_row_span(height.checked_sub(row)?)?
                });
            }
            repeated_rows.push(cells);
        }
        rows = repeated_rows;
    }
    Some(TableMatrix {
        width,
        height,
        rows,
    })
}

pub(crate) fn tab_separated_fields(text: &str) -> Option<Vec<Vec<String>>> {
    let normalized = normalized_line_breaks(text);
    if !normalized.contains(FIELD_SEPARATOR) {
        return None;
    }
    let body = normalized
        .strip_suffix(LINE_BREAK)
        .unwrap_or(normalized.as_str());
    let characters: Vec<char> = body.chars().collect();
    let mut rows = Vec::new();
    let mut fields = Vec::new();
    let mut start = 0usize;
    loop {
        let (field, end) = match characters.get(start) {
            Some(&QUOTE) => quoted_field(&characters, start)
                .unwrap_or_else(|| literal_field(&characters, start)),
            Some(_) | None => literal_field(&characters, start),
        };
        fields.push(field);
        match characters.get(end) {
            Some(&FIELD_SEPARATOR) => {}
            Some(_) => rows.push(std::mem::take(&mut fields)),
            None => {
                rows.push(fields);
                return Some(rows);
            }
        }
        start = end.checked_add(NEXT_CHARACTER)?;
    }
}

fn ends_field(character: Option<&char>) -> bool {
    matches!(character, None | Some(&FIELD_SEPARATOR) | Some(&LINE_BREAK))
}

fn literal_field(characters: &[char], start: usize) -> (String, usize) {
    let mut end = start;
    while !ends_field(characters.get(end)) {
        end = end.saturating_add(NEXT_CHARACTER);
    }
    (
        characters
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .collect(),
        end,
    )
}

fn quoted_field(characters: &[char], start: usize) -> Option<(String, usize)> {
    let mut field = String::new();
    let mut cursor = start.checked_add(NEXT_CHARACTER)?;
    loop {
        let character = *characters.get(cursor)?;
        let next = cursor.checked_add(NEXT_CHARACTER)?;
        if character != QUOTE {
            field.push(character);
            cursor = next;
            continue;
        }
        if characters.get(next) == Some(&QUOTE) {
            field.push(QUOTE);
            cursor = next.checked_add(NEXT_CHARACTER)?;
            continue;
        }
        return ends_field(characters.get(next)).then_some((field, next));
    }
}
