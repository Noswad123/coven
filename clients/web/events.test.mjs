import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {validateConversations, validateAttention} from './app.js';

const app = await readFile(new URL('./app.js', import.meta.url), 'utf8');
const html = await readFile(new URL('./index.html', import.meta.url), 'utf8');
const flush = () => new Promise(resolve => setImmediate(resolve));
const freshness = (generation = 'epoch:1', remaining_ms = 15000) => ({generation, remaining_ms});
const conversations = f => ({contract: 'coven.conversations.v1', conversations: [], ...(f === undefined ? {} : {freshness: f})});
const attention = f => ({contract: 'coven.attention.v1', state: 'observed', items: [], omitted_count: 0,
  unavailable_count: 0, unlinked_count: 0, ...(f === undefined ? {} : {freshness: f})});

// The same deferred-fetch VM pattern as attention.test.mjs, with a monotonic clock.
function browser({events = true} = {}) {
  class Node {
    children = []; listeners = new Map(); dataset = {}; value = ''; text = ''; open = false;
    get textContent() { return this.text + this.children.map(n => n.textContent).join(''); }
    set textContent(v) { this.text = v; this.children = []; }
    append(...nodes) { this.children.push(...nodes); }
    replaceChildren(...nodes) { this.text = ''; this.children = nodes; }
    addEventListener(e, h) { const list = this.listeners.get(e) ?? []; list.push(h); this.listeners.set(e, list); }
    emit(e, data, extra = {}) { for (const h of this.listeners.get(e) ?? []) h({target: this, data, ...extra}); }
    close() { this.open = false; }
    setAttribute() {}
    querySelector() { return null; }
  }
  const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Node()]));
  const document = new Node(); document.hidden = false;
  document.getElementById = id => nodes.get(id); document.createElement = () => new Node();
  const sources = [], requests = [], timers = new Map(), intervals = new Map(), window = new Node();
  let now = 0, serial = 0;
  class EventSource extends Node {
    constructor(path) { super(); assert.equal(path, '/api/v1/events'); sources.push(this); }
    close() { this.closed = true; }
    event(kind, value = {contract: 'coven.events.v1'}) { this.emit(kind, JSON.stringify(value)); }
  }
  const context = vm.createContext({document, TextEncoder, AbortController, URL, console,
    performance: {now: () => now}, ...(events ? {EventSource} : {}),
    addEventListener: (e, h) => window.addEventListener(e, h),
    setTimeout: (h, ms) => { timers.set(++serial, {h, ms}); return serial; }, clearTimeout: id => timers.delete(id),
    setInterval: h => { intervals.set(++serial, h); return serial; }, clearInterval: id => intervals.delete(id),
    fetch: (path, options) => new Promise(resolve => requests.push({path, options, resolve, done: false})),
  });
  vm.runInContext(app.replace(/^export /gm, ''), context);
  const pending = kind => requests.find(r => !r.done && r.path === `/api/v1/${kind}`);
  const respond = async (kind, value, status = 200) => {
    const r = pending(kind); assert.ok(r, `pending ${kind}`); r.done = true;
    r.resolve({ok: status === 200, status, json: async () => value}); await flush();
  };
  const fleet = () => ({contract: 'coven.fleet-observer.v1', generated_at_ms: Date.now(),
    refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: []});
  const cycle = async (f = freshness()) => {
    await respond('fleet', fleet()); await respond('conversations', conversations(f)); await respond('attention', attention(f));
  };
  return {sources, requests, pending, respond, cycle, fleet, document, nodes, context,
    live: () => nodes.get('poll-status').textContent.includes('Live metadata'),
    advance: ms => { now += ms; intervals.forEach(h => h()); },
    timer: async ms => { for (const [id, t] of [...timers]) if (t.ms === ms) { timers.delete(id); t.h(); } await flush(); },
    hide: hidden => { document.hidden = hidden; document.emit('visibilitychange'); },
    pagehide: persisted => window.emit('pagehide', undefined, {persisted}),
    pageshow: persisted => window.emit('pageshow', undefined, {persisted}),
    intervalCount: () => intervals.size,
    destroy: () => window.emit('pagehide'),
  };
}

test('freshness is optional but strictly validates generation, bounded integer and closed fields', () => {
  for (const [validate, value] of [[validateConversations, conversations], [validateAttention, attention]]) {
    validate(value()); validate(value(freshness())); validate(value(freshness('opaque-epoch:42', 0)));
    for (const f of [null, {}, freshness('', 10), freshness('epoch', 10), freshness('epoch:1', -1),
      freshness('epoch:1', 15001), freshness('epoch:1', 1.5), {...freshness(), extra: true}]) assert.throws(() => validate(value(f)));
  }
});

