pub(crate) mod admission;
pub(crate) mod projection;
pub(crate) mod roles;
pub(crate) mod selection;
pub(crate) mod types;
pub(crate) mod widths;

#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod widths_tests;

pub(crate) use roles::{TableRole, TableRoles};
