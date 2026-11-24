import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { MAX_UNRESOLVED_PROMPTS, validateConversations } from './app.js';

const target = {id: 'a'.repeat(64), label: 'Shared target', team: 'research', host: 'Local native', can_submit: true, max_prompt_bytes: 65536,
  association_status: 'verified', card: {fleet_agent_id: 'e'.repeat(64), fleet_endpoint_id: 'desktop', binding_id: 'f'.repeat(64)}};
const list = targets => ({contract: 'coven.conversations.v1', conversations: targets});
const token = {contract: 'coven.csrf.v1', token: `0-${'a'.repeat(64)}-${'b'.repeat(64)}`};
const flush = () => new Promise(resolve => setImmediate(resolve));
const messages = (row = target) => ({contract: 'coven.conversations.v1', id: row.id, team: row.team,
  source: 'djinn.conversation-read.v1', messages: [], has_older: false, truncated: false,
  omitted_parts: false, projection_sha256: 'b'.repeat(64)});
let serial = 0;

// Real bootstrap, real handlers; deferred fetch ignores abort deliberately.
async function browser(rows = [target], fleetEndpoints = [{id: 'desktop', label: 'Fleet host', connection: 'reachable',
  last_attempt_at_ms: Date.now(), observed_at_ms: Date.now(), error: null,
  agents: ['e', 'f'].map(letter => ({id: letter.repeat(64), pane_name: `Fleet ${letter}`, title: `Fleet ${letter}`, provider: 'djinn', project: 'Project',
    activity: 'unknown', evidence: 'kitsune_snapshot', observed_at_ms: Date.now(), output_available: false}))}]) {
  class Node {
    children = []; listeners = new Map(); dataset = {}; value = ''; open = false; text = ''; checked = false;
    contains(node) { return node === this || this.children.some(child => child.contains(node)); }
    focus() { document.activeElement = this; }
    get textContent() { return this.text + this.children.map(n => n.textContent).join(''); }
    set textContent(value) { this.replaceChildren(); this.text = value; }
    append(...nodes) { for (const node of nodes) node.parentNode = this; this.children.push(...nodes); }
    replaceChildren(...nodes) { for (const node of this.children) node.parentNode = null; this.text = ''; this.children = []; this.append(...nodes); }
    addEventListener(event, handler) { const handlers = this.listeners.get(event) ?? []; handlers.push(handler); this.listeners.set(event, handlers); }
    emit(event) { for (const handler of this.listeners.get(event) ?? []) handler({target: this, preventDefault() {}}); }
    showModal() { this.open = true; }
    close() { const open = this.open; this.open = false; if (open) this.emit('close'); }
    setAttribute() {}
    querySelector(selector) {
      return this.children.find(child => child.className?.split(' ').includes(selector.slice(1)))
        ?? this.children.map(child => child.querySelector(selector)).find(Boolean) ?? null;
    }
    remove() {
      if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1);
      this.parentNode = null;
    }
    insertBefore(node, before) {
      node.remove(); node.parentNode = this;
      this.children.splice(before ? this.children.indexOf(before) : this.children.length, 0, node);
    }
  }
  const html = await readFile(new URL('./index.html', import.meta.url), 'utf8');
  const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Node()]));
  const document = new Node(); document.hidden = false;
  document.getElementById = id => { assert.ok(nodes.has(id), `missing ${id}`); return nodes.get(id); };
  document.createElement = () => new Node();
  const globals = ['document', 'fetch', 'setTimeout', 'clearTimeout', 'setInterval', 'crypto', 'Map'];
  const saved = new Map(globals.map(k => [k, Object.getOwnPropertyDescriptor(globalThis, k)]));
  const originalNow = Date.now; let now = originalNow(); let uuidCount = 0;
  const requests = []; const intervals = []; const timers = new Map(); let timerID = 0;
  const NativeMap = Map;
  let attemptMap = null;
  // Observe the real bootstrap's stored metadata, not a separate gate helper.
  globalThis.Map = class extends NativeMap {
    set(key, value) {
      if (value?.request_id === key && typeof value.phase === 'string') attemptMap = this;
      return super.set(key, value);
    }
  };
  Object.defineProperty(globalThis, 'crypto', {configurable: true, value: {randomUUID() { uuidCount++; return `12345678-1234-4234-8234-${String(uuidCount).padStart(12, '0')}`; }}});
  Object.assign(globalThis, {document,
    fetch: (path, options) => path === '/api/v1/attention'
      ? Promise.resolve({ok: true, json: async () => ({contract: 'coven.attention.v1', state: 'observed', items: [], omitted_count: 0, unavailable_count: 0, unlinked_count: 0})})
      : new Promise((resolve, reject) => requests.push({path, options, resolve, reject, done: false})),
    setTimeout: handler => { timers.set(++timerID, handler); return timerID; },
    clearTimeout: id => timers.delete(id), setInterval: handler => { intervals.push(handler); return ++timerID; },
  });
  Date.now = () => now;
  const $ = id => document.getElementById(id);
  const pending = path => requests.find(r => !r.done && r.path === path);
  const respond = async (path, value, status = 200) => {
    const request = pending(path); assert.ok(request, `no pending ${path}`); request.done = true;
    request.resolve({ok: status === 200, status, json: async () => value}); await flush(); return request;
  };
  function restore() { Date.now = originalNow; for (const [key, descriptor] of saved) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key]; } }
  const fleet = async (status = 200) => {
    $('refresh').emit('click');
    await respond('/api/v1/fleet', {contract: 'coven.fleet-observer.v1', generated_at_ms: now, refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: fleetEndpoints}, status);
  };
  try {
    await import(`./app.js?prompts=${++serial}`);
    await respond('/api/v1/fleet', {contract: 'coven.fleet-observer.v1', generated_at_ms: now, refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: fleetEndpoints});
    await respond('/api/v1/conversations', list(rows));
  } catch (error) { restore(); throw error; }
  return {$, requests, pending, respond, restore, fleet, document,
    open: (index = 0) => {
      $('agent-grid').children[index]?.children[0].emit('click');
    },
    attempts: () => [...(attemptMap?.values() ?? [])].map(value => ({...value})),
    draft: text => { $('prompt-text').value = text; $('prompt-text').emit('input'); },
    send: () => $('prompt-composer').emit('submit'),
    posts: () => requests.filter(r => r.options.method === 'POST'),
    uuids: () => uuidCount,
    tick: ms => { now += ms; for (const handler of intervals) handler(); },
    lose: async path => { const r = pending(path); assert.ok(r); r.done = true; r.reject(new Error('lost receipt')); await flush(); },
  };
}
const postPath = `/api/v1/conversations/${target.id}/prompts`;
const boundPath = row => `/api/v1/agents/${row.card.fleet_agent_id}/conversations/${row.id}/bindings/${row.card.binding_id}/messages`;
const readPath = boundPath(target);
const receipt = (request, row = target) => ({contract: 'coven.conversation-submit.v1', id: row.id,
  request_id: JSON.parse(request.options.body).request_id, outcome: 'accepted', delivery: 'accepted'});

