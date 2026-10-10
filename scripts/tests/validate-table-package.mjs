import assert from 'node:assert/strict';
import { closeSync, existsSync, openSync, readFileSync, readSync, readdirSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const packageRoot = resolve(process.argv[2] ?? '.');
const runtimeExports = ['withTablesSchema', 'TABLE_NODE_NAMES', 'TableToolbar', 'TABLE_TOOLBAR_ACTIONS', 'useTableToolbar', 'placeTableToolbar'];
const typeExports = ['TableCommand', 'TableCellSelection', 'TableCellStep', 'TableDirection', 'TableEdge', 'TableHeaderTarget', 'TableSelectionGeometry', 'TableRole', 'TableNamingPreset', 'TableNodeNames', 'TablesSchemaOptions', 'TableToolbarProps', 'TableToolbarAction', 'TableToolbarActionSpec', 'TableToolbarIdentity', 'TableToolbarOptions', 'TableToolbarState', 'TableToolbarRect', 'TableToolbarSize'];
const tableRecords = ['FfiNativeRenderFrame', 'FfiCellInputBlock', 'FfiCellNestedTable', 'FfiTableAttribute', 'FfiTableCellRecord', 'FfiTableCellUpdate', 'FfiTableExtent', 'FfiTableFrame', 'FfiTableHost', 'FfiTableRecord', 'FfiTableSourceRow', 'FfiViewerTable', 'FfiViewerTableCell', 'TableRenderRow', 'TableRenderSyntheticRegion'];
const nativeDirectories = ['ios/Tables', 'android/src/main/java/com/apollohg/editor/tables'];
const forbiddenPaths = ['scripts/table-interop', 'AGENTS.md', 'CLAUDE.md', 'docs/superpowers', '.superpowers', '.agents', '.codex', 'plans', 'specs'];
const peerMarker = /table_interop|tableInteropPeer|table_interop_peer/;
const binaryChunkBytes = 1024 * 1024;
const markerOverlapBytes = 64;

function requiredFile(relativePath) {
    const file = join(packageRoot, relativePath);
    assert.ok(existsSync(file) && statSync(file).isFile() && statSync(file).size > 0, `missing packed file: ${relativePath}`);
    return file;
}

function* files(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const file = join(directory, entry.name);
        if (entry.isDirectory()) yield* files(file);
        else if (entry.isFile()) yield file;
    }
}

const manifest = JSON.parse(readFileSync(requiredFile('package.json'), 'utf8'));
for (const section of ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies']) {
    for (const name of Object.keys(manifest[section] ?? {})) {
        assert.ok(!/^(?:@tiptap\/|prosemirror-|yjs$|y-prosemirror$|y-protocols$|(?:@[^/]+\/)?(?:playwright|puppeteer|chromium)(?:$|[-/]))/.test(name), `forbidden development dependency: ${section}/${name}`);
    }
}
for (const path of forbiddenPaths) assert.ok(!existsSync(join(packageRoot, path)), `forbidden packaged path: ${path}`);
for (const directory of nativeDirectories) {
    for (const file of files(join(repositoryRoot, directory))) requiredFile(file.slice(repositoryRoot.length + 1));
}
assert.match(readFileSync(requiredFile('ReactNativeProseEditor.podspec'), 'utf8'), /['"]ios\/Tables\/\*\*\/\*\.swift['"]/, 'CocoaPods podspec must include ios/Tables sources');
for (const file of ['TableTypes.d.ts', 'TableToolbar.js', 'TableToolbar.d.ts', 'useTableToolbar.js', 'TableToolbarPlacement.js', 'schemas.js']) requiredFile(`dist/${file}`);

const entry = requiredFile('dist/index.d.ts');
const program = ts.createProgram([entry], { noEmit: true, skipLibCheck: true, moduleResolution: ts.ModuleResolutionKind.Node10, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX });
const checker = program.getTypeChecker();
const exported = new Map(checker.getExportsOfModule(checker.getSymbolAtLocation(program.getSourceFile(entry))).map((symbol) => [symbol.name, symbol]));
for (const name of [...typeExports, ...runtimeExports]) {
    const symbol = exported.get(name);
    assert.ok(symbol, `missing public table declaration: ${name}`);
    const target = symbol.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(symbol) : symbol;
    assert.ok(target.declarations?.length, `unresolved public table declaration: ${name}`);
}
const runtime = readFileSync(requiredFile('dist/index.js'), 'utf8');
for (const name of runtimeExports) {
    assert.ok(runtime.includes(`Object.defineProperty(exports, "${name}",`), `missing runtime table export: ${name}`);
}
const require = createRequire(import.meta.url);
const schemas = require(requiredFile('dist/schemas.js'));
for (const schema of [schemas.prosemirrorSchema, schemas.tiptapCompatibleSchema]) {
    const extended = schemas.withTablesSchema(schema);
    assert.ok(extended.nodes.some((node) => node.tableRole === 'table'), 'withTablesSchema must produce a table-enabled schema');
}

const swift = readFileSync(requiredFile('ios/Generated_editor_core.swift'), 'utf8');
const kotlin = readFileSync(requiredFile('rust/bindings/kotlin/uniffi/editor_core/editor_core.kt'), 'utf8');
assert.ok(!peerMarker.test(swift) && !peerMarker.test(kotlin), 'packaged bindings expose a test-only table peer');
const kotlinScalars = { UInt: 'UInt32', ULong: 'UInt64', Int: 'Int32', Long: 'Int64', Boolean: 'Bool', Float: 'Float', Double: 'Double', String: 'String', UByte: 'UInt8', Byte: 'Int8', Short: 'Int16', UShort: 'UInt16' };
function normalizeKotlin(type) {
    return type.trim().replace(/List<([^<>]+)>/g, '[$1]').replace(/kotlin\.([A-Za-z]+)/g, (_, name) => kotlinScalars[name] ?? name);
}
for (const name of tableRecords) {
    const swiftBody = swift.match(new RegExp(`public struct ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
    const kotlinBody = kotlin.match(new RegExp(`data class ${name} \\(([\\s\\S]*?)\\n\\)`))?.[1];
    assert.ok(swiftBody && kotlinBody, `missing generated table record: ${name}`);
    const swiftFields = [...swiftBody.matchAll(/public var (\w+): ([^\n]+)/g)].map((match) => [match[1], match[2].trim()]);
    const kotlinFields = [...kotlinBody.matchAll(/var `(\w+)`: ([^,\n]+)/g)].map((match) => [match[1], normalizeKotlin(match[2])]);
    assert.ok(swiftFields.length > 0, `empty generated table record: ${name}`);
    assert.deepEqual(kotlinFields, swiftFields, `generated table fields disagree: ${name}`);
}
for (const file of files(packageRoot)) {
    if (!/libeditor_core\.(?:a|so)$/.test(file)) continue;
    const descriptor = openSync(file, 'r');
    try {
        const buffer = Buffer.alloc(binaryChunkBytes);
        let preceding = '';
        let count;
        while ((count = readSync(descriptor, buffer)) > 0) {
            const text = preceding + buffer.subarray(0, count).toString('latin1');
            assert.ok(!peerMarker.test(text), `test-only table peer symbol in ${file}`);
            preceding = text.slice(-markerOverlapBytes);
        }
    } finally {
        closeSync(descriptor);
    }
}
console.log('Table package exports, native sources, generated records and development exclusions pass.');
