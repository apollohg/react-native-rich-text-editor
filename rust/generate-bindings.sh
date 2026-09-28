#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$SCRIPT_DIR/editor-core"
OUT_DIR="$SCRIPT_DIR/bindings"
TARGET_DIR="${CARGO_TARGET_DIR:-$CRATE_DIR/target}"
STATICLIB_PATH="$TARGET_DIR/release/libeditor_core.a"
CDYLIB_PATH="$TARGET_DIR/release/libeditor_core.dylib"

source "$SCRIPT_DIR/v2-symbols.sh"

OBSOLETE_V2_SYMBOLS=(
    editor_v2_collaboration_begin_connect
    editor_v2_collaboration_take_outbound
    editor_v2_collaboration_tick
)

normalize_header() {
    local header_path="$1"
    local normalized_path="${header_path}.normalized"
    awk '
      {
        sub(/[[:blank:]]+$/, "")
        if ($0 == "") {
          blank_lines += 1
        } else {
          while (blank_lines > 0) {
            print ""
            blank_lines -= 1
          }
          print
        }
      }
    ' "$header_path" > "$normalized_path"
    mv "$normalized_path" "$header_path"
}

usage() {
    echo "usage: $0 [--normalize-header PATH]" >&2
    exit 2
}

if [[ "${1:-}" == "--normalize-header" ]]; then
    [[ "$#" == "2" ]] || usage
    normalize_header "$2"
    exit 0
elif [[ "$#" != "0" ]]; then
    echo "unknown argument: ${1:-}" >&2
    exit 2
fi

source "$SCRIPT_DIR/toolchain.sh"
CARGO_CMD=("$RUST_TOOLCHAIN_CARGO")

# Always rebuild before generating bindings so UniFFI sees the current exported API.
echo "==> Building editor-core for host target..."
"${CARGO_CMD[@]}" build --manifest-path "$CRATE_DIR/Cargo.toml" --release \
    --target-dir "$TARGET_DIR"

echo "==> Verifying the dylib exposes exactly the ${#V2_SYMBOLS[@]} editor_v2_* symbols..."
DYLIB_SYMBOLS="$(nm -gU "$CDYLIB_PATH")"
for symbol in "${V2_SYMBOLS[@]}"; do
    grep -q "uniffi_editor_core_fn_func_${symbol}" <<< "$DYLIB_SYMBOLS" || {
        echo "error: dylib is missing uniffi_editor_core_fn_func_${symbol}" >&2
        exit 1
    }
done
for symbol in "${OBSOLETE_V2_SYMBOLS[@]}"; do
    if grep -q "uniffi_editor_core_fn_func_${symbol}" <<< "$DYLIB_SYMBOLS"; then
        echo "error: dylib still exposes obsolete uniffi_editor_core_fn_func_${symbol}" >&2
        exit 1
    fi
done
grep -q "uniffi_editor_core_fn_func_editor_core_version" <<< "$DYLIB_SYMBOLS" || {
    echo "error: dylib is missing the editor_core_version query" >&2
    exit 1
}
grep -q "uniffi_editor_core_fn_func_viewer_compile" <<< "$DYLIB_SYMBOLS" || {
    echo "error: dylib is missing uniffi_editor_core_fn_func_viewer_compile" >&2
    exit 1
}
for method in "${VIEWER_METHODS[@]}"; do
    grep -q "uniffi_editor_core_fn_method_viewercompileddocument_${method}" <<< "$DYLIB_SYMBOLS" || {
        echo "error: dylib is missing ViewerCompiledDocument.${method}" >&2
        exit 1
    }
done
LEGACY_LINES="$(nm -gU "$CDYLIB_PATH" | grep -E 'uniffi_editor_core_(fn|checksum)_func_(editor_|collaboration_)' | grep -v 'editor_v2\|editor_core_version' || true)"
if [[ -n "$LEGACY_LINES" ]]; then
    echo "error: dylib exposes legacy editor_*/collaboration_* symbols (expected 0):" >&2
    echo "$LEGACY_LINES" >&2
    exit 1
fi

# uniffi-bindgen --library mode needs to find Cargo.toml via cargo metadata,
# so we run from within the crate directory.
cd "$CRATE_DIR"

echo "==> Generating Swift bindings..."
mkdir -p "$OUT_DIR/swift"
"${CARGO_CMD[@]}" run --release \
    --features cli \
    --target-dir "$TARGET_DIR" \
    --bin uniffi-bindgen -- \
    generate --library "$STATICLIB_PATH" \
    --language swift \
    --out-dir "$OUT_DIR/swift"
