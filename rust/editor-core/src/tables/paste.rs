use crate::boundary::ResourceLimits;
use crate::clipboard::{
    node_text, normalized_line_breaks, CLOSED_FRAGMENT_DEPTH, LINE_BREAK, LINE_BREAK_TEXT,
};
use crate::model::{Fragment, Node};
use crate::schema::Schema;
use crate::tables::commands::{
    attrs_with_removed_columns, attrs_with_row_span, fresh_cell_node, ONE_SLOT,
};
use crate::tables::projection::span_attribute;
use crate::tables::roles::{TableRoles, TABLE_CELL_COLSPAN_ATTR, TABLE_CELL_ROWSPAN_ATTR};
use crate::tables::types::{try_resize, TableError};

const FIELD_SEPARATOR: char = '\t';
const FIELD_SEPARATOR_TEXT: &str = "\t";
const QUOTE: char = '"';
const ESCAPED_QUOTE: &str = "\"\"";
const QUOTE_TEXT: &str = "\"";
const ONE_DEPTH: usize = ONE_SLOT as usize;
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
    ) -> Result<Self, TableError> {
        Ok(Self {
            node: Node::element(
                self.node.node_type().to_owned(),
                attrs,
                self.node
                    .content()
                    .cloned()
                    .ok_or(TableError::InvalidStructure)?,
            ),
            colspan,
            rowspan,
        })
    }

    fn without_trailing_columns(&self, count: u32) -> Result<Self, TableError> {
        self.with_attrs(
            attrs_with_removed_columns(&self.node, self.colspan, count)
                .ok_or(TableError::InvalidAttributes)?,
            self.colspan
                .checked_sub(count)
                .ok_or(TableError::InvalidAttributes)?,
            self.rowspan,
        )
    }

    fn with_row_span(&self, rowspan: u32) -> Result<Self, TableError> {
        self.with_attrs(
            attrs_with_row_span(&self.node, rowspan),
            self.colspan,
            rowspan,
        )
    }

    fn spans(&self) -> (u32, u32) {
        (self.colspan, self.rowspan)
    }
}

fn place_spans(rows: &[Vec<(u32, u32)>]) -> Result<(Vec<Vec<u32>>, u32), TableError> {
    let mut covered_until: Vec<u32> = Vec::new();
    let mut width = EMPTY_WIDTH;
    let mut placed = Vec::with_capacity(rows.len());
    for (row, spans) in (0u32..).zip(rows) {
        let mut column = EMPTY_WIDTH;
        let mut columns = Vec::with_capacity(spans.len());
        for (colspan, rowspan) in spans {
            while covered_until
                .get(column as usize)
                .is_some_and(|until| *until > row)
            {
                column = column.checked_add(ONE_SLOT).ok_or(TableError::Allocation)?;
            }
            let end = column.checked_add(*colspan).ok_or(TableError::Allocation)?;
            if covered_until.len() < end as usize {
                try_resize(&mut covered_until, end as usize, UNCOVERED)?;
            }
            let until = row.checked_add(*rowspan).ok_or(TableError::Allocation)?;
            for slot in covered_until
                .get_mut(column as usize..end as usize)
                .ok_or(TableError::Allocation)?
            {
                *slot = until;
            }
            columns.push(column);
            width = width.max(end);
            column = end;
        }
        placed.push(columns);
    }
    Ok((placed, width))
}

