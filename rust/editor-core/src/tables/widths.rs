#[derive(Debug, Default)]
pub(crate) struct ColumnWidthResolver {
    candidates: Vec<Option<(u32, usize)>>,
}

impl ColumnWidthResolver {
    pub(crate) fn new() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    pub(crate) fn contribute_at(&mut self, column: usize, width: u32) {
        if self.candidates.len() <= column {
            self.candidates.resize(column.saturating_add(1), None);
        }
        if let Some(candidate) = self.candidates.get_mut(column) {
            contribute(candidate, width);
        }
    }

    pub(crate) fn finish(mut self, columns: usize) -> Vec<Option<u32>> {
        self.candidates.resize(columns, None);
        self.candidates
            .into_iter()
            .map(|candidate| candidate.map(|(value, _)| value))
            .collect()
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
