#!/usr/bin/env python3
import pathlib
import runpy
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[2]
optimizer = root / "rust/optimize-kotlin-bindings.py"
patch = runpy.run_path(str(optimizer))
original = patch["ORIGINAL_ENCODER"]
optimized = patch["OPTIMIZED_ENCODER"]
with tempfile.TemporaryDirectory(prefix="native-editor-kotlin-encoder.") as directory:
    path = pathlib.Path(directory) / "binding.kt"
    for name, fixture, succeeds in [
        ("original", original, True),
        ("already optimized", optimized, False),
        ("duplicate", original + "\n" + original, False),
        ("upstream drift", original.replace("CharBuffer.wrap(value)", "CharBuffer.wrap(value, 0, value.length)"), False),
    ]:
        path.write_text(fixture)
        result = subprocess.run([sys.executable, str(optimizer), str(path)], capture_output=True, text=True)
        assert (result.returncode == 0) == succeeds, (name, result.stderr)
        assert path.read_text() == (optimized if succeeds else fixture), name
    checked_in = (root / "rust/bindings/kotlin/uniffi/editor_core/editor_core.kt").read_text()
    assert checked_in.count(optimized) == 1
    path.write_text(checked_in.replace(optimized, original))
    subprocess.run([sys.executable, str(optimizer), str(path)], check=True)
    assert path.read_text() == checked_in, "Kotlin regeneration must reproduce the checked-in encoder"
print("Kotlin encoder regeneration and upstream drift checks passed.")