normalize_header "$OUT_DIR/swift/editor_coreFFI.h"
normalize_header "$OUT_DIR/swift/editor_core.swift"

echo "==> Generating Kotlin bindings..."
mkdir -p "$OUT_DIR/kotlin"
"${CARGO_CMD[@]}" run --release \
    --features cli \
    --target-dir "$TARGET_DIR" \
    --bin uniffi-bindgen -- \
    generate --library "$CDYLIB_PATH" \
    --language kotlin \
    --no-format \
    --out-dir "$OUT_DIR/kotlin"
normalize_header "$OUT_DIR/kotlin/uniffi/editor_core/editor_core.kt"

echo "==> Verifying the generated bindings expose the v2 symbols..."
for symbol in "${V2_SYMBOLS[@]}"; do
    for artifact in \
        "$OUT_DIR/swift/editor_coreFFI.h" \
        "$OUT_DIR/swift/editor_core.swift" \
        "$OUT_DIR/kotlin/uniffi/editor_core/editor_core.kt"; do
        grep -q "uniffi_editor_core_fn_func_${symbol}" "$artifact" || {
            echo "error: generated binding $artifact is missing uniffi_editor_core_fn_func_${symbol}" >&2
            exit 1
        }
    done
done
for artifact in \
    "$OUT_DIR/swift/editor_coreFFI.h" \
    "$OUT_DIR/swift/editor_core.swift" \
    "$OUT_DIR/kotlin/uniffi/editor_core/editor_core.kt"; do
    grep -q "uniffi_editor_core_fn_func_viewer_compile" "$artifact" || {
        echo "error: generated binding $artifact is missing viewer_compile" >&2
        exit 1
    }
    for method in "${VIEWER_METHODS[@]}"; do
        grep -q "uniffi_editor_core_fn_method_viewercompileddocument_${method}" "$artifact" || {
            echo "error: generated binding $artifact is missing ViewerCompiledDocument.${method}" >&2
            exit 1
        }
    done
done
for artifact in \
    "$OUT_DIR/swift/editor_coreFFI.h" \
    "$OUT_DIR/swift/editor_core.swift" \
    "$OUT_DIR/kotlin/uniffi/editor_core/editor_core.kt"; do
    LEGACY_ARTIFACT_LINES="$(grep -E 'uniffi_editor_core_(fn|checksum)_func_(editor_|collaboration_)' "$artifact" | grep -v 'editor_v2\|editor_core_version' || true)"
    if [[ -n "$LEGACY_ARTIFACT_LINES" ]]; then
        echo "error: generated binding $artifact exposes legacy symbols (expected 0):" >&2
        echo "$LEGACY_ARTIFACT_LINES" >&2
        exit 1
    fi
done
for symbol in "${OBSOLETE_V2_SYMBOLS[@]}"; do
    for artifact in \
        "$OUT_DIR/swift/editor_coreFFI.h" \
        "$OUT_DIR/swift/editor_core.swift" \
        "$OUT_DIR/kotlin/uniffi/editor_core/editor_core.kt"; do
        if grep -q "uniffi_editor_core_\(fn\|checksum\)_func_${symbol}" "$artifact"; then
            echo "error: generated binding $artifact still exposes obsolete ${symbol}" >&2
            exit 1
        fi
    done
done

echo "==> Copying Swift binding into ios/ for Xcode compilation..."
PKG_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cp "$OUT_DIR/swift/editor_core.swift" "$PKG_DIR/ios/Generated_editor_core.swift"
mkdir -p "$PKG_DIR/ios/editor_coreFFI"
cp "$OUT_DIR/swift/editor_coreFFI.h" "$PKG_DIR/ios/editor_coreFFI/editor_coreFFI.h"
cp "$OUT_DIR/swift/editor_coreFFI.modulemap" "$PKG_DIR/ios/editor_coreFFI/module.modulemap"

echo "==> Bindings generated and copied:"
echo "  Swift:     $OUT_DIR/swift/"
echo "  Kotlin:    $OUT_DIR/kotlin/"
echo "  iOS copy:  $PKG_DIR/ios/Generated_editor_core.swift"
echo "  iOS FFI:   $PKG_DIR/ios/editor_coreFFI/"
echo "  Android:   Gradle sources include $OUT_DIR/kotlin/ via build.gradle"
