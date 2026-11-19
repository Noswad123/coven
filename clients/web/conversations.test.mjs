import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { validateConversations, validateConversation, createOutputGate, setText } from './app.js';

const target = { id: 'a'.repeat(64), label: 'Explicit target', team: 'research', host: 'Local native' };
const text = '<script>alert(1)</script>\u001b[31m\0\b\t\n\f\r\u001f"\\/é\u2028\u2029😀';
const response = () => ({contract: 'coven.conversations.v1', id: target.id, team: target.team,
  source: 'djinn.conversation-read.v1', messages: [{role: 'assistant', text, truncated: false}],
  has_older: true, truncated: false, omitted_parts: true, projection_sha256: 'b'.repeat(64)});

test('explicit list validates references and no inferred availability', () => {
  assert.deepEqual(validateConversations({contract: 'coven.conversations.v1', conversations: [target]}), [target]);
  assert.throws(() => validateConversations({contract: 'coven.conversations.v1', conversations: [{...target, id: '/private/path'}]}));
  assert.throws(() => validateConversations({contract: 'coven.conversations.v1', conversations: [target, target]}));
});

test('hostile text is inert and bounded by UTF8 bytes, target and team', () => {
  assert.equal(validateConversation(response(), target).messages[0].text, text);
  const node = { textContent: '' };
  setText(node, text); assert.equal(node.textContent, text); assert.equal(node.innerHTML, undefined);
  assert.throws(() => validateConversation(response(), {...target, team: 'other'}));
  const long = response(); long.messages[0].text = 'é'.repeat(4097);
  assert.throws(() => validateConversation(long, target));
  const valid = response(); valid.messages[0].text = 'x'.repeat(8192);
  assert.equal(validateConversation(valid, target), valid);
});

test('late async responses fail across close/reopen, target and team changes', async () => {
  const gate = createOutputGate();
  const key = JSON.stringify([target.id, target.team]);
  let finish;
  const late = new Promise(resolve => { finish = resolve; });
  const old = gate.begin(key);
  const update = late.then(() => gate.accepts(old, key));
  gate.invalidate(); gate.begin(key);
  finish(); assert.equal(await update, false);
  const ticket = gate.begin(key);
  assert.equal(gate.accepts(ticket, JSON.stringify([target.id, 'other'])), false);
  gate.begin('new-target'); assert.equal(gate.accepts(ticket, key), false);
});

test('JSON.stringify canonical tuple encoding agrees with Rust vector', () => {
  const value = '\0\b\t\n\f\r\u001f"\\/é\u2028\u2029😀';
  const encoded = JSON.stringify([['msg_fixture', 'assistant', value, false]]);
  assert.equal(encoded, '[["msg_fixture","assistant","\\u0000\\b\\t\\n\\f\\r\\u001f\\"\\\\/é\u2028\u2029😀",false]]');
  assert.equal(createHash('sha256').update(encoded).digest('hex'), '4b55cfc034c79e9c1f6bbc31c7443fe11a9e18f14777f16f988d520a3a9ddbb8');
});
