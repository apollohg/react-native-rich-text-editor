use serde::Serialize;

use crate::schema::{AttrSpec, NodeSpec, Schema};
use crate::tables::types::TableError;

pub const COLSPAN_ATTR: &str = "colspan";
pub const ROWSPAN_ATTR: &str = "rowspan";
pub const COLWIDTH_ATTR: &str = "colwidth";
pub const MIN_SPAN: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableRole {
    Table,
    Row,
    Cell,
    HeaderCell,
}

impl TableRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Row => "row",
            Self::Cell => "cell",
            Self::HeaderCell => "header_cell",
        }
    }

    pub fn from_schema_name(value: &str) -> Option<Self> {
        match value {
            "table" => Some(Self::Table),
            "row" => Some(Self::Row),
            "cell" => Some(Self::Cell),
            "header_cell" => Some(Self::HeaderCell),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRoles {
    pub table: String,
    pub row: String,
    pub cell: String,
    pub header_cell: String,
}

impl TableRoles {
    pub fn resolve(schema: &Schema) -> Result<Option<Self>, TableError> {
        let mut table = None;
        let mut row = None;
        let mut cell = None;
        let mut header_cell = None;

        for node in schema.all_nodes() {
            let slot = match node.table_role {
                None => continue,
                Some(TableRole::Table) => &mut table,
                Some(TableRole::Row) => &mut row,
                Some(TableRole::Cell) => &mut cell,
                Some(TableRole::HeaderCell) => &mut header_cell,
            };

            if slot.is_some() {
                return Err(TableError::InvalidStructure);
            }

            *slot = Some(node.name.clone());
        }

        if table.is_none() && row.is_none() && cell.is_none() && header_cell.is_none() {
            return Ok(None);
        }

        let roles = match (table, row, cell, header_cell) {
            (Some(table), Some(row), Some(cell), Some(header_cell)) => Self {
                table,
                row,
                cell,
                header_cell,
            },
            _ => return Err(TableError::InvalidStructure),
        };

        roles.validate_content(schema)?;
        roles.validate_cell_attributes(schema)?;

        Ok(Some(roles))
    }

    fn validate_content(&self, schema: &Schema) -> Result<(), TableError> {
        let table = self.node(schema, &self.table)?;
        let row = self.node(schema, &self.row)?;
        let text_block = schema
            .preferred_text_block()
            .ok_or(TableError::InvalidStructure)?
            .name
            .clone();

        let accepts = |spec: &NodeSpec, children: &[&str]| {
            spec.content.matches(children, |child, symbol| {
                schema.node_matches_symbol(child, symbol)
            })
        };

        let structured = accepts(table, &[self.row.as_str()])
            && !accepts(table, &[])
            && accepts(row, &[])
            && accepts(row, &[self.cell.as_str()])
            && accepts(row, &[self.header_cell.as_str()])
            && accepts(row, &[self.cell.as_str(), self.header_cell.as_str()]);

        if !structured {
            return Err(TableError::InvalidStructure);
        }

        for name in [&self.cell, &self.header_cell] {
            let spec = self.node(schema, name)?;

            if !accepts(spec, &[text_block.as_str()]) || accepts(spec, &[]) {
                return Err(TableError::InvalidStructure);
            }
        }

        Ok(())
    }

    fn validate_cell_attributes(&self, schema: &Schema) -> Result<(), TableError> {
        for name in [&self.cell, &self.header_cell] {
            let spec = self.node(schema, name)?;

            for span in [COLSPAN_ATTR, ROWSPAN_ATTR] {
                let attr = spec.attrs.get(span).ok_or(TableError::InvalidAttributes)?;

                if !span_default_is_valid(attr) {
                    return Err(TableError::InvalidAttributes);
                }
            }

            let width = spec
                .attrs
                .get(COLWIDTH_ATTR)
                .ok_or(TableError::InvalidAttributes)?;

            if !width.has_default {
                return Err(TableError::InvalidAttributes);
            }
        }

        Ok(())
    }

    fn node<'a>(&self, schema: &'a Schema, name: &str) -> Result<&'a NodeSpec, TableError> {
        schema.node(name).ok_or(TableError::InvalidStructure)
    }
}

fn span_default_is_valid(attr: &AttrSpec) -> bool {
    attr.has_default
        && attr
            .default
            .as_ref()
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|value| value >= MIN_SPAN)
}
