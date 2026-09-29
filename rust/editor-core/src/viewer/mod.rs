mod compile;
mod types;

pub use types::{
    FfiViewerCompileRequest, FfiViewerCompileResult, FfiViewerElement, FfiViewerMark,
    FfiViewerSourceKind, ViewerCompiledDocument,
};

#[uniffi::export]
pub fn viewer_compile(request: FfiViewerCompileRequest) -> FfiViewerCompileResult {
    compile::compile(request)
}

#[cfg(test)]
mod tests;

pub(crate) use compile::viewer_leaf_element;

#[cfg(test)]
pub(crate) use compile::lower_cached_tables_for_test;
