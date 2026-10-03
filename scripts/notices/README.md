# Notice generator inputs

`npm run notices:generate` builds the core and code-highlighting notices from
these inputs and Cargo's local crate sources. `npm run notices:check` checks the
same output without writing files. Both support `-- --offline` and require the
repository's pinned Rust toolchain. Cargo may download missing crate sources
unless offline mode is used; neither command builds native libraries or updates
lockfiles.

The generator unions normal and build dependencies across the seven targets in
the iOS and Android build scripts, with default features. Development dependencies
and optional CLI features are excluded. Update its target list if the shipped
targets change. License, copyright, and notice files are collected recursively,
including nested native code, plus any declared `license-file`.

- `config.json` defines package scopes, reviewed asset versions, patched source
  locations, and version-specific license fallbacks. A dependency without license
  text fails generation. Add the upstream text to `licenses/` and record its
  original source URL and exact crate version here; do not substitute a generic
  license for an upstream copyright notice.
- `preamble.md` supplies the common introduction.
- `core-supplement.md` retains the reviewed JavaScript, framework, copied icon,
  and native runtime notices. Update this input when those components change.
  Host application dependencies are not inferred from the development lockfile.
- `highlighting-supplement.md` retains the reviewed grammar, theme, and addon
  native notices. Syntect/two-face version changes require reviewing this file
  and updating `reviewedAssetVersions`.

The supplemental sections are maintained inputs, not automatically resolved
Gradle, CocoaPods, npm, or asset inventories. Their content is preserved verbatim.
The existing `RUST-STANDARD-LIBRARY-NOTICES.html` files remain separate artifacts;
a pinned toolchain version change requires refreshing them and updating
`standardLibraryVersion` before generation proceeds.

The output files are never used as inputs. Both documents are assembled before
either is written, so a missing dependency license does not replace one document
with incomplete output. Generated files should be checked in with dependency
updates. Run the focused regression tests with:

```sh
node --test scripts/tests/generate-third-party-notices.test.mjs
```
