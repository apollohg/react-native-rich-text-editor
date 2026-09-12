pub(crate) mod admission;
#[allow(dead_code)]
pub(crate) mod command_context;
pub(crate) mod commands;
#[allow(dead_code)]
pub(crate) mod normalize;
pub(crate) mod projection;
pub(crate) mod roles;
pub(crate) mod selection;
pub(crate) mod types;
pub(crate) mod widths;

#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod normalize_tests;
#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod widths_tests;

pub(crate) use roles::{TableRole, TableRoles};
