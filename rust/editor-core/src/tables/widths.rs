use crate::tables::types::{try_resize, TableError};

#[derive(Debug)]
pub(crate) struct ColumnWidthResolver {
    candidates: Vec<Option<(u32, usize)>>,
}

impl ColumnWidthResolver {
    pub(crate) fn new() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    pub(crate) fn contribute_at(&mut self, column: usize, width: u32) -> Result<(), TableError> {
        if self.candidates.len() <= column {
            try_resize(&mut self.candidates, column.saturating_add(1), None)?;
        }
        if let Some(candidate) = self.candidates.get_mut(column) {
            contribute(candidate, width);
        }
        Ok(())
    }

    pub(crate) fn finish(mut self, columns: usize) -> Result<Vec<Option<u32>>, TableError> {
        try_resize(&mut self.candidates, columns, None)?;
        Ok(self
            .candidates
            .into_iter()
            .map(|candidate| candidate.map(|(value, _)| value))
            .collect())
    }
}

#[rustfmt::skip]
fn contribute(candidate: &mut Option<(u32, usize)>, width: u32) {
    if width == 0 { return; }
    match candidate {
        None => *candidate = Some((width, 1)),
        Some((value, count)) if *value == width => *count += 1,
        Some((_, count)) if *count == 1 => *candidate = Some((width, 1)),
        Some(_) => {}
    }
}