test('merged card posts to the exact native handle; changed bindings cannot inspect or resolve its unknown receipt', async () => {
  const fleetID = 'e'.repeat(64), binding = 'f'.repeat(64);
  const linked = {...target, association_status: 'verified', card: {fleet_agent_id: fleetID, fleet_endpoint_id: 'desktop', binding_id: binding}};
  const endpoint = {id: 'desktop', label: 'Desktop', connection: 'reachable', last_attempt_at_ms: Date.now(), observed_at_ms: Date.now(), error: null,
    agents: [{id: fleetID, pane_name: target.label, title: target.label, provider: 'djinn', project: '', activity: 'unknown', evidence: 'kitsune_snapshot', observed_at_ms: Date.now(), output_available: true}]};
  const path = id => `/api/v1/agents/${fleetID}/conversations/${target.id}/bindings/${id}/messages`;
  const b = await browser([linked], [endpoint]);
  try {
    b.open(); await b.respond(path(binding), messages());
    b.draft('follow up'); b.send(); await b.respond('/api/v1/csrf', token);
    assert.equal(b.posts()[0].path, postPath);
    await b.lose(postPath);
    const requestID = b.attempts()[0].request_id;
    b.$('read-conversation').emit('click');
    const replacement = {...linked, card: {...linked.card, binding_id: 'c'.repeat(64)}};
    await b.fleet(); await b.respond('/api/v1/conversations', list([replacement]));
    await b.respond(path(binding), messages());
    assert.equal(b.attempts()[0].request_id, requestID);
    assert.equal(b.attempts()[0].inspected, false);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('prompt-new-decision').disabled, true);
    assert.equal(b.posts().length, 1);
    b.$('read-conversation').emit('click'); await b.respond(path(replacement.card.binding_id), messages());
    assert.equal(b.attempts()[0].inspected, true);
    assert.equal(b.attempts()[0].phase, 'uncertain');
    assert.equal(b.posts().length, 1);
    assert.equal(b.requests.some(r => r.path.endsWith('/output')), false);
  } finally { b.restore(); }
});

