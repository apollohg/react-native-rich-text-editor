//! Compile crate-private engine sources directly for white-box fuzzing.
#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../../src/boundary.rs"]
pub mod boundary;
#[path = "../../src/collaboration_runtime/mod.rs"]
pub mod collaboration_runtime;
#[path = "command_planner_shim/command_planner.rs"]
pub(crate) mod command_planner;
#[path = "../../src/document_api.rs"]
pub mod document_api;
#[path = "../../src/editor_state.rs"]
pub mod editor_state;
#[path = "../../src/ffi_v2/types.rs"]
pub(crate) mod ffi_v2_types;
pub(crate) mod ffi_v2 {
    pub(crate) use super::ffi_v2_types as types;
}
uniffi::setup_scaffolding!();
#[path = "../../src/model/mod.rs"]
pub mod model;
#[path = "../../src/native_transaction_bridge.rs"]
pub mod native_transaction_bridge;
#[path = "../../src/position/mod.rs"]
pub mod position;
#[path = "../../src/position_epoch.rs"]
pub(crate) mod position_epoch;
#[path = "../../src/registry.rs"]
pub mod registry;
#[path = "../../src/render/mod.rs"]
pub mod render;
#[path = "../../src/schema/mod.rs"]
pub mod schema;
#[path = "../../src/selection/mod.rs"]
pub mod selection;
#[path = "../../src/serialize/mod.rs"]
pub mod serialize;
#[path = "../../src/session.rs"]
pub(crate) mod session;
#[path = "../../src/transform/mod.rs"]
pub mod transform;
#[path = "../../src/viewer/types.rs"]
pub(crate) mod viewer;
#[path = "../../src/yrs_engine/mod.rs"]
pub mod yrs_engine;

pub use schema::presets::{prosemirror_schema, tiptap_schema};
