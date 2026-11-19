import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const target = { id: 'a'.repeat(64), label: 'Private target', team: 'research', host: 'Local native' };
const text = '<script>inert</script>\u001b[31m';
const messages = () => ({contract: 'coven.conversations.v1', id: target.id, team: target.team,
  source: 'djinn.conversation-read.v1', messages: [{role: 'assistant', text, truncated: false}],
  has_older: false, truncated: false, omitted_parts: false, projection_sha256: 'b'.repeat(64)});
const list = targets => ({contract: 'coven.conversations.v1', conversations: targets});
const flush = () => new Promise(resolve => setImmediate(resolve));
let serial = 0;

// Execute the real bootstrap with its actual event handlers. Fetch deliberately
// ignores abort so generation guards, not just AbortController, must fence replies.
async function browser() {
  class Node {
    children = []; listeners = new Map(); dataset = {}; value = ''; open = false; text = '';
    parentNode = null; root = false; opener = null;
    get isConnected() { return this.root || Boolean(this.parentNode?.isConnected); }
    contains(node) { return node === this || this.children.some(child => child.contains(node)); }
    focus() { if (this.isConnected) document.activeElement = this; }
    get textContent() { return this.text + this.children.map(n => n.textContent).join(''); }
    set textContent(value) { this.replaceChildren(); this.text = value; }
    append(...nodes) { for (const node of nodes) node.parentNode = this; this.children.push(...nodes); }
    replaceChildren(...nodes) {
      if (this.children.some(child => child.contains(document.activeElement))) document.activeElement = null;
      for (const node of this.children) node.parentNode = null;
      this.text = ''; this.children = []; this.append(...nodes);
    }
    addEventListener(event, handler) {
      const handlers = this.listeners.get(event) ?? []; handlers.push(handler); this.listeners.set(event, handlers);
    }
    emit(event) { for (const handler of this.listeners.get(event) ?? []) handler({ target: this }); }
    showModal() { this.opener = document.activeElement; this.open = true; }
    close() {
      const wasOpen = this.open; this.open = false;
      if (wasOpen) { this.opener?.focus(); this.emit('close'); }
    }
    setAttribute() {}
  }
  const html = await readFile(new URL('./index.html', import.meta.url), 'utf8');
  const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Node()]));
  const document = new Node(); document.hidden = false;
  document.activeElement = null;
  for (const node of nodes.values()) node.root = true;
  document.getElementById = id => { assert.ok(nodes.has(id), `missing DOM node ${id}`); return nodes.get(id); };
  document.createElement = () => new Node();
  const globals = ['document', 'fetch', 'setTimeout', 'clearTimeout', 'setInterval'];
  const saved = new Map(globals.map(k => [k, Object.getOwnPropertyDescriptor(globalThis, k)]));
  const originalNow = Date.now;
  let now = originalNow();
  const requests = []; const intervals = []; const timers = new Map(); let timerID = 0;
  Object.assign(globalThis, {document,
    fetch: (path, options) => new Promise(resolve => requests.push({path, options, resolve, done: false})),
    setTimeout: handler => { timers.set(++timerID, handler); return timerID; },
    clearTimeout: id => timers.delete(id), setInterval: handler => { intervals.push(handler); return ++timerID; },
  });
  Date.now = () => now;
  const $ = id => document.getElementById(id);
  const pending = path => requests.find(r => !r.done && r.path === path);
  const respond = async (path, value, status = 200) => {
    const request = pending(path); assert.ok(request, `no pending request for ${path}`);
    request.done = true;
    request.resolve({ok: status === 200, status, json: async () => value});
    await flush(); return request;
  };
  try {
    await import(`./app.js?bootstrap=${++serial}`);
    await respond('/api/v1/fleet', {contract: 'coven.fleet-observer.v1', generated_at_ms: now,
      refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: []});
    await respond('/api/v1/conversations', list([target]));
  } catch (error) { restore(); throw error; }
  function restore() {
    Date.now = originalNow;
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key];
    }
  }
  return { $, document, requests, pending, respond, restore,
    open: () => $('conversation-list').children[0].emit('click'),
    tick: ms => { now += ms; for (const handler of intervals) handler(); },
    fleet: async (status = 200) => {
      $('refresh').emit('click');
      await respond('/api/v1/fleet', status === 200 ? {contract: 'coven.fleet-observer.v1', generated_at_ms: now,
        refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: []} : {error: {code: 'access_denied'}}, status);
    },
  };
}

