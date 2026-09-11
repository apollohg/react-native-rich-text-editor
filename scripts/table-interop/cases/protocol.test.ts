import assert from 'node:assert/strict';
import test from 'node:test';
import { assertReply, type PeerReply } from '../peer-protocol.js';

test('assertReply throws when the reply id does not match the request id', () => {
    const reply: PeerReply = { id: '2', value: {}, error: null, events: [] };
    assert.throws(() => assertReply(reply, '1'), /Invalid peer reply envelope/);
});

test('assertReply throws when both value and error are null', () => {
    const reply: PeerReply = { id: '1', value: null, error: null, events: [] };
    assert.throws(() => assertReply(reply, '1'), /Invalid peer reply envelope/);
});

test('assertReply throws when both value and error are present', () => {
    const reply: PeerReply = {
        id: '1',
        value: {},
        error: { code: 'E_TEST_BOTH_PRESENT', message: 'both value and error set' },
        events: [],
    };
    assert.throws(() => assertReply(reply, '1'), /Invalid peer reply envelope/);
});

test('assertReply accepts the exact valid envelope shape without throwing', () => {
    const reply: PeerReply = { id: '1', value: {}, error: null, events: [] };
    assert.equal(assertReply(reply, '1'), undefined);
});

test('base64 event payloads round-trip through Buffer byte-for-byte, including the zero byte', () => {
    const bytesBase64 = 'AAH/';
    const decoded = Buffer.from(bytesBase64, 'base64');
    assert.equal(decoded.length, 3);
    assert.equal(decoded[0], 0x00);
    assert.equal(decoded[1], 0x01);
    assert.equal(decoded[2], 0xff);
    assert.equal(decoded.toString('base64'), bytesBase64);
});
