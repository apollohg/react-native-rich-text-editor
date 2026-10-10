// This white-box harness compiles a superset of the engine sources; items
// the benchmark does not drive are expected to be unused here.
#![allow(dead_code)]
#![allow(unused_imports)]

use std::env;
use std::hint::black_box;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// The engine modules are crate-private, so this white-box harness compiles
// them directly via `#[path]` — the same files the shipped crate compiles —
// rather than through the crate API. Expectations below are derived from the
// same render/position/active-state paths the v2 render accessor uses.
#[path = "../src/boundary.rs"]
mod boundary;
#[path = "../src/clipboard.rs"]
mod clipboard;
#[path = "../src/collaboration_runtime/mod.rs"]
mod collaboration_runtime;
#[path = "command_planner_shim/command_planner.rs"]
mod command_planner;
#[path = "../src/document_api.rs"]
mod document_api;
#[path = "../src/editor_state.rs"]
mod editor_state;
#[path = "../src/ffi_v2/mod.rs"]
pub(crate) mod ffi_v2;
uniffi::setup_scaffolding!();
#[path = "../src/model/mod.rs"]
mod model;
#[path = "../src/native_transaction_bridge.rs"]
mod native_transaction_bridge;
#[path = "../src/position/mod.rs"]
mod position;
#[path = "../src/position_epoch.rs"]
mod position_epoch;
#[path = "../src/registry.rs"]
mod registry;
#[path = "../src/render/mod.rs"]
mod render;
#[path = "../src/schema/mod.rs"]
mod schema;
#[path = "../src/selection/mod.rs"]
mod selection;
#[path = "../src/serialize/mod.rs"]
mod serialize;
#[path = "../src/session.rs"]
mod session;
#[path = "../src/tables/mod.rs"]
mod tables;
#[path = "../src/transform/mod.rs"]
mod transform;
#[path = "../src/viewer/mod.rs"]
mod viewer;
#[path = "../src/yrs_engine/mod.rs"]
mod yrs_engine;

use crate::boundary::ResourceLimits;
use crate::render::RenderElement;
use crate::transform::DocumentValidator;
use crate::yrs_engine::{
    Affinity, EditorOffsetKind, HistoryPolicy, InitializationMode, RenderUpdate, ResolvedSelection,
    RevisionedPosition, SelectionInput, SelectionIntent, TransactionOrigin, TypedCommand,
    TypedTransaction, TypedTransactionResult, YrsDocumentEngine, YrsEngineConfig,
};
// `cargo bench` compiles this target with the `test` cfg, so the included
// sources' inline test modules compile as well; they reference the crate-root
// schema presets and the session-registry concurrency guard.
pub use schema::presets::{prosemirror_schema, tiptap_schema};

#[cfg(test)]
#[path = "../src/test_support/mod.rs"]
mod test_support;
#[cfg(test)]
pub(crate) use document_api::session_initialization_test_support;
#[cfg(test)]
pub(crate) use native_transaction_bridge::native_bridge_test_support;
#[cfg(test)]
pub(crate) use registry::session_lifecycle_test_support;

use serde_json::{json, Value};

#[path = "support/benchmark_filter.rs"]
mod benchmark_filter;

const EDITING_TYPING_BURST: usize = 20;
const EDITING_CURSOR_SCALAR: u32 = 44;

struct EditingCaseExpectation {
    before: Value,
    after: Value,
}

#[derive(Clone, Copy)]
enum BenchMode {
    Quick,
    Standard,
}

impl BenchMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Standard => "standard",
        }
    }

    fn profile(self) -> BenchProfile {
        match self {
            Self::Quick => BenchProfile {
                warmup_iterations: 2,
                iterations: 8,
                article_blocks: 48,
                paragraph_chars: 140,
                mapping_points: 192,
                selection_width: 64,
                typing_burst: 24,
                selection_scrub_points: 48,
                awareness_peer_count: 12,
                opaque_payload_bytes: 32 * 1024,
            },
            Self::Standard => BenchProfile {
                warmup_iterations: 4,
                iterations: 20,
                article_blocks: 160,
                paragraph_chars: 220,
                mapping_points: 768,
                selection_width: 160,
                typing_burst: 64,
                selection_scrub_points: 160,
                awareness_peer_count: 32,
                opaque_payload_bytes: 256 * 1024,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct BenchProfile {
    warmup_iterations: usize,
    iterations: usize,
    article_blocks: usize,
    paragraph_chars: usize,
    mapping_points: usize,
    selection_width: u32,
    typing_burst: usize,
    selection_scrub_points: usize,
    awareness_peer_count: usize,
    opaque_payload_bytes: usize,
}

struct BenchOptions {
    mode: BenchMode,
    json_output: bool,
    filter: Option<String>,
}

#[derive(Debug)]
struct BenchResult {
    name: &'static str,
    group: &'static str,
    iterations: usize,
    ops_per_iteration: usize,
    min_ms: f64,
    p50_ms: f64,
    mean_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    mean_us_per_op: f64,
}

macro_rules! push_case {
    (
        $results:expr,
        $options:expr,
        verified_bench_case($name:expr, $group:expr, $($arguments:tt)*),
    ) => {
        push_case_lazy($results, $options, $name, $group, || {
            verified_bench_case($name, $group, $($arguments)*)
        })
    };
    (
        $results:expr,
        $options:expr,
        bench_case($name:expr, $group:expr, $($arguments:tt)*),
    ) => {
        push_case_lazy($results, $options, $name, $group, || {
            bench_case($name, $group, $($arguments)*)
        })
    };
}

include!("perf_suite/cases.rs");

include!("perf_suite/measurement.rs");

include!("perf_suite/fixtures.rs");
