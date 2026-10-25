#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
source "$repo_root/rust/toolchain.sh"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/native-editor-stripping.XXXXXX")"
trap 'rm -rf "$fixture_root"' EXIT
strip_script="$repo_root/rust/strip-native-library.sh"
host="$("$RUSTC" -vV | sed -n 's/^host: //p')"
llvm_bin="$("$RUSTC" --print sysroot)/lib/rustlib/$host/bin"

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

cat > "$fixture_root/probe.c" <<'SOURCE'
int native_size_probe(int value) { return value + 1; }
SOURCE
cat > "$fixture_root/consumer.c" <<'SOURCE'
extern int native_size_probe(int);
int consume_native_size_probe(void) { return native_size_probe(41); }
SOURCE

platforms=("$@")
if [[ "${#platforms[@]}" -eq 0 ]]; then platforms=(android ios); fi
for platform in "${platforms[@]}"; do
    if [[ "$platform" == ios ]]; then
        sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
        xcrun clang -target arm64-apple-ios16.4 -isysroot "$sdk" -g -c \
            "$fixture_root/probe.c" -o "$fixture_root/probe.o"
        "$llvm_bin/llvm-ar" crs "$fixture_root/ios-original.a" "$fixture_root/probe.o"
        extension=a
    elif [[ "$platform" == android ]]; then
        ndk_root="${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to run Android stripping tests}"
        ndk_bins=("$ndk_root"/toolchains/llvm/prebuilt/*/bin)
        [[ "${#ndk_bins[@]}" -eq 1 ]] || fail "expected one NDK host toolchain"
        "${ndk_bins[0]}/aarch64-linux-android24-clang" -g -shared -nostdlib \
            "$fixture_root/probe.c" -o "$fixture_root/android-original.so"
        extension=so
    else
        fail "unknown test platform: $platform"
    fi
    original="$fixture_root/$platform-original.$extension"
    published="$fixture_root/$platform-published.$extension"
    cp "$original" "$original.saved"
    bash "$strip_script" "$platform" "$original" "$published"
    original_bytes="$(wc -c < "$original" | tr -d ' ')"
    published_bytes="$(wc -c < "$published" | tr -d ' ')"
    [[ "$published_bytes" -lt "$original_bytes" ]] || fail "$platform did not shrink: $original_bytes -> $published_bytes"
    if [[ "$platform" == android ]]; then
        for artifact in original published; do
            "$llvm_bin/llvm-nm" --dynamic --defined-only "$fixture_root/$platform-$artifact.$extension" > "$fixture_root/$artifact.symbols"
            "$llvm_bin/llvm-objcopy" --dump-section ".text=$fixture_root/$artifact.text" \
                "$fixture_root/$platform-$artifact.$extension" "$fixture_root/$artifact.inspected.$extension"
        done
        cmp "$fixture_root/original.symbols" "$fixture_root/published.symbols" || fail "Android dynamic exports changed"
        cmp "$fixture_root/original.text" "$fixture_root/published.text" || fail "Android executable code changed"

        # Size optimisation can place x86 functions at odd addresses.
        ruby -rjson -e '
            manifest = JSON.parse(File.read(ARGV.fetch(0)))
            functions = [manifest.fetch("version")] + manifest.fetch("functions") + manifest.fetch("viewer").fetch("functions")
            symbols = functions.map { |entry| ["uniffi_editor_core_checksum_func_#{entry.fetch("name")}", entry.fetch("checksum")] }
            manifest.fetch("viewer").fetch("objects").each do |object|
                object.fetch("methods").each do |entry|
                    symbols << ["uniffi_editor_core_checksum_method_#{object.fetch("name")}_#{entry.fetch("name")}", entry.fetch("checksum")]
                end
            end
            puts ".text"
            symbols.each do |name, checksum|
                puts ".p2align 1", "nop", ".globl #{name}", ".type #{name},@function", "#{name}:", "movw $#{checksum}, %ax", "ret", ".size #{name}, .-#{name}"
            end
        ' "$repo_root/scripts/package-abi-manifest.json" > "$fixture_root/odd-checksums.S"
        for abi in x86 x86_64; do
            if [[ "$abi" == x86 ]]; then triple=i686; else triple=x86_64; fi
            "${ndk_bins[0]}/$triple-linux-android24-clang" -shared -nostdlib \
                "$fixture_root/odd-checksums.S" -o "$fixture_root/$abi-checksums.so"
            ruby "$repo_root/scripts/validate-uniffi-checksum-values.rb" \
                --manifest "$repo_root/scripts/package-abi-manifest.json" \
                --label "$abi odd-address checksum fixture" --elf "$abi" "$fixture_root/$abi-checksums.so"
            echo "$abi: odd-address checksum functions validated"
        done
    else
        xcrun clang -target arm64-apple-ios16.4 -isysroot "$sdk" -dynamiclib \
            "$fixture_root/consumer.c" "$published" -o "$fixture_root/consumer.dylib"
    fi
    cmp "$original" "$original.saved" || fail "$platform inspection modified the compiler output"
    echo "$platform: $original_bytes -> $published_bytes bytes; compiler output preserved; consumer contract passed"
done

if bash "$strip_script" unsupported "$original" "$fixture_root/invalid.so" > "$fixture_root/invalid.log" 2>&1; then
    fail "accepted an unsupported platform"
fi
[[ ! -e "$fixture_root/invalid.so" ]] || fail "invalid platform produced an artifact"
grep -q 'unsupported native platform' "$fixture_root/invalid.log" || fail "missing platform diagnostic"
if bash "$strip_script" "$platform" "$original" "$original" > "$fixture_root/same-file.log" 2>&1; then
    fail "accepted overwriting the compiler output"
fi
cmp "$original" "$original.saved" || fail "same-file rejection modified the compiler output"
grep -q 'preserve the original native library' "$fixture_root/same-file.log" || fail "missing preservation diagnostic"
echo "Native stripping tests passed."