test('bootstrap clears rendered transcripts and all authorized metadata on fleet auth failure', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    assert.equal(b.$('conversation-messages').children[0].children[1].textContent, text);
    assert.equal(b.$('conversation-messages').children[0].children[1].innerHTML, undefined);
    await b.fleet(403);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
    assert.equal(b.$('conversation-team').textContent.includes(target.team), false);
    assert.equal(b.$('conversation-title').textContent, 'Conversation');
    assert.equal(b.$('conversation-dialog').open, false);
  } finally { b.restore(); }
});

test('bootstrap rejects deferred transcript and metadata callbacks after known auth revocation', async () => {
  const b = await browser();
  try {
    b.open();
    const read = b.pending(`/api/v1/conversations/${target.id}/messages`);
    await b.fleet(); // Starts a metadata-only revalidation while the read is pending.
    await b.fleet(403);
    assert.equal(read.options.signal.aborted, true);
    await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
    assert.equal(b.$('conversation-title').textContent, 'Conversation');
  } finally { b.restore(); }
});

test('bootstrap centralizes native-read and metadata-list authentication failures', async () => {
  for (const source of ['read', 'list']) {
    const b = await browser();
    try {
      b.open();
      if (source === 'list') await b.fleet();
      await b.respond(source === 'read' ? `/api/v1/conversations/${target.id}/messages`
        : '/api/v1/conversations', {error: {code: 'access_denied'}}, source === 'read' ? 401 : 403);
      assert.equal(b.$('conversation-list').children.length, 0);
      assert.equal(b.$('conversation-dialog').open, false);
      assert.match(b.$('connection-status').textContent, /unavailable/);
      assert.match(b.$('conversation-list-status').textContent, /Access denied/);
    } finally { b.restore(); }
  }
});

test('bootstrap refuses a deferred successful JSON body after another request revoked auth', async () => {
  const b = await browser();
  try {
    b.open();
    let finish;
    const body = new Promise(resolve => { finish = resolve; });
    await b.respond(`/api/v1/conversations/${target.id}/messages`, body);
    await b.fleet(403);
    finish(messages()); await flush();
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
  } finally { b.restore(); }
});

test('bootstrap observes missing resource references through metadata, without transcript polling', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    const reads = () => b.requests.filter(r => r.path.endsWith('/messages')).length;
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(reads(), 1);
    assert.equal(b.$('conversation-messages').textContent.includes(text), true);
    b.$('read-conversation').emit('click');
    await b.fleet(); await b.respond('/api/v1/conversations', list([{...target, id: 'c'.repeat(64)}]));
    assert.equal(b.$('conversation-dialog').open, false);
    await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(reads(), 2);
  } finally { b.restore(); }
});

test('bootstrap leases expire and visible lifecycle revalidates metadata only', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    b.tick(15000);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
    await b.respond('/api/v1/conversations', list([target]));
    b.document.hidden = true; b.document.emit('visibilitychange');
    assert.equal(b.$('conversation-list').children.length, 0);
    b.document.hidden = false; b.document.emit('visibilitychange');
    await b.respond('/api/v1/conversations', list([]));
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.$('conversation-list').children.length, 0);
  } finally { b.restore(); }
});

test('bootstrap close/reopen and team changes reject actual late read handlers', async () => {
  const b = await browser();
  try {
    b.open(); b.$('close-conversation').emit('click'); b.open();
    await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    b.$('conversation-team').value = target.team; b.$('conversation-team').emit('change');
    await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-dialog').open, false);
  } finally { b.restore(); }
});

test('unchanged metadata preserves connected conversation buttons and keyboard focus', async () => {
  const b = await browser();
  try {
    const opener = b.$('conversation-list').children[0];
    opener.focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.$('conversation-list').children[0], opener);
    assert.equal(opener.isConnected, true);
    assert.equal(b.document.activeElement, opener);
    // Revocation still removes the node immediately; preservation is not a cache.
    await b.fleet(); await b.respond('/api/v1/conversations', list([]));
    assert.equal(opener.isConnected, false);
    assert.equal(b.document.activeElement, null);
  } finally { b.restore(); }
});

test('metadata refresh retains the dialog opener for focus restoration on close', async () => {
  const b = await browser();
  try {
    const opener = b.$('conversation-list').children[0];
    opener.focus(); b.open();
    await b.respond(`/api/v1/conversations/${target.id}/messages`, messages());
    b.$('close-conversation').focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    b.$('close-conversation').emit('click');
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.$('conversation-list').children[0], opener);
    assert.equal(b.document.activeElement, opener);
  } finally { b.restore(); }
});