test('list write permission is optional fail-closed and byte limit is fixed', () => {
  const old = {...target}; delete old.can_submit; delete old.max_prompt_bytes;
  assert.equal(validateConversations(list([old]))[0].can_submit, undefined);
  for (const change of [{can_submit: 'true'}, {max_prompt_bytes: 65537}, {can_submit: true, max_prompt_bytes: undefined}]) {
    assert.throws(() => validateConversations(list([{...target, ...change}])));
  }
});

test('actual composer is read-only for legacy rows and validates exact UTF8/control/blank text', async () => {
  const b = await browser([{...target, can_submit: false}]);
  try { b.open(); b.draft('hello'); b.send(); assert.equal(b.$('prompt-composer').hidden, true); assert.equal(b.requests.some(r => r.path === '/api/v1/csrf'), false); }
  finally { b.restore(); }
  const w = await browser();
  try {
    w.open(); assert.equal(w.$('prompt-composer').hidden, false);
    for (const invalid of [' \t\r\n', 'bad\0', 'bad\u0085', 'é'.repeat(32769)]) { w.draft(invalid); assert.equal(w.$('send-prompt').disabled, true); w.send(); }
    assert.equal(w.posts().length, 0);
    w.draft('é'); assert.equal(w.$('prompt-bytes').textContent, '2 / 65536 UTF-8 bytes');
    assert.equal(w.$('send-prompt').disabled, false);
  } finally { w.restore(); }
});

test('one UUID and one fetch invocation per deliberate send; accepted never means completed', async () => {
  const b = await browser();
  try {
    b.open(); const exact = '<script>not HTML</script>\t\r\n'; b.draft(exact); b.send(); b.send();
    assert.equal(b.uuids(), 0); assert.equal(b.posts().length, 0);
    await b.respond('/api/v1/csrf', token);
    b.send(); assert.equal(b.posts().length, 1); assert.equal(b.uuids(), 1);
    assert.deepEqual(JSON.parse(b.posts()[0].options.body), {contract: 'coven.conversation-submit.v1', request_id: '12345678-1234-4234-8234-000000000001', execute: true, text: exact});
    assert.equal(b.posts()[0].options.redirect, 'error');
    await b.respond(postPath, receipt(b.posts()[0]));
    assert.match(b.$('prompt-attempt-status').textContent, /persisted\/scheduled, not completed/);
    b.$('read-conversation').emit('click'); assert.equal(b.posts().length, 1);
  } finally { b.restore(); }
});

