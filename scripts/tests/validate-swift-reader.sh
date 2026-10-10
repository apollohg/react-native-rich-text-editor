#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
fixture_dir="$(mktemp -d "${TMPDIR:-/tmp}/native-editor-swift-reader.XXXXXX")"
trap 'rm -rf "$fixture_dir"' EXIT
python3 - "$repo_root" "$fixture_dir/reader.swift" <<'PYTHON'
import pathlib
import sys
import runpy
import subprocess
root = pathlib.Path(sys.argv[1])
optimizer = root / "rust/optimize-swift-bindings.py"
patch = runpy.run_path(str(optimizer))
original = patch["ORIGINAL_READER"]
optimized = patch["OPTIMIZED_READER"]
fixture_path = pathlib.Path(sys.argv[2]).with_name("binding.swift")
for name, fixture, succeeds in [
    ("original", original, True),
    ("already optimized", optimized, False),
    ("duplicate", original + "\n" + original, False),
    ("upstream drift", original.replace("var value: T = 0", "var value: T = .zero"), False),
]:
    fixture_path.write_text(fixture)
    result = subprocess.run([sys.executable, str(optimizer), str(fixture_path)], capture_output=True, text=True)
    assert (result.returncode == 0) == succeeds, (name, result.stderr)
    assert fixture_path.read_text() == (optimized if succeeds else fixture), name
source = (root / "ios/Generated_editor_core.swift").read_text()
start = source.index("fileprivate func readInt<")
end = source.index("// Reads an arbitrary number of bytes", start)
fixture = (root / "scripts/tests/fixtures/swift-reader.swift").read_text()
pathlib.Path(sys.argv[2]).write_text(
    "import Foundation\nenum UniffiInternalError: Error { case bufferOverflow }\n"
    + source[start:end] + fixture
)
PYTHON
xcrun swiftc -swift-version 5 -O "$fixture_dir/reader.swift" -o "$fixture_dir/reader"
"$fixture_dir/reader"
