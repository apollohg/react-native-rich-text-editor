use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    InvalidStructure,
    InvalidAttributes,
    GridLimit { limit: usize, actual: usize },
    WorkLimit,
    Allocation,
}

impl fmt::Display for TableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStructure => formatter.write_str("table structure is invalid"),
            Self::InvalidAttributes => formatter.write_str("table attributes are invalid"),
            Self::GridLimit { limit, actual } => {
                write!(formatter, "table grid exceeds limit {limit}: {actual}")
            }
            Self::WorkLimit => formatter.write_str("table work budget exhausted"),
            Self::Allocation => formatter.write_str("table allocation failed"),
        }
    }
}

impl std::error::Error for TableError {}

pub(crate) fn try_resize<T: Clone>(
    values: &mut Vec<T>,
    length: usize,
    filler: T,
) -> Result<(), TableError> {
    values
        .try_reserve_exact(length.saturating_sub(values.len()))
        .map_err(|_| TableError::Allocation)?;
    values.resize(length, filler);
    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableActionKind {
    InsertTable,
    DeleteTable,
    InsertRow,
    DeleteRow,
    InsertColumn,
    DeleteColumn,
    Header,
    Merge,
    Split,
    Resize,
    MatrixPaste,
}

#[allow(dead_code)]
impl TableActionKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InsertTable => "insertTable",
            Self::DeleteTable => "deleteTable",
            Self::InsertRow => "insertRow",
            Self::DeleteRow => "deleteRow",
            Self::InsertColumn => "insertColumn",
            Self::DeleteColumn => "deleteColumn",
            Self::Header => "header",
            Self::Merge => "merge",
            Self::Split => "split",
            Self::Resize => "resize",
            Self::MatrixPaste => "matrixPaste",
        }
    }
}

#[allow(dead_code)]
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableWorkCounters {
    pub pre_normalization_passes: u32,
    pub post_normalization_passes: u32,
    pub normalization_operations: u32,
}