test('token fetch failure is unattempted; lost receipt freezes with no retry until inspection and separate decision', async () => {
  const b = await browser();
  try {
    b.open(); b.draft('first'); b.send(); await b.lose('/api/v1/csrf');
    assert.equal(b.posts().length, 0); assert.equal(b.uuids(), 0); assert.match(b.$('prompt-attempt-status').textContent, /Not sent/);
    b.send(); await b.respond('/api/v1/csrf', token); await b.lose(postPath);
    assert.match(b.$('prompt-attempt-status').textContent, /Delivery uncertain.*Inspect conversation/);
    assert.equal(b.$('prompt-text').disabled, true); b.draft('second'); b.send(); assert.equal(b.posts().length, 1);
    await b.respond(readPath, messages()); // Original open-read is not explicit inspection.
    assert.equal(b.$('prompt-new-decision').disabled, true);
    b.$('read-conversation').emit('click'); await b.respond(readPath, messages());
    assert.equal(b.$('prompt-new-decision').disabled, false); assert.equal(b.$('send-prompt').disabled, true);
    b.$('prompt-new-decision').checked = true; b.$('prompt-new-decision').emit('change');
    b.send(); await b.respond('/api/v1/csrf', token);
    assert.equal(b.posts().length, 2); assert.equal(b.uuids(), 2);
    assert.notEqual(JSON.parse(b.posts()[0].options.body).request_id, JSON.parse(b.posts()[1].options.body).request_id);
  } finally { b.restore(); }
});

test('close/reopen and team changes fence late token and accepted callbacks', async () => {
  for (const stage of ['token', 'post']) {
    const b = await browser();
    try {
      b.open(); b.draft('sensitive'); b.send();
      if (stage === 'post') await b.respond('/api/v1/csrf', token);
      b.$('close-conversation').emit('click'); b.open();
      assert.equal(b.$('prompt-text').value, '');
      if (stage === 'token') { await b.respond('/api/v1/csrf', token); assert.equal(b.posts().length, 0); }
      else {
        assert.equal(b.posts()[0].options.signal.aborted, true);
        await b.respond(postPath, receipt(b.posts()[0]));
        assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /Accepted:/);
        assert.match(b.$('prompt-attempt-status').textContent, /Inspect conversation/);
        assert.equal(b.$('send-prompt').disabled, true);
      }
      b.$('conversation-team').emit('change'); assert.equal(b.$('prompt-text').value, '');
    } finally { b.restore(); }
  }
});

test('auth, hide, metadata expiry and write revocation clear content but retain non-content pending notice', async () => {
  for (const change of ['auth', 'hide', 'expiry', 'revoke']) {
    const b = await browser();
    try {
      b.open(); b.draft('secret'); b.send(); await b.respond('/api/v1/csrf', token);
      if (change === 'auth') await b.fleet(403);
      if (change === 'hide') { b.document.hidden = true; b.document.emit('visibilitychange'); }
      if (change === 'expiry') b.tick(15000);
      if (change === 'revoke') { await b.fleet(); await b.respond('/api/v1/conversations', list([{...target, can_submit: false}])); }
      assert.equal(b.$('prompt-text').value, ''); assert.equal(b.$('conversation-messages').textContent, '');
      assert.match(b.$('prompt-attempt-status').textContent, /uncertain/);
      assert.equal(b.$('prompt-attempt-status').textContent.includes('secret'), false);
      await b.respond(postPath, receipt(b.posts()[0]));
      assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /Accepted:/);
      assert.equal(b.posts().length, 1);
    } finally { b.restore(); }
  }
});

test('deferred receipt bodies cannot restore accepted status after auth revocation', async () => {
  const b = await browser();
  try {
    b.open(); b.draft('sensitive'); b.send(); await b.respond('/api/v1/csrf', token);
    let finish;
    const body = new Promise(resolve => { finish = resolve; });
    await b.respond(postPath, body);
    await b.fleet(403);
    finish(receipt(b.posts()[0]));
    await new Promise(resolve => setImmediate(resolve));
    assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /Accepted:/);
    assert.match(b.$('prompt-attempt-status').textContent, /Inspect conversation/);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('prompt-text').value, '');
    assert.equal(b.posts().length, 1);
  } finally { b.restore(); }
});