test('open is not live; ready requires a complete ordered resync and coalesces in-flight invalidation', async () => {
  const b = browser(), s = b.sources[0]; s.emit('open'); assert.equal(b.live(), false);
  s.event('ready'); s.event('fleet'); s.event('fleet');
  await b.cycle(); assert.equal(b.live(), false);
  assert.ok(b.pending('fleet')); await b.respond('fleet', b.fleet()); assert.equal(b.live(), true);
  assert.deepEqual(b.requests.map(r => r.path.split('/').at(-1)), ['fleet', 'conversations', 'attention', 'fleet']);
  s.event('metadata'); s.event('metadata'); s.event('metadata');
  await b.respond('conversations', conversations(freshness())); await b.respond('attention', attention(freshness()));
  assert.ok(b.pending('conversations')); await b.respond('conversations', conversations(freshness()));
  assert.equal(b.pending('attention'), undefined);
  assert.ok(b.requests.every(r => r.options.method === 'GET' && /^\/api\/v1\/(fleet|conversations|attention)$/.test(r.path)));
});

test('same generation cannot extend a deadline; new evidence renews, transit consumes, zero expires', async () => {
  const b = browser(); b.sources[0].event('ready'); await b.cycle(); await b.respond('fleet', b.fleet());
  b.advance(10000); b.sources[0].event('metadata');
  await b.respond('conversations', conversations(freshness())); await b.respond('attention', attention(freshness()));
  b.advance(5000);
  assert.equal(b.live(), false);
  assert.match(b.nodes.get('attention-status').textContent, /unknown/);
  b.sources[0].event('metadata');
  await b.respond('conversations', conversations(freshness('epoch:2', 5000)));
  await b.respond('attention', attention(freshness('epoch:2', 5000)));
  assert.match(b.nodes.get('attention-status').textContent, /No pending/);
  b.sources[0].event('metadata'); b.advance(3000);
  await b.respond('conversations', conversations(freshness('epoch:3', 2000)));
  await b.respond('attention', attention(freshness('epoch:3', 0)));
  assert.match(b.nodes.get('attention-status').textContent, /unknown/);
  assert.equal(b.live(), false);
});

test('hide, reconnect and auth fence old callbacks and deferred GETs; resume fully resyncs', async () => {
  for (const fence of ['hide', 'error', 'auth']) {
    const b = browser(), old = b.sources[0]; old.event('ready');
    const late = b.pending('fleet');
    if (fence === 'hide') { b.hide(true); b.hide(false); }
    if (fence === 'error') { old.emit('error'); await b.timer(6000); }
    if (fence === 'auth') { await b.respond('fleet', {}, 403); }
    assert.equal(old.closed, true);
    const count = b.requests.length; old.event('ready'); old.event('metadata'); assert.equal(b.requests.length, count);
    if (!late.done) { late.done = true; late.resolve({ok: false, status: 403, json: async () => ({})}); await flush(); }
    if (fence !== 'auth') {
      const s = b.sources.at(-1); assert.notEqual(s, old); assert.equal(s.closed, undefined);
      s.event('ready'); await b.cycle();
      if (b.pending('fleet')) await b.respond('fleet', b.fleet());
      assert.equal(b.live(), true);
    }
    b.destroy(); assert.ok(b.sources.every(s => s.closed));
  }
});

test('no EventSource and old freshness contracts stay on six-second fallback; healthy stream reconciles at thirty', async () => {
  const absent = browser({events: false}); await absent.cycle(); assert.equal(absent.live(), false);
  await absent.timer(6000); assert.ok(absent.pending('fleet'));
  const old = browser(); await old.cycle(undefined); old.sources[0].event('ready');
  await old.respond('fleet', old.fleet()); await old.respond('conversations', conversations()); await old.respond('attention', attention());
  assert.equal(old.live(), false);
  const healthy = browser(); healthy.sources[0].event('ready'); await healthy.cycle(); await healthy.respond('fleet', healthy.fleet());
  await healthy.timer(6000); assert.equal(healthy.pending('fleet'), undefined);
  await healthy.timer(30000); assert.ok(healthy.pending('fleet'));
});

test('metadata fetch errors and malformed freshness fail closed without a cached-GET feedback loop', async () => {
  for (const value of [attention(freshness('epoch:2', 0)), attention(null), attention(freshness('epoch:2', '5000'))]) {
    const b = browser(); b.sources[0].event('ready'); await b.cycle(); await b.respond('fleet', b.fleet());
    b.sources[0].event('metadata');
    await b.respond('conversations', conversations(freshness('epoch:2', 0)));
    await b.respond('attention', value);
    assert.equal(b.live(), false); assert.match(b.nodes.get('attention-status').textContent, /unknown/);
    const count = b.requests.length; b.advance(30000); await flush(); assert.equal(b.requests.length, count);
    await b.timer(6000); assert.ok(b.pending('fleet'));
  }
});

