#!/usr/bin/env bash
set -euo pipefail

[[ "$#" -eq 3 ]] || { echo "usage: $0 ios|android INPUT OUTPUT" >&2; exit 1; }
platform="$1"
input="$2"
output="$3"
case "$platform" in
    ios) strip_mode=--strip-debug ;;
    android) strip_mode=--strip-unneeded ;;
    *) echo "error: unsupported native platform: $platform" >&2; exit 1 ;;
esac
[[ ! "$input" -ef "$output" ]] || { echo "error: preserve the original native library; output must be a separate file" >&2; exit 1; }

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
source "$script_dir/toolchain.sh"
host="$("$RUSTC" -vV | sed -n 's/^host: //p')"
llvm_strip="$("$RUSTC" --print sysroot)/lib/rustlib/$host/bin/llvm-strip"
[[ -x "$llvm_strip" ]] || {
    echo "error: missing pinned LLVM strip; run rustup component add llvm-tools-preview --toolchain $RUST_TOOLCHAIN_VERSION" >&2
    exit 1
}

# Apple strip can break Rust archives with embedded LLVM bitcode.
"$llvm_strip" "$strip_mode" -o "$output" "$input"
