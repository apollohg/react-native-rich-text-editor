use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;

use jni_sys::{jboolean, jbyteArray, jclass, jlong, jobject, jsize, JNIEnv, JNI_TRUE};

use super::editor::editor_v2_apply_native_intent;
use super::native_frame::editor_v2_render_native_frame;

unsafe fn utf8_argument(env: *mut JNIEnv, array: jbyteArray) -> Option<String> {
    let functions = &**env;
    let length = (functions.GetArrayLength?)(env, array);
    let mut bytes = vec![0u8; usize::try_from(length).ok()?];
    (functions.GetByteArrayRegion?)(env, array, 0, length, bytes.as_mut_ptr().cast());
    String::from_utf8(bytes).ok()
}

unsafe fn byte_array(env: *mut JNIEnv, bytes: &[u8]) -> Option<jbyteArray> {
    let functions = &**env;
    let length = jsize::try_from(bytes.len()).ok()?;
    let array = (functions.NewByteArray?)(env, length);
    if array.is_null() {
        return None;
    }
    (functions.SetByteArrayRegion?)(env, array, 0, length, bytes.as_ptr().cast());
    Some(array)
}

fn serialized<T: uniffi::Lower<crate::UniFfiTag>>(value: T) -> Vec<u8> {
    let mut bytes = Vec::new();
    T::write(value, &mut bytes);
    bytes
}

unsafe fn respond(env: *mut JNIEnv, call: impl FnOnce() -> Option<Vec<u8>>) -> jbyteArray {
    catch_unwind(AssertUnwindSafe(call))
        .ok()
        .flatten()
        .and_then(|bytes| byte_array(env, &bytes))
        .unwrap_or(null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_apollohg_editor_EditorCoreJni_isAvailable(
    _env: *mut JNIEnv,
    _binding: jobject,
) -> jboolean {
    JNI_TRUE
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_apollohg_editor_EditorCoreJni_applyNativeIntent(
    env: *mut JNIEnv,
    _class: jclass,
    editor_id: jbyteArray,
    request_json: jbyteArray,
) -> jbyteArray {
    respond(env, || {
        Some(serialized(editor_v2_apply_native_intent(
            utf8_argument(env, editor_id)?,
            utf8_argument(env, request_json)?,
        )))
    })
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_apollohg_editor_EditorCoreJni_renderNativeFrame(
    env: *mut JNIEnv,
    _class: jclass,
    editor_id: jbyteArray,
    owner_id: jbyteArray,
    has_mirror_anchor: jboolean,
    mirror_anchor: jlong,
    has_mirror_head: jboolean,
    mirror_head: jlong,
) -> jbyteArray {
    respond(env, || {
        let owner_id = if owner_id.is_null() {
            None
        } else {
            Some(utf8_argument(env, owner_id)?)
        };
        Some(serialized(editor_v2_render_native_frame(
            utf8_argument(env, editor_id)?,
            owner_id,
            optional_u32(has_mirror_anchor, mirror_anchor)?,
            optional_u32(has_mirror_head, mirror_head)?,
        )))
    })
}

fn optional_u32(present: jboolean, value: jlong) -> Option<Option<u32>> {
    if present == JNI_TRUE {
        u32::try_from(value).ok().map(Some)
    } else {
        Some(None)
    }
}