test('HTTP errors and invalid receipts after POST are uncertain, never automatic rejections or retries', async () => {
  for (const status of [200, 403, 503]) {
    const b = await browser();
    try {
      b.open(); b.draft('once'); b.send(); await b.respond('/api/v1/csrf', token);
      await b.respond(postPath, {outcome: 'accepted', delivery: 'accepted'}, status);
      assert.match(b.$('prompt-attempt-status').textContent, /Delivery uncertain/);
      assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /Not admitted/);
      b.send(); assert.equal(b.posts().length, 1);
      if (status === 403) assert.equal(b.$('conversation-dialog').open, false);
    } finally { b.restore(); }
  }
});

test('a read started before uncertainty cannot count as inspecting that uncertain attempt', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(readPath, messages());
    b.draft('once'); b.send(); await b.respond('/api/v1/csrf', token);
    b.$('read-conversation').emit('click');
    await b.lose(postPath);
    await b.respond(readPath, messages());
    assert.equal(b.$('prompt-new-decision').disabled, true);
    b.$('read-conversation').emit('click'); await b.respond(readPath, messages());
    assert.equal(b.$('prompt-new-decision').disabled, false);
  } finally { b.restore(); }
});

const second = {...target, id: 'c'.repeat(64), team: 'other-team', label: 'Independent target',
  card: {...target.card, fleet_agent_id: 'f'.repeat(64), binding_id: 'b'.repeat(64)}};
const pathFor = (row, resource) => resource === 'messages' ? boundPath(row) : `/api/v1/conversations/${row.id}/${resource}`;
async function uncertainA(b) {
  b.open(); await b.respond(readPath, messages());
  b.draft('private A text'); b.send(); await b.respond('/api/v1/csrf', token);
  await b.lose(postPath);
}

test('wrong-target inspection and independent accepted, unsent or token-failed sends cannot resolve A uncertainty', async () => {
  for (const outcome of ['accepted', 'unsent', 'token-failed']) {
    const b = await browser([target, second]);
    try {
      await uncertainA(b);
      const original = b.attempts()[0];
      b.$('close-conversation').emit('click'); b.open(1);
      await b.respond(pathFor(second, 'messages'), messages(second));
      b.$('read-conversation').emit('click');
      await b.respond(pathFor(second, 'messages'), messages(second));
      assert.equal(b.attempts()[0].inspected, false, 'B read cannot inspect A');
      b.draft('private B text');
      assert.equal(b.$('send-prompt').disabled, false, 'A must not block independent B');
      b.send(); b.send();
      if (outcome === 'token-failed') {
        await b.lose('/api/v1/csrf');
        assert.match(b.$('prompt-status').textContent, /Not sent/);
      } else {
        await b.respond('/api/v1/csrf', token);
        const post = b.posts()[1];
        const report = receipt(post, second);
        if (outcome === 'unsent') { report.outcome = 'unavailable'; report.delivery = 'unsent'; }
        await b.respond(pathFor(second, 'prompts'), report);
        assert.match(b.$('prompt-status').textContent, outcome === 'accepted' ? /Accepted:/ : /Not admitted/);
      }
      assert.deepEqual(b.attempts(), [original], 'only A remains unresolved');
      assert.match(b.$('prompt-attempt-status').textContent, /1 unresolved.*Delivery uncertain/);
      assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /Accepted:|private A|private B/);
      b.$('close-conversation').emit('click'); b.open();
      await b.respond(readPath, messages());
      assert.equal(b.$('prompt-new-decision').disabled, true);
      b.draft('new A'); b.send(); assert.equal(b.pending('/api/v1/csrf'), undefined);
      b.$('read-conversation').emit('click'); await b.respond(readPath, messages());
      assert.equal(b.$('prompt-new-decision').disabled, false);
      assert.equal(b.$('send-prompt').disabled, true, 'matching inspection alone does not authorize a new send');
      b.$('prompt-new-decision').checked = true; b.$('prompt-new-decision').emit('change');
      b.send(); await b.respond('/api/v1/csrf', token);
      const next = b.posts().at(-1);
      assert.notEqual(JSON.parse(next.options.body).request_id, original.request_id);
      await b.respond(postPath, receipt(next));
      assert.equal(b.attempts().length, 1, 'even a new accepted A send cannot resolve the original unknown receipt');
      assert.match(b.$('prompt-attempt-status').textContent, /1 unresolved/);
    } finally { b.restore(); }
  }
});

