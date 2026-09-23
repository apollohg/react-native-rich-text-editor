import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const mappingPath = process.argv[2];
assert.ok(mappingPath, 'Pass the consuming app’s R8 mapping.txt path');
const mapping = readFileSync(mappingPath, 'utf8');
const bridgeName = 'com.apollohg.editor.viewer.FabricLeaseHandleBridge';
const block = mapping.split(/\r?\n(?=[^\s#])/).find(line => line.startsWith(`${bridgeName} -> `));
assert.ok(block, `${bridgeName} was removed from the release build`);
assert.equal(block.split(/\r?\n/)[0], `${bridgeName} -> ${bridgeName}:`,
    'The viewer JNI bridge class must retain its name in release builds');

for (const [method, parameters] of [
    ['registerNativeLease', 'int,int,long'],
    ['finalizeNativeLease', 'int,int,long'],
    ['beginNativeMeasure', 'long'],
    ['beginNativeFinalLayout', 'long,int,int'],
    ['endNativeMeasure', ''],
]) {
    assert.ok(block.split(/\r?\n/).some(line =>
        line.includes(`void ${method}(${parameters})`) && line.endsWith(` -> ${method}`)),
    `JNI entry point ${method}(${parameters}) must survive R8 with its original name`);
}

console.log('Release viewer JNI class and entry points retain their native lookup names.');
