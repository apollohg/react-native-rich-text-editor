use std::fmt;

#[allow(dead_code)]
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