test('unresolved metadata survives close, team, auth, hide and opaque-reference replacement without retaining text', async () => {
  for (const change of ['close', 'team', 'auth', 'hide', 'replacement']) {
    const b = await browser([target, second]);
    try {
      await uncertainA(b);
      const original = b.attempts()[0];
      assert.deepEqual(Object.keys(original).sort(), ['fenced', 'id', 'inspected', 'phase', 'request_id', 'sent', 'team']);
      assert.equal(original.id, target.id); assert.equal(original.team, target.team);
      assert.equal(original.request_id, JSON.parse(b.posts()[0].options.body).request_id);
      b.draft('discarded draft');
      if (change === 'close') b.$('close-conversation').emit('click');
      if (change === 'team') { b.$('conversation-team').value = second.team; b.$('conversation-team').emit('change'); }
      if (change === 'auth') await b.fleet(403);
      if (change === 'hide') { b.document.hidden = true; b.document.emit('visibilitychange'); }
      if (change === 'replacement') {
        await b.fleet();
        await b.respond('/api/v1/conversations', list([{...target, id: 'd'.repeat(64)}, second]));
      }
      assert.equal(b.$('prompt-text').value, '');
      assert.equal(b.$('conversation-messages').textContent, '');
      assert.deepEqual(b.attempts(), [original]);
      assert.doesNotMatch(JSON.stringify(b.attempts()), /private A text|discarded draft|text|label/);
      assert.match(b.$('prompt-attempt-status').textContent, /1 unresolved.*Original references may expire/);
      assert.doesNotMatch(b.$('prompt-attempt-status').textContent, /private A|discarded draft|research/);
      if (change === 'replacement') {
        const replacement = {...target, id: 'd'.repeat(64)};
        b.open(); await b.respond(pathFor(replacement, 'messages'), messages(replacement));
        b.$('read-conversation').emit('click'); await b.respond(pathFor(replacement, 'messages'), messages(replacement));
        assert.equal(b.attempts()[0].inspected, false, 'a new opaque reference is not the old target');
        b.draft('new reference'); assert.equal(b.$('send-prompt').disabled, false);
      }
    } finally { b.restore(); }
  }
});

test('unresolved set is bounded and never silently evicts uncertain attempts, including inspected attempts', async () => {
  const b = await browser([target, second]);
  try {
    await uncertainA(b);
    const first = b.attempts()[0];
    for (let count = 1; count < MAX_UNRESOLVED_PROMPTS; count++) {
      b.$('read-conversation').emit('click'); await b.respond(readPath, messages());
      b.$('prompt-new-decision').checked = true; b.$('prompt-new-decision').emit('change');
      b.draft('next bounded attempt'); b.send(); await b.respond('/api/v1/csrf', token); await b.lose(postPath);
      assert.equal(b.attempts().length, count + 1);
    }
    assert.equal(b.attempts()[0].request_id, first.request_id);
    assert.equal(b.attempts()[0].inspected, true, 'inspection is not proof of delivery and cannot evict a record');
    assert.equal(b.$('send-prompt').disabled, true);
    assert.match(b.$('prompt-status').textContent, /capacity reached.*no uncertain records have been discarded/);
    b.$('close-conversation').emit('click'); b.open(1);
    await b.respond(pathFor(second, 'messages'), messages(second));
    b.draft('independent but at capacity'); b.send();
    assert.equal(b.pending('/api/v1/csrf'), undefined);
    assert.equal(b.posts().length, MAX_UNRESOLVED_PROMPTS);
    assert.equal(b.attempts().length, MAX_UNRESOLVED_PROMPTS);
    assert.match(b.$('prompt-status').textContent, /capacity reached/);
  } finally { b.restore(); }
});