test('malformed events and events before ready fall back without inventing authentication loss', async () => {
  for (const kind of ['fleet', 'metadata', 'malformed']) {
    const b = browser(), old = b.sources[0];
    if (kind === 'malformed') old.emit('ready', '{'); else old.event(kind);
    assert.equal(old.closed, true); assert.equal(b.live(), false);
    assert.doesNotMatch(b.nodes.get('notice').textContent, /Access denied/);
    await b.timer(6000); assert.equal(b.sources.length, 2);
  }
});

test('persisted page lifecycle restores one expiry guard and fully resyncs SSE and fallback without content reads', async () => {
  for (const events of [false, true]) {
    for (const visibilityFirst of [false, true]) {
      const b = browser({events});
      await b.cycle();
      if (events) { b.sources[0].event('ready'); await b.cycle(); }
      const old = b.sources[0];
      b.nodes.get('conversation-messages').textContent = 'private transcript';
      b.nodes.get('output-text').textContent = 'private output';
      b.nodes.get('decision-details').textContent = 'private decision';
      b.nodes.get('prompt-text').value = 'private draft';
      b.pagehide(true);
      assert.equal(b.intervalCount(), 0);
      for (const id of ['conversation-messages', 'output-text', 'decision-details']) assert.equal(b.nodes.get(id).textContent, '');
      assert.equal(b.nodes.get('prompt-text').value, '');
      assert.match(b.nodes.get('attention-status').textContent, /unknown/);
      if (old) assert.equal(old.closed, true);
      const count = b.requests.length;
      b.hide(true);
      if (visibilityFirst) b.hide(false);
      b.advance(30000); await b.timer(6000); await b.timer(30000);
      assert.equal(b.requests.length, count);
      b.pageshow(true); b.pageshow(true);
      assert.equal(b.intervalCount(), 1);
      if (!visibilityFirst) {
        assert.equal(b.requests.length, count);
        b.hide(false);
      }
      assert.equal(b.requests.length, count + 1);
      assert.ok(b.pending('fleet'));
      if (events) {
        assert.equal(b.sources.length, 2);
        old.event('ready'); old.event('metadata'); old.emit('error');
        assert.equal(b.sources.length, 2);
      }
      await b.cycle(freshness('epoch:2', 5000));
      if (events) { assert.equal(b.live(), false); b.sources[1].event('ready'); await b.cycle(freshness('epoch:2', 5000)); }
      const restoredCount = b.requests.length;
      b.pageshow(true); assert.equal(b.requests.length, restoredCount); assert.equal(b.intervalCount(), 1);
      b.advance(5000);
      assert.equal(b.live(), false);
      assert.match(b.nodes.get('attention-status').textContent, /unknown/);
      assert.match(b.nodes.get('conversation-list-status').textContent, /lease expired/);
      assert.ok(b.requests.every(r => r.options.method === 'GET' && /^\/api\/v1\/(fleet|conversations|attention)$/.test(r.path)));
      b.destroy(); assert.equal(b.intervalCount(), 0);
    }
  }
});

test('BFCache restore fences deferred metadata headers and bodies across a fresh resync', async () => {
  for (const events of [false, true]) {
    for (const stage of ['headers', 'body']) {
      const b = browser({events});
      await b.respond('fleet', b.fleet());
      const late = b.pending('conversations'); late.done = true;
      let finish;
      if (stage === 'body') {
        late.resolve({ok: true, json: () => new Promise(resolve => { finish = resolve; })}); await flush();
      }
      b.pagehide(true); assert.equal(late.options.signal.aborted, true);
      b.pageshow(true);
      await b.cycle(freshness('epoch:2'));
      if (events) { b.sources.at(-1).event('ready'); await b.cycle(freshness('epoch:2')); }
      if (finish) finish(conversations(freshness('old:1', 0)));
      else late.resolve({ok: false, status: 403, json: async () => ({})});
      await flush();
      assert.match(b.nodes.get('attention-status').textContent, /No pending/);
      assert.equal(b.live(), events);
      assert.doesNotMatch(b.nodes.get('notice').textContent, /Access denied/);
      assert.equal(b.intervalCount(), 1);
      assert.ok(b.requests.every(r => r.options.method === 'GET' && /^\/api\/v1\/(fleet|conversations|attention)$/.test(r.path)));
      b.destroy();
    }
  }
});

test('nonpersisted teardown cannot be revived by pageshow or visible events', async () => {
  for (const events of [false, true]) {
    const b = browser({events}); b.destroy();
    const count = b.requests.length;
    b.pageshow(true); b.pageshow(false); b.hide(true); b.hide(false);
    await b.timer(6000); b.advance(30000);
    assert.equal(b.intervalCount(), 0); assert.equal(b.requests.length, count);
    assert.ok(b.sources.every(s => s.closed));
  }
});
