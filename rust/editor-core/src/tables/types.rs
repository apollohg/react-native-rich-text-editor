use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    InvalidStructure,
    InvalidAttributes,
    #[allow(dead_code)]
    GridLimit {
        limit: usize,
        actual: usize,
    },
    #[allow(dead_code)]
    WorkLimit,
    #[allow(dead_code)]
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
