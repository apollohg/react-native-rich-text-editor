#[cfg_attr(not(feature = "table-interop"), allow(dead_code))]
pub(crate) mod projection;
pub(crate) mod roles;
pub(crate) mod types;
#[cfg_attr(not(feature = "table-interop"), allow(dead_code))]
pub(crate) mod widths;

#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod widths_tests;

pub(crate) use roles::{TableRole, TableRoles};
