#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
CRATE_NAME="yrs"
CRATE_VERSION="0.27.4"
CRATE_SHA256="3987db9bdbe6f0f49c58ec3d0daf4750a70b40019c190f6c6708abfcdfe6bea0"
CRATE_URL="https://static.crates.io/crates/$CRATE_NAME/$CRATE_NAME-$CRATE_VERSION.crate"
VENDOR_DIR="$SCRIPT_DIR/$CRATE_NAME"
PATCH_DIR="$SCRIPT_DIR/patches"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "==> Downloading $CRATE_NAME $CRATE_VERSION from crates.io..."
curl -sSLf -o "$WORK_DIR/crate.tar.gz" "$CRATE_URL"

echo "==> Verifying checksum..."
ACTUAL_SHA256="$(shasum -a 256 "$WORK_DIR/crate.tar.gz" | cut -d' ' -f1)"
if [ "$ACTUAL_SHA256" != "$CRATE_SHA256" ]; then
    echo "ERROR: checksum mismatch for $CRATE_NAME-$CRATE_VERSION.crate" >&2
    echo "  expected $CRATE_SHA256" >&2
    echo "  actual   $ACTUAL_SHA256" >&2
    exit 1
fi

echo "==> Extracting..."
tar -xzf "$WORK_DIR/crate.tar.gz" -C "$WORK_DIR"

echo "==> Applying patches..."
for patch in "$PATCH_DIR"/*.patch; do
    echo "  -> $(basename "$patch")"
    patch -p1 -d "$WORK_DIR/$CRATE_NAME-$CRATE_VERSION" < "$patch"
done

echo "==> Replacing $VENDOR_DIR..."
rm -rf "$VENDOR_DIR"
mv "$WORK_DIR/$CRATE_NAME-$CRATE_VERSION" "$VENDOR_DIR"

echo "==> Vendored $CRATE_NAME $CRATE_VERSION at $VENDOR_DIR"