test('pending submission stays globally single-flight across targets, then only its origin is gated', async () => {
  const b = await browser([target, second]);
  try {
    b.open(); await b.respond(readPath, messages());
    b.draft('pending A'); b.send(); await b.respond('/api/v1/csrf', token);
    b.$('close-conversation').emit('click'); b.open(1);
    await b.respond(pathFor(second, 'messages'), messages(second));
    b.draft('B waits'); b.send();
    assert.equal(b.$('send-prompt').disabled, true);
    assert.equal(b.pending('/api/v1/csrf'), undefined);
    assert.equal(b.posts().length, 1);
    await b.lose(postPath);
    assert.equal(b.$('send-prompt').disabled, false, 'B is independent once the pending A request settles');
    b.send(); await b.respond('/api/v1/csrf', token);
    await b.respond(pathFor(second, 'prompts'), receipt(b.posts()[1], second));
    assert.equal(b.attempts().length, 1);
    assert.equal(b.attempts()[0].id, target.id);
    assert.equal(b.attempts()[0].inspected, false);
    assert.match(b.$('prompt-attempt-status').textContent, /1 unresolved/);
  } finally { b.restore(); }
});

test('same opaque ID under a different team is not an inspection of the original attempt', async () => {
  const b = await browser([target, second]);
  try {
    await uncertainA(b);
    const changedTeam = {...target, team: 'changed-team'};
    await b.fleet(); await b.respond('/api/v1/conversations', list([changedTeam, second]));
    b.open(); await b.respond(readPath, messages(changedTeam));
    b.$('read-conversation').emit('click'); await b.respond(readPath, messages(changedTeam));
    assert.equal(b.attempts()[0].team, target.team);
    assert.equal(b.attempts()[0].inspected, false);
    b.draft('independent team'); assert.equal(b.$('send-prompt').disabled, false);
    assert.match(b.$('prompt-attempt-status').textContent, /1 unresolved/);
  } finally { b.restore(); }
});

test('verified root removal or conflicting verification fences a pending token without posting or retargeting', async () => {
  for (const change of ['remove-root', 'verify-other', 'replace-reference']) {
    const endpoints = [{id: 'desktop', label: 'Fleet', connection: 'reachable', last_attempt_at_ms: Date.now(), observed_at_ms: Date.now(), error: null,
      agents: [{id: 'e'.repeat(64), pane_name: 'Original root', title: 'Original root', provider: 'djinn', project: '', activity: 'unknown', evidence: 'kitsune_snapshot', observed_at_ms: Date.now(), output_available: false}]}];
    const b = await browser([target], endpoints);
    try {
      b.open(); b.draft('must not send'); b.send();
      if (change === 'remove-root') endpoints[0].agents = [];
      await b.fleet();
      const next = change === 'verify-other' ? {...target, association_status: 'verified',
        card: {fleet_endpoint_id: 'other', fleet_agent_id: 'f'.repeat(64), binding_id: 'b'.repeat(64)}}
        : change === 'replace-reference' ? {...target, id: 'd'.repeat(64)} : target;
      await b.respond('/api/v1/conversations', list([next]));
      assert.equal(b.$('conversation-dialog').open, false);
      assert.equal(b.$('prompt-text').value, '');
      await b.respond('/api/v1/csrf', token);
      await b.respond(readPath, messages());
      assert.equal(b.posts().length, 0);
      assert.equal(b.$('conversation-messages').textContent, '');
      if (change !== 'remove-root') {
        b.$('agent-grid').children[0].children[0].emit('click');
        assert.equal(b.$('conversation-dialog').open, change === 'replace-reference');
        assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, change === 'replace-reference' ? 2 : 1);
      }
    } finally { b.restore(); }
  }
});
