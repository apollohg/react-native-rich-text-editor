#[cfg(target_os = "android")]
use editor_core::ffi_v2::editor::{
    editor_v2_create, editor_v2_destroy, editor_v2_replace_document,
};
#[cfg(target_os = "android")]
use editor_core::ffi_v2::render::editor_v2_render_update;
#[cfg(target_os = "android")]
use std::sync::mpsc::channel;

#[cfg(target_os = "android")]
const PARAGRAPH_COUNT: usize = 1536;
#[cfg(target_os = "android")]
const MINIMUM_LOADED_BYTES: usize = 16 * 1024 * 1024;
#[cfg(target_os = "android")]
const REPLACE_REQUEST_ID: u64 = 1;
#[cfg(target_os = "android")]
const SCHEMA_NAME: &str = "Tiptap-compatible camelCase schema";
#[cfg(target_os = "android")]
const SMAPS_PATH: &str = "/proc/self/smaps";
#[cfg(target_os = "android")]
const ALLOCATOR_MAPPING_NAME: &str = "[anon:mimalloc]";
#[cfg(target_os = "android")]
const RESIDENT_FIELD: &str = "Rss:";
#[cfg(target_os = "android")]
const BYTES_PER_KIB: usize = 1024;
#[cfg(target_os = "android")]
const MAXIMUM_RETAINED_PERCENT: usize = 2;
#[cfg(target_os = "android")]
const PERCENT: usize = 100;

#[cfg(target_os = "android")]
fn resident_bytes() -> usize {
    let smaps = std::fs::read_to_string(SMAPS_PATH).unwrap();
    let mut in_allocator_mapping = false;
    let mut resident_kib = 0;
    for line in smaps.lines() {
        let range = line.split_whitespace().next().unwrap_or_default();
        if range.contains('-') && range.chars().all(|c| c == '-' || c.is_ascii_hexdigit()) {
            in_allocator_mapping = line.ends_with(ALLOCATOR_MAPPING_NAME);
        } else if in_allocator_mapping {
            if let Some(value) = line.strip_prefix(RESIDENT_FIELD) {
                resident_kib += value
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<usize>()
                    .unwrap();
            }
        }
    }
    resident_kib * BYTES_PER_KIB
}

#[cfg(target_os = "android")]
fn large_document_html() -> String {
    let paragraphs: String = (0..PARAGRAPH_COUNT)
        .map(|index| {
            format!(
                "<p>Paragraph {index:04}: Native editing preserves each word, its formatting, \
                 and the caret while the document grows. <b>bold emphasis</b>, \
                 <i>italic detail</i>, <a href=\"https://example.com/release/{index}\">linked \
                 reference</a>. Unicode: café, 東京, 😀.</p>"
            )
        })
        .collect();
    format!("<html><body>{paragraphs}</body></html>")
}

#[cfg(target_os = "android")]
fn schema() -> serde_json::Value {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/schema-fingerprints.json"
    )))
    .unwrap();
    fixtures["fingerprints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["name"] == SCHEMA_NAME)
        .unwrap()["schema"]
        .clone()
}

#[cfg(target_os = "android")]
fn main() {
    let html = large_document_html();
    let config = serde_json::json!({
        "schema": schema(),
        "initialization": { "type": "localEmpty" },
    })
    .to_string();
    let request = serde_json::json!({
        "version": 1,
        "requestId": REPLACE_REQUEST_ID.to_string(),
        "setHtml": html,
        "history": "resetAndClear",
    })
    .to_string();
    let baseline = resident_bytes();
    let created = editor_v2_create(config, None);
    let created: serde_json::Value =
        serde_json::from_str(created.value.as_deref().unwrap_or_else(|| {
            panic!("create failed: {:?}", created.error);
        }))
        .unwrap();
    let editor_id = created["editorId"].as_str().unwrap().to_string();
    let replaced = editor_v2_replace_document(editor_id.clone(), request);
    assert!(
        replaced.error.is_none(),
        "replace failed: {:?}",
        replaced.error
    );
    let (render_sender, render_requests) = channel::<String>();
    let (rendered_sender, rendered) = channel::<()>();
    let (exit_sender, exit) = channel::<()>();
    let render_thread = std::thread::spawn(move || {
        let editor_id = render_requests.recv().unwrap();
        let frame = editor_v2_render_update(editor_id, None, None);
        assert!(frame.error.is_none(), "render failed: {:?}", frame.error);
        drop(frame);
        rendered_sender.send(()).unwrap();
        exit.recv().unwrap();
    });
    render_sender.send(editor_id.clone()).unwrap();
    rendered.recv().unwrap();
    let loaded = resident_bytes();
    let destroyed = editor_v2_destroy(editor_id);
    assert!(
        destroyed.error.is_none(),
        "destroy failed: {:?}",
        destroyed.error
    );
    let released = resident_bytes();
    let loaded_growth = loaded.saturating_sub(baseline);
    let retained_growth = released.saturating_sub(baseline);
    eprintln!(
        "document release: paragraphs={PARAGRAPH_COUNT} baseline={baseline} loaded={loaded} \
         released={released} loaded_growth={loaded_growth} retained_growth={retained_growth}"
    );
    assert!(
        loaded_growth >= MINIMUM_LOADED_BYTES,
        "the document kept only {loaded_growth} resident bytes, below {MINIMUM_LOADED_BYTES}"
    );
    assert!(
        retained_growth * PERCENT <= loaded_growth * MAXIMUM_RETAINED_PERCENT,
        "destroy kept {retained_growth} of {loaded_growth} resident document bytes above the \
         pre-create baseline {baseline}, over {MAXIMUM_RETAINED_PERCENT}%"
    );
    exit_sender.send(()).unwrap();
    render_thread.join().unwrap();
    eprintln!("destroy returned the document's resident memory");
}

#[cfg(not(target_os = "android"))]
fn main() {}