impl TableMatrix {
    pub(crate) fn cell_columns(&self) -> Result<Vec<Vec<u32>>, TableError> {
        let spans: Vec<Vec<(u32, u32)>> = self
            .rows
            .iter()
            .map(|cells| cells.iter().map(MatrixCell::spans).collect())
            .collect();
        let (columns, width) = place_spans(&spans)?;
        if width > self.width {
            return Err(TableError::InvalidStructure);
        }
        Ok(columns)
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
    while let [only] = content.children() {
        let open_on_both_sides =
            open_start > CLOSED_FRAGMENT_DEPTH && open_end > CLOSED_FRAGMENT_DEPTH;
        if !open_on_both_sides && only.node_type() != roles.table {
            break;
        }
        let Some(inner) = only.content() else {
            return Ok(None);
        };
        open_start = open_start.saturating_sub(ONE_DEPTH);
        open_end = open_end.saturating_sub(ONE_DEPTH);
        content = inner;
    }
    let Some(first) = content.children().first() else {
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

pub(crate) fn clip_matrix(
    matrix: &TableMatrix,
    width: u32,
    height: u32,
) -> Result<TableMatrix, TableError> {
    let mut rows = matrix.rows.clone();
    if matrix.width != width {
        let mut added: Vec<u32> = vec![UNCOVERED; rows.len()];
        let mut clipped_rows = Vec::with_capacity(rows.len());
        for (row, source) in rows.iter().enumerate() {
            let mut repeated = source.iter().cycle();
            let mut cells = Vec::new();
            let mut column = added.get(row).copied().ok_or(TableError::Allocation)?;
            while column < width {
                let next = repeated.next().ok_or(TableError::InvalidStructure)?;
                let overhang = column
                    .checked_add(next.colspan)
                    .ok_or(TableError::Allocation)?
                    .saturating_sub(width);
                let cell = if overhang > EMPTY_WIDTH {
                    next.without_trailing_columns(overhang)?
                } else {
                    next.clone()
                };
                column = column
                    .checked_add(cell.colspan)
                    .ok_or(TableError::Allocation)?;
                for below in ONE_SLOT..cell.rowspan {
                    let covered = row
                        .checked_add(below as usize)
                        .ok_or(TableError::Allocation)?;
                    if let Some(slot) = added.get_mut(covered) {
                        *slot = slot
                            .checked_add(cell.colspan)
                            .ok_or(TableError::Allocation)?;
                    }
                }
                cells.push(cell);
            }
            clipped_rows.push(cells);
        }
        rows = clipped_rows;
    }
    if matrix.height != height {
        if rows.is_empty() {
            return Err(TableError::InvalidStructure);
        }
        let mut repeated_rows = Vec::with_capacity(height as usize);
        for (row, source) in (0..height).zip(rows.iter().cycle()) {
            let mut cells = Vec::with_capacity(source.len());
            for cell in source {
                let fits = row
                    .checked_add(cell.rowspan)
                    .ok_or(TableError::Allocation)?
                    <= height;
                cells.push(if fits {
                    cell.clone()
                } else {
                    cell.with_row_span(height.checked_sub(row).ok_or(TableError::Allocation)?)?
                });
            }
            repeated_rows.push(cells);
        }
        rows = repeated_rows;
    }
    Ok(TableMatrix {
        width,
        height,
        rows,
    })
}

pub(crate) fn tab_separated_text(table: &Node, schema: &Schema) -> Result<String, TableError> {
    let rows: Vec<&Node> = table
        .content()
        .ok_or(TableError::InvalidStructure)?
        .iter()
        .collect();
    let mut spans = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut row_spans = Vec::with_capacity(row.child_count());
        for cell in row.content().ok_or(TableError::InvalidStructure)?.iter() {
            row_spans.push((
                span_attribute(cell, TABLE_CELL_COLSPAN_ATTR)?,
                span_attribute(cell, TABLE_CELL_ROWSPAN_ATTR)?,
            ));
        }
        spans.push(row_spans);
    }
    let (columns, width) = place_spans(&spans)?;
    let mut lines = Vec::with_capacity(rows.len());
    for (row, row_columns) in rows.iter().zip(&columns) {
        let mut fields = vec![String::new(); width as usize];
        let cells = row.content().ok_or(TableError::InvalidStructure)?.iter();
        for (cell, column) in cells.zip(row_columns) {
            let field = fields
                .get_mut(*column as usize)
                .ok_or(TableError::InvalidStructure)?;
            *field = spreadsheet_field(&node_text(cell, schema));
        }
        lines.push(fields.join(FIELD_SEPARATOR_TEXT));
    }
    Ok(lines.join(LINE_BREAK_TEXT))
}

fn spreadsheet_field(text: &str) -> String {
    if !text.contains([FIELD_SEPARATOR, LINE_BREAK, QUOTE]) {
        return text.to_owned();
    }
    format!(
        "{QUOTE_TEXT}{}{QUOTE_TEXT}",
        text.replace(QUOTE_TEXT, ESCAPED_QUOTE)
    )
}

pub(crate) fn tab_separated_fields(text: &str) -> Option<Vec<Vec<String>>> {
    let normalized = normalized_line_breaks(text);
    if !normalized.contains(FIELD_SEPARATOR) {
        return None;
    }
    let body = normalized
        .strip_suffix(LINE_BREAK)
        .unwrap_or(normalized.as_str());
    let mut characters = body.chars().peekable();
    let mut rows = Vec::new();
    let mut fields = Vec::new();
    loop {
        let field = if characters.peek() == Some(&QUOTE) {
            let mut attempt = characters.clone();
            match quoted_field(&mut attempt) {
                Some(field) => {
                    characters = attempt;
                    field
                }
                None => literal_field(&mut characters),
            }
        } else {
            literal_field(&mut characters)
        };
        fields.push(field);
        match characters.next() {
            Some(FIELD_SEPARATOR) => {}
            Some(_) => rows.push(std::mem::take(&mut fields)),
            None => {
                rows.push(fields);
                return Some(rows);
            }
        }
    }
}

type Characters<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn ends_field(character: &char) -> bool {
    *character == FIELD_SEPARATOR || *character == LINE_BREAK
}

fn literal_field(characters: &mut Characters<'_>) -> String {
    let mut field = String::new();
    while let Some(character) = characters.next_if(|character| !ends_field(character)) {
        field.push(character);
    }
    field
}

fn quoted_field(characters: &mut Characters<'_>) -> Option<String> {
    characters.next_if_eq(&QUOTE)?;
    let mut field = String::new();
    loop {
        let character = characters.next()?;
        if character != QUOTE {
            field.push(character);
            continue;
        }
        if characters.next_if_eq(&QUOTE).is_some() {
            field.push(QUOTE);
            continue;
        }
        return characters.peek().is_none_or(ends_field).then_some(field);
    }
}
