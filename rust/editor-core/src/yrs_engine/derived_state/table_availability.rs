use super::DerivedStateCache;
use crate::boundary::ResourceLimits;
use crate::model::Document;
use crate::render::incremental::CachedRenderBlocks;
use crate::schema::Schema;
use crate::selection::Selection;
use crate::tables::commands::{
    cell_holds_only, default_text_block_node, node_path_starting_at, node_starting_at,
};
use crate::tables::selection::resolve_cell_rect;
use crate::yrs_engine::commands::tables::{anchor_in, CLEAR_CELLS_FIELD};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableStructureFingerprint {
    node_path: Vec<u32>,
    rows: u32,
    columns: u32,
    table_attrs_key: String,
    row_attrs_keys: Vec<String>,
    cells: Vec<TableCellStructureFingerprint>,
    retained_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableCellStructureFingerprint {
    source_row: u32,
    rowspan: u32,
    colspan: u32,
    header: bool,
    attrs_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableSelectionRect {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
    cuts_a_span: bool,
    header_selection_survives: bool,
    navigation_cell: Option<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableCommandAvailabilityKey {
    pub(crate) structure: Arc<TableStructureFingerprint>,
    pub(crate) selection: TableSelectionRect,
    pub(crate) document_scope_revision: u64,
}

#[derive(Debug, Clone)]
pub(super) struct CachedTableCommandAvailability {
    key: TableCommandAvailabilityKey,
    commands: HashMap<String, bool>,
    limits: ResourceLimits,
    retained_bytes: usize,
}

impl DerivedStateCache {
    pub(crate) fn table_command_availability(
        &self,
        document: &Document,
        schema: &Schema,
        selection: &Selection,
        limits: &ResourceLimits,
        retained_budget: usize,
        render: &CachedRenderBlocks,
        document_scope_revision: u64,
    ) -> Option<HashMap<String, bool>> {
        if self.validation_certificate.resource_limits != *limits {
            return None;
        }
        let index = &render.table_projection_index;
        let anchor = anchor_in(index, selection)?;
        let rect = resolve_cell_rect(index, anchor.anchors.anchor, anchor.anchors.head)?;
        let path = node_path_starting_at(document, rect.table_pos)?;
        let rectangle = TableSelectionRect {
            top: rect.top,
            left: rect.left,
            bottom: rect.bottom,
            right: rect.right,
            cuts_a_span: rect.cuts_a_span,
            header_selection_survives: crate::tables::commands::headers::text_selection_survives(
                selection,
                &rect,
                index.table_at(rect.table_pos)?,
            ),
            navigation_cell: match selection {
                Selection::Text { head, .. } | Selection::Cell { head, .. } => {
                    crate::tables::interchange::outer_cell_containing(index, *head)
                        .and_then(|cell| node_path_starting_at(document, cell.cell_pos))
                }
                Selection::Node { .. } | Selection::All => None,
            },
        };
        let prior = self.table_command_availability.borrow();
        let same_structure = prior.as_ref().filter(|entry| {
            entry.limits == *limits
                && entry.retained_bytes <= retained_budget
                && entry.key.document_scope_revision == document_scope_revision
                && entry.key.structure.node_path == path
        });
        let structure = if let Some(entry) = same_structure {
            Arc::clone(&entry.key.structure)
        } else {
            let mut records = Vec::new();
            render.visit_table_records(&mut records);
            let (_, table) = records
                .into_iter()
                .find(|(position, _)| *position == rect.table_pos)?;
            let mut structure = TableStructureFingerprint {
                node_path: path,
                rows: table.structure.rows,
                columns: table.structure.columns,
                table_attrs_key: table.structure.attrs_key.clone(),
                row_attrs_keys: table
                    .structure
                    .source_rows
                    .iter()
                    .map(|row| row.attrs_key.clone())
                    .collect(),
                cells: table
                    .cells
                    .iter()
                    .map(|cell| TableCellStructureFingerprint {
                        source_row: cell.source_row,
                        rowspan: cell.rowspan,
                        colspan: cell.colspan,
                        header: cell.header,
                        attrs_key: cell.attrs_key.clone(),
                    })
                    .collect(),
                retained_bytes: 0,
            };
            structure.retained_bytes = structure.measure_retained_bytes()?;
            Arc::new(structure)
        };
        let key = TableCommandAvailabilityKey {
            structure,
            selection: rectangle,
            document_scope_revision,
        };
        let mut commands = if let Some(entry) = prior.as_ref().filter(|entry| {
            entry.limits == *limits && entry.retained_bytes <= retained_budget && entry.key == key
        }) {
            entry.commands.clone()
        } else {
            #[cfg(test)]
            crate::yrs_engine::observability::record_active_applicability_pass();
            let surface = crate::yrs_engine::TableCommandSurface::resolve_in(
                document,
                schema,
                selection,
                limits,
                index.as_ref().clone(),
            );
            crate::editor_state::table_command_surface()
                .into_iter()
                .map(|(name, command)| (name.to_owned(), surface.is_available(command)))
                .collect()
        };
        drop(prior);
        let clear_available = default_text_block_node(schema).is_some_and(|block| {
            rect.cells.iter().any(|position| {
                node_starting_at(document, *position)
                    .is_some_and(|cell| !cell_holds_only(cell, &block))
            })
        });
        commands.insert(CLEAR_CELLS_FIELD.into(), clear_available);
        let retained_bytes = table_availability_retained_bytes(&key, &commands);
        if let Some(retained_bytes) =
            retained_bytes.filter(|bytes| *bytes <= retained_budget.min(limits.max_input_bytes))
        {
            *self.table_command_availability.borrow_mut() = Some(CachedTableCommandAvailability {
                key,
                commands: commands.clone(),
                limits: limits.clone(),
                retained_bytes,
            });
        } else {
            self.table_command_availability.borrow_mut().take();
        }
        Some(commands)
    }
}

impl DerivedStateCache {
    pub(crate) fn cache_render_active_state(
        &self,
        value: crate::editor_state::ActiveState,
        limits: &ResourceLimits,
        editing_limits: &crate::yrs_engine::EditingLimits,
    ) {
        if let Ok(cached) = super::CachedActiveState::try_new(value, limits, editing_limits) {
            let _ = self.render_active_state.set(CachedRenderActiveState {
                cached,
                limits: limits.clone(),
                editing_limits: editing_limits.clone(),
            });
        }
    }

    pub(crate) fn render_active_state(
        &self,
        schema: &Schema,
        limits: &ResourceLimits,
        editing_limits: &crate::yrs_engine::EditingLimits,
        document_scope_revision: u64,
    ) -> crate::editor_state::ActiveState {
        if let Some(value) = self
            .render_active_state
            .get()
            .filter(|entry| entry.limits == *limits && entry.editing_limits == *editing_limits)
            .and_then(|entry| entry.cached.clone_public(limits, editing_limits))
        {
            return value;
        }
        let commands = match self.table_command_availability(
            &self.document,
            schema,
            &self.legacy_selection,
            limits,
            editing_limits.max_derived_output_bytes,
            &self.render_blocks,
            document_scope_revision,
        ) {
            Some(commands) => crate::editor_state::command_applicability_with_cached_tables(
                &self.document,
                schema,
                &self.legacy_selection,
                limits,
                self.document_node_count,
                commands,
            ),
            None => crate::editor_state::command_applicability_with_known_node_count(
                &self.document,
                schema,
                &self.legacy_selection,
                limits,
                self.document_node_count,
            ),
        };
        let state = crate::editor_state::active_state(
            &self.document,
            schema,
            &self.legacy_selection,
            self.stored_marks.as_deref(),
            commands,
            limits,
        );
        self.cache_render_active_state(state.clone(), limits, editing_limits);
        state
    }
}

#[derive(Debug)]
pub(super) struct CachedRenderActiveState {
    cached: Arc<super::CachedActiveState>,
    limits: ResourceLimits,
    editing_limits: crate::yrs_engine::EditingLimits,
}

impl TableStructureFingerprint {
    fn measure_retained_bytes(&self) -> Option<usize> {
        let mut bytes = crate::model::arc_allocation_retained_bytes(std::mem::size_of::<Self>())?
            .checked_add(
                self.node_path
                    .capacity()
                    .checked_mul(std::mem::size_of::<u32>())?,
            )?
            .checked_add(self.table_attrs_key.capacity())?
            .checked_add(
                self.row_attrs_keys
                    .capacity()
                    .checked_mul(std::mem::size_of::<String>())?,
            )?
            .checked_add(
                self.cells
                    .capacity()
                    .checked_mul(std::mem::size_of::<TableCellStructureFingerprint>())?,
            )?;
        for key in self
            .row_attrs_keys
            .iter()
            .chain(self.cells.iter().map(|cell| &cell.attrs_key))
        {
            bytes = bytes.checked_add(key.capacity())?;
        }
        Some(bytes)
    }
}

fn table_availability_retained_bytes(
    key: &TableCommandAvailabilityKey,
    commands: &HashMap<String, bool>,
) -> Option<usize> {
    let navigation_bytes = match &key.selection.navigation_cell {
        Some(path) => path.capacity().checked_mul(std::mem::size_of::<u32>())?,
        None => 0,
    };
    let bytes = key
        .structure
        .retained_bytes
        .checked_add(navigation_bytes)?
        .checked_add(std::mem::size_of::<CachedTableCommandAvailability>())?
        .checked_add(
            commands
                .capacity()
                .checked_mul(std::mem::size_of::<(String, bool)>())?,
        )?;
    commands
        .keys()
        .try_fold(bytes, |bytes, name| bytes.checked_add(name.capacity()))
}
