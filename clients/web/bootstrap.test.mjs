import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const target = { id: 'a'.repeat(64), label: 'Private target', team: 'research', host: 'Local native' };
const text = '<script>inert</script>\u001b[31m';
const messages = () => ({contract: 'coven.conversations.v1', id: target.id, team: target.team,
  source: 'djinn.conversation-read.v1', messages: [{role: 'assistant', text, truncated: false}],
  has_older: false, truncated: false, omitted_parts: false, projection_sha256: 'b'.repeat(64)});
const list = targets => ({contract: 'coven.conversations.v1', conversations: targets});
const agentID = 'e'.repeat(64);
const bindingID = 'f'.repeat(64);
const linked = (changes = {}) => ({...target, association_status: 'verified',
  card: {fleet_agent_id: agentID, fleet_endpoint_id: 'desktop', binding_id: bindingID}, ...changes});
const readPath = `/api/v1/agents/${agentID}/conversations/${target.id}/bindings/${bindingID}/messages`;
const endpoints = () => ['desktop', 'other'].map((id, index) => ({id, label: 'Same host', connection: 'reachable',
  last_attempt_at_ms: Date.now(), observed_at_ms: Date.now(), error: null,
  agents: [{id: index ? 'd'.repeat(64) : agentID, title: target.label, provider: 'djinn', project: '', activity: 'unknown',
    observed_at_ms: Date.now(), evidence: 'kitsune_snapshot', output_available: false}]}));
const flush = () => new Promise(resolve => setImmediate(resolve));
let serial = 0;

// Execute the real bootstrap with its actual event handlers. Fetch deliberately
// ignores abort so generation guards, not just AbortController, must fence replies.
async function browser(rows = [linked()], fleetEndpoints = endpoints(), metadata = {}) {
  const modals = [];
  const closeEvents = new Set();
  class Node {
    children = []; listeners = new Map(); dataset = {}; value = ''; open = false; text = '';
    parentNode = null; root = false; opener = null;
    get isConnected() { return this.root || Boolean(this.parentNode?.isConnected); }
    contains(node) { return node === this || this.children.some(child => child.contains(node)); }
    focus() {
      if (this.isConnected && !this.hidden && !this.disabled && (!modals.length || modals.at(-1).contains(this))) document.activeElement = this;
    }
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
    showModal() {
      this.opener = document.activeElement; this.open = true; modals.push(this);
      const close = this === nodes.get('conversation-dialog') ? nodes.get('close-conversation') : nodes.get('close-detail');
      close.focus();
    }
    close() {
      const wasOpen = this.open; this.open = false;
      if (wasOpen) {
        modals.splice(modals.indexOf(this), 1);
        // A disconnected opener cannot receive native return focus. Close events
        // are queued after the native focus steps, not dispatched synchronously.
        if (this.contains(document.activeElement)) document.activeElement = null;
        this.opener?.focus();
        const event = setImmediate(() => { closeEvents.delete(event); this.emit('close'); });
        closeEvents.add(event);
      }
    }
    setAttribute() {}
    querySelector(selector) {
      return this.children.find(child => child.className?.split(' ').includes(selector.slice(1)))
        ?? this.children.map(child => child.querySelector(selector)).find(Boolean) ?? null;
    }
    remove() {
      if (this.contains(document.activeElement)) document.activeElement = null;
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
  document.activeElement = null;
  for (const node of nodes.values()) node.root = true;
  for (const match of html.matchAll(/<dialog id="([^"]+)"[^>]*>([\s\S]*?)<\/dialog>/g)) {
    const dialog = nodes.get(match[1]);
    for (const id of match[2].matchAll(/id="([^"]+)"/g)) {
      const child = nodes.get(id[1]); child.root = false; dialog.append(child);
    }
  }
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
  // Old assertions address the native list root; the production root is now the one grid.
  const $ = id => document.getElementById(id === 'conversation-list' ? 'agent-grid' : id);
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
      refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: fleetEndpoints});
    await respond('/api/v1/conversations', {...list(rows), ...metadata});
  } catch (error) { restore(); throw error; }
  function restore() {
    for (const event of closeEvents) clearImmediate(event);
    closeEvents.clear();
    Date.now = originalNow;
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key];
    }
  }
  return { $, document, requests, pending, respond, restore,
    open: () => {
      $('agent-grid').children[0]?.children[0].emit('click');
    },
    tick: ms => { now += ms; for (const handler of intervals) handler(); },
    fleet: async (status = 200) => {
      $('refresh').emit('click');
      await respond('/api/v1/fleet', status === 200 ? {contract: 'coven.fleet-observer.v1', generated_at_ms: now,
        refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: fleetEndpoints} : {error: {code: 'access_denied'}}, status);
    },
  };
}

test('legacy Djinn decoration is absent from card, chat and details headings while the exact route stays pinned', async () => {
  const fleet = endpoints();
  fleet[0].agents[0].title = 'OC | <script>Actual session</script>';
  const b = await browser([linked()], fleet);
  try {
    const card = b.$('agent-grid').children[0];
    assert.equal(card.querySelector('.card-title').textContent, '<script>Actual session</script>');
    assert.equal(card.querySelector('.card-title').innerHTML, undefined);
    b.open();
    assert.equal(b.$('conversation-title').textContent, '<script>Actual session</script> / research');
    assert.ok(b.pending(readPath));
    await b.respond(readPath, messages());
    b.$('close-conversation').emit('click'); await flush();
    card.querySelector('.card-details').emit('click');
    assert.equal(b.$('detail-title').textContent, '<script>Actual session</script>');
    assert.equal(fleet[0].agents[0].title, 'OC | <script>Actual session</script>');
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.requests.filter(r => r.options.method === 'POST').length, 0);
  } finally { b.restore(); }
});

test('one grid merges only verified exact endpoint identity and primary click uses the pinned bound route', async () => {
  const b = await browser([linked()], endpoints());
  const path = `/api/v1/agents/${agentID}/conversations/${target.id}/bindings/${bindingID}/messages`;
  try {
    const grid = b.$('agent-grid');
    assert.equal(grid.children.length, 2);
    assert.match(grid.children[0].textContent, /Chat available/);
    assert.match(grid.children[1].textContent, /Chat unavailable/);
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
    b.open(); assert.ok(b.pending(path));
    assert.equal(b.$('output-dialog').open, false);
    await b.respond(path, messages());
    assert.match(b.$('conversation-messages').textContent, /inert/);
    assert.equal(b.requests.some(r => /\/output$/.test(r.path)), false);
  } finally { b.restore(); }
});

test('binding A-to-B-to-A fences deferred reads and retains focused opaque card nodes without prefetch', async () => {
  const b = await browser([linked()], endpoints());
  const path = `/api/v1/agents/${agentID}/conversations/${target.id}/bindings/${bindingID}/messages`;
  try {
    const primary = b.$('agent-grid').children[0].children[0]; primary.focus(); b.open();
    b.$('prompt-text').value = 'discard this draft';
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked({card: {...linked().card, binding_id: 'c'.repeat(64)}})]));
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
    assert.equal(b.$('agent-grid').children[0].children[0], primary);
    assert.equal(b.document.activeElement, b.$('close-conversation'));
    assert.equal(b.$('prompt-text').value, '');
    await b.respond(path, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.requests.filter(r => r.options.method === 'POST').length, 0);
  } finally { b.restore(); }
});

test('bound reads reject mismatched opaque conversation or team and never fall back to a native read', async () => {
  for (const change of [{id: 'b'.repeat(64)}, {team: 'different-team'}]) {
    const b = await browser([linked()], endpoints());
    try {
      b.open();
      await b.respond(`/api/v1/agents/${agentID}/conversations/${target.id}/bindings/${bindingID}/messages`, {...messages(), ...change});
      assert.equal(b.$('conversation-messages').textContent, '');
      assert.match(b.$('conversation-status').textContent, /refused, unavailable or invalid/);
      assert.equal(b.requests.some(r => r.path === `/api/v1/conversations/${target.id}/messages`), false);
      assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
    } finally { b.restore(); }
  }
});

test('a server-refused stale bound read never falls back or follows another selected conversation', async () => {
  const b = await browser();
  try {
    b.open();
    await b.respond(readPath, {error: {code: 'binding_unavailable'}}, 503);
    assert.match(b.$('conversation-status').textContent, /refused, unavailable or invalid/);
    assert.equal(b.$('conversation-messages').textContent, '');
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked({id: 'c'.repeat(64)})]));
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
  } finally { b.restore(); }
});

test('failed fleet and missing metadata prevent clicks while preserving the original root and Details focus', async () => {
  for (const failure of ['fleet', 'metadata']) {
    const b = await browser();
    try {
      const primary = b.$('agent-grid').children[0].children[0];
      primary.focus();
      await b.fleet(failure === 'fleet' ? 503 : 200);
      await b.respond('/api/v1/conversations', failure === 'metadata' ? {} : list([linked()]), failure === 'metadata' ? 503 : 200);
      primary.emit('click');
      assert.equal(b.$('conversation-dialog').open, false);
      assert.equal(b.document.activeElement, primary);
      assert.equal(b.$('agent-grid').children.length, 2);
      assert.match(b.$('agent-grid').textContent, /Chat unavailable/);
      b.$('agent-grid').children[0].children[1].emit('click');
      assert.match(b.$('detail-chat-status').textContent, /Chat unavailable/);
      b.$('close-detail').emit('click'); await flush();
      assert.equal(b.document.activeElement, b.$('agent-grid').children[0].children[1]);
      assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
    } finally { b.restore(); }
  }
});

test('unavailable, ambiguous and legacy rows disable chat with exactly two fleet cards and accessible Details', async () => {
  for (const row of [target, linked({association_status: 'unavailable'}), linked({association_status: 'ambiguous'})]) {
    const b = await browser([row], endpoints());
    try {
      const grid = b.$('agent-grid'); assert.equal(grid.children.length, 2);
      grid.children[0].children[0].emit('click');
      assert.equal(b.$('conversation-dialog').open, false);
      assert.equal(b.$('output-dialog').open, false);
      assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
      assert.match(grid.children[0].textContent, /Chat unavailable/);
      grid.children[0].children[1].emit('click');
      assert.equal(b.$('output-dialog').open, true);
      assert.match(b.$('detail-chat-status').textContent, /Chat unavailable/);
      b.$('close-detail').emit('click');
      grid.children[0].children[0].emit('click');
      assert.equal(b.$('conversation-dialog').open, false);
      assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 0);
    } finally { b.restore(); }
  }
});

test('global host and team filters use fleet endpoint identity and explicit chat team metadata', async () => {
  const legacy = {...target, id: 'b'.repeat(64), team: 'other'};
  const b = await browser([linked(), legacy], endpoints());
  try {
    const host = b.$('host-filter'), team = b.$('conversation-team'), grid = b.$('agent-grid');
    assert.equal(grid.children.length, 2);
    host.value = 'other'; host.emit('change'); assert.equal(grid.children.length, 1);
    assert.match(grid.textContent, /Chat unavailable/);
    assert.deepEqual(host.children.map(option => option.value), ['', 'desktop', 'other']);
    host.value = ''; host.emit('change'); team.value = target.team; team.emit('change');
    assert.equal(grid.children.length, 1); assert.match(grid.textContent, /Chat available/);
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
    b.document.hidden = true; b.document.emit('visibilitychange');
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
  } finally { b.restore(); }
});

test('source health, list limits and fleet-empty recovery stay visible in the single sessions area', async () => {
  const b = await browser([], [], {sources: [{id: 'source-one', label: 'Native source', team: 'research', status: 'unavailable'}], omitted_conversations: 2});
  try {
    assert.match(b.$('conversation-list-status').textContent, /Some chats unavailable.*Some sessions are not shown/);
    assert.equal(b.$('empty-state').hidden, false);
    assert.match(b.$('empty-description').textContent, /not an inventory/);
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.$('empty-state').hidden, false);
    assert.equal(b.$('agent-grid').children.length, 0);
    assert.equal(b.$('conversation-list-status').textContent, '');
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
  } finally { b.restore(); }
});

test('one refresh still refreshes native-only metadata when the fleet API is unavailable without enabling chat', async () => {
  const b = await browser([target], []);
  try {
    await b.fleet(503);
    await b.respond('/api/v1/conversations', list([target]));
    assert.match(b.$('notice').textContent, /could not be reached/);
    assert.equal(b.$('agent-grid').children.length, 0);
    b.open(); assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
  } finally { b.restore(); }
});

test('bootstrap clears rendered transcripts and all authorized metadata on fleet auth failure', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(readPath, messages());
    assert.equal(b.$('conversation-messages').children[0].children[1].textContent, text);
    assert.equal(b.$('conversation-messages').children[0].children[1].innerHTML, undefined);
    await b.fleet(403);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
    assert.equal(b.$('conversation-team').textContent.includes(target.team), false);
    assert.equal(b.$('conversation-title').textContent, 'Conversation');
    assert.equal(b.$('conversation-dialog').open, false);
    await flush();
    assert.equal(b.document.activeElement, b.$('host-filter'));
  } finally { b.restore(); }
});

test('bootstrap rejects deferred transcript and metadata callbacks after known auth revocation', async () => {
  const b = await browser();
  try {
    b.open();
    const read = b.pending(readPath);
    await b.fleet(); // Starts a metadata-only revalidation while the read is pending.
    await b.fleet(403);
    assert.equal(read.options.signal.aborted, true);
    await b.respond(readPath, messages());
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
      await b.respond(source === 'read' ? readPath
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
    await b.respond(readPath, body);
    await b.fleet(403);
    finish(messages()); await flush();
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 0);
  } finally { b.restore(); }
});

test('bootstrap observes missing resource references through metadata, without transcript polling', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(readPath, messages());
    const reads = () => b.requests.filter(r => r.path.endsWith('/messages')).length;
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
    assert.equal(reads(), 1);
    assert.equal(b.$('conversation-messages').textContent.includes(text), true);
    b.$('read-conversation').emit('click');
    await b.fleet(); await b.respond('/api/v1/conversations', list([{...target, id: 'c'.repeat(64)}]));
    assert.equal(b.$('conversation-dialog').open, false);
    await b.respond(readPath, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(reads(), 2);
  } finally { b.restore(); }
});

test('bootstrap leases expire and visible lifecycle revalidates metadata only', async () => {
  const b = await browser();
  try {
    b.open(); await b.respond(readPath, messages());
    b.tick(15000);
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-list').children.length, 2);
    assert.match(b.$('agent-grid').textContent, /Chat unavailable/);
    await b.respond('/api/v1/conversations', list([linked()]));
    b.document.hidden = true; b.document.emit('visibilitychange');
    assert.equal(b.$('conversation-list').children.length, 2);
    b.document.hidden = false; b.document.emit('visibilitychange');
    await b.respond('/api/v1/conversations', list([]));
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.$('conversation-list').children.length, 2);
  } finally { b.restore(); }
});

test('bootstrap close/reopen and team changes reject actual late read handlers', async () => {
  const b = await browser();
  try {
    b.open(); b.$('close-conversation').emit('click'); b.open();
    await b.respond(readPath, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    b.$('conversation-team').value = target.team; b.$('conversation-team').emit('change');
    await b.respond(readPath, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    assert.equal(b.$('conversation-dialog').open, false);
  } finally { b.restore(); }
});

test('unchanged metadata preserves connected conversation buttons and keyboard focus', async () => {
  const b = await browser([linked()], endpoints());
  try {
    const opener = b.$('agent-grid').children[0].children[0];
    opener.focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
    assert.equal(b.$('agent-grid').children[0].children[0], opener);
    assert.equal(opener.isConnected, true);
    assert.equal(b.document.activeElement, opener);
    // Chat revocation does not remove the underlying fleet observation.
    await b.fleet(); await b.respond('/api/v1/conversations', list([]));
    assert.equal(opener.isConnected, true);
    assert.equal(b.document.activeElement, opener);
    opener.emit('click');
    assert.match(b.$('agent-grid').textContent, /Chat unavailable/);
  } finally { b.restore(); }
});

test('metadata refresh retains the dialog opener for focus restoration on close', async () => {
  const b = await browser([linked()], endpoints());
  try {
    const opener = b.$('agent-grid').children[0].children[0];
    opener.focus(); b.open();
    await b.respond(readPath, messages());
    b.$('close-conversation').focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
    b.$('close-conversation').emit('click');
    await flush();
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.$('agent-grid').children[0].children[0], opener);
    assert.equal(b.document.activeElement, opener);
  } finally { b.restore(); }
});

test('keyboard focus stays on the original root through unavailable-to-verified-to-unlinked changes', async () => {
  const b = await browser([target], endpoints());
  try {
    const grid = b.$('agent-grid');
    const native = grid.children[0].children[0]; native.focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
    const merged = grid.children[0].children[0];
    assert.equal(native.isConnected, true);
    assert.equal(b.document.activeElement, merged);
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.document.activeElement, grid.children[0].children[0]);
    assert.equal(b.document.activeElement, merged);
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
  } finally { b.restore(); }
});

test('focused secondary Details action remains available when a verified association disappears', async () => {
  const b = await browser([linked()], endpoints());
  try {
    const grid = b.$('agent-grid');
    const details = grid.children[0].children[1]; details.focus();
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(details.hidden, false);
    assert.equal(b.document.activeElement, details);
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
  } finally { b.restore(); }
});

test('open verified chat keeps modal focus through binding changes and restores primary on unlink', async () => {
  for (const unlink of [false, true]) {
    const b = await browser([linked()], endpoints());
    try {
      const grid = b.$('agent-grid');
      const oldPrimary = grid.children[0].children[0];
      oldPrimary.focus(); b.open();
      assert.equal(b.document.activeElement, b.$('close-conversation'));
      const path = readPath;
      b.$('prompt-text').value = 'must be cleared by binding fence';
      await b.fleet(); await b.respond('/api/v1/conversations', list([unlink ? target : linked({card: {...linked().card, binding_id: 'c'.repeat(64)}})]));
      const primary = grid.children[0].children[0];
      assert.equal(b.$('conversation-dialog').open, !unlink);
      if (!unlink) {
        assert.equal(b.document.activeElement, b.$('close-conversation'));
        primary.focus(); // Outside a modal is inert, even with programmatic focus.
        assert.equal(b.document.activeElement, b.$('close-conversation'));
      }
      assert.equal(b.$('prompt-text').value, '');
      await b.respond(path, messages());
      assert.equal(b.$('conversation-messages').textContent, '');
      b.$('close-conversation').emit('click'); await flush();
      assert.equal(b.document.activeElement, primary);
      assert.equal(primary, oldPrimary);
      assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
      assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
    } finally { b.restore(); }
  }
});

test('Details modal keeps focus during unlink and returns to the original root Details control', async () => {
  const b = await browser([linked()], endpoints());
  try {
    const grid = b.$('agent-grid');
    const details = grid.children[0].children[1]; details.focus(); details.emit('click');
    assert.equal(b.document.activeElement, b.$('close-detail'));
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.document.activeElement, b.$('close-detail'));
    b.$('close-detail').emit('click'); await flush();
    assert.equal(b.document.activeElement, details);
    assert.equal(details.hidden, false);
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
  } finally { b.restore(); }
});

test('revoked or team-replaced dialog targets return to a safe visible control, not a same-label card', async () => {
  for (const rows of [[], [{...target, team: 'different-team'}]]) {
    const b = await browser([linked()], endpoints());
    try {
      const oldPrimary = b.$('agent-grid').children[0].children[0]; oldPrimary.focus(); b.open();
      b.$('prompt-text').value = 'private draft';
      await b.fleet(); await b.respond('/api/v1/conversations', list(rows));
      assert.equal(b.$('conversation-dialog').open, false);
      await flush();
      assert.equal(b.document.activeElement, oldPrimary);
      assert.equal(b.$('prompt-text').value, '');
      assert.equal(b.$('conversation-messages').textContent, '');
      assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    } finally { b.restore(); }
  }
});

test('healthy fleet with an unmatched configured-host filter does not claim missing host configuration', async () => {
  const b = await browser([linked()], endpoints());
  try {
    b.$('host-filter').value = 'other'; b.$('host-filter').emit('change');
    b.$('conversation-team').value = target.team; b.$('conversation-team').emit('change');
    assert.equal(b.$('empty-state').hidden, false);
    assert.equal(b.$('empty-title').textContent, 'No matching sessions');
    assert.match(b.$('empty-description').textContent, /Team filters exclude/);
    assert.doesNotMatch(b.$('empty-description').textContent, /No hosts are configured/);
    assert.equal(b.$('conversation-list-status').textContent, '');
  } finally { b.restore(); }
});

test('unhealthy empty hosts warn alongside healthy cards only in the selected host scope; recovery and retained cards stay quiet', async () => {
  for (const connection of ['unavailable', 'unsupported']) {
    const hosts = endpoints();
    const retained = hosts[1].agents;
    Object.assign(hosts[1], {label: 'Megatron', connection, agents: []});
    const b = await browser([linked()], hosts);
    try {
      assert.equal(b.$('agent-grid').children.length, 1);
      assert.equal(b.$('empty-state').hidden, true);
      assert.equal(b.$('notice').hidden, false);
      assert.equal(b.$('notice').textContent, `Megatron ${connection}.`);
      b.$('host-filter').value = 'desktop'; b.$('host-filter').emit('change');
      assert.equal(b.$('notice').hidden, true);
      assert.equal(b.$('notice').textContent, '');
      b.$('host-filter').value = 'other'; b.$('host-filter').emit('change');
      assert.equal(b.$('notice').hidden, false);
      b.$('host-filter').value = ''; b.$('host-filter').emit('change');
      hosts[1].agents = retained;
      await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
      assert.equal(b.$('agent-grid').children.length, 2);
      assert.equal(b.$('notice').hidden, true, 'retained cards already carry their host state');
      hosts[1].agents = []; hosts[1].connection = 'reachable'; hosts[0].agents[0].activity = 'blocked';
      await b.fleet(); await b.respond('/api/v1/conversations', list([linked()]));
      assert.equal(b.$('notice').hidden, true, 'recovery and heuristic blocking are not network errors');
      assert.equal(b.$('notice').textContent, '');
      hosts[1].connection = connection;
      await b.fleet(503); await b.respond('/api/v1/conversations', list([linked()]));
      assert.equal(b.$('notice').hidden, false);
      assert.match(b.$('notice').textContent, /could not be reached/);
      assert.doesNotMatch(b.$('notice').textContent, /Megatron/);
      assert.match(b.$('connection-status').textContent, /stale/);
      await b.fleet(403);
      assert.match(b.$('notice').textContent, /Access denied/);
      assert.doesNotMatch(b.$('notice').textContent, /Megatron/);
    } finally { b.restore(); }
  }
});

test('a team filter excludes healthy unassigned terminal observations without claiming no hosts', async () => {
  const b = await browser([target], endpoints());
  try {
    b.$('host-filter').value = 'desktop'; b.$('host-filter').emit('change');
    b.$('conversation-team').value = target.team; b.$('conversation-team').emit('change');
    assert.equal(b.$('agent-grid').children.length, 0);
    assert.equal(b.$('empty-title').textContent, 'No matching sessions');
    assert.match(b.$('empty-description').textContent, /Team filters exclude unassigned terminal-only/);
    assert.doesNotMatch(b.$('empty-description').textContent, /No hosts are configured/);
    assert.equal(b.$('conversation-list-status').textContent, '');
  } finally { b.restore(); }
});

test('empty native metadata, metadata failure and actual zero fleet configuration stay distinct', async () => {
  const b = await browser([], [], {sources: [{id: 'one', label: 'Native source', team: 'research', status: 'unavailable'}]});
  try {
    assert.equal(b.$('empty-state').hidden, false);
    assert.match(b.$('empty-description').textContent, /No hosts are configured.*No authorized native chat sessions/);
    assert.match(b.$('conversation-list-status').textContent, /Some chats unavailable/);
    b.open();
    assert.equal(b.$('conversation-dialog').open, false);
    assert.match(b.$('empty-description').textContent, /No authorized native chat sessions/);
    assert.match(b.$('conversation-list-status').textContent, /Some chats unavailable/);
    await b.fleet(); await b.respond('/api/v1/conversations', {}, 503);
    assert.match(b.$('empty-description').textContent, /Native chat metadata is unavailable or awaiting revalidation/);
    assert.match(b.$('conversation-list-status').textContent, /list unavailable/);
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.equal(b.$('empty-state').hidden, false);
    assert.equal(b.$('agent-grid').children.length, 0);
    assert.equal(b.$('conversation-list-status').textContent, '');
    assert.equal(b.requests.some(r => /messages|output|prompts/.test(r.path)), false);
  } finally { b.restore(); }
});

test('conversation rendering hides blank and non-displayable entries without altering real text', async () => {
  const b = await browser();
  try {
    const userText = '\n  Preserve this indentation.\t\n';
    const response = messages();
    response.omitted_parts = true;
    response.truncated = true;
    response.messages = [
      {role: 'assistant', text: '', truncated: false},
      {role: 'assistant', text: ' \t\r\n\u00a0', truncated: false},
      {role: 'user', text: userText, truncated: false},
      {role: 'assistant', text: '\u200b\u200d\ufeff\u2066\u2069', truncated: false},
      {role: 'assistant', text: '', truncated: true},
      {role: 'assistant', text, truncated: false},
    ];
    b.open(); await b.respond(readPath, response);
    const rows = b.$('conversation-messages').children;
    assert.equal(rows.length, 2);
    assert.equal(rows[0].children[0].textContent, 'User');
    assert.equal(rows[0].children[1].textContent, userText);
    assert.equal(rows[1].children[0].textContent, 'Assistant');
    assert.equal(rows[1].children[1].textContent, text);
    assert.equal(rows[1].children[1].innerHTML, undefined);
    assert.equal(b.$('conversation-status').textContent, 'Some content omitted.');
    assert.equal(response.messages.length, 6); // Filtering must not mutate the native projection.
  } finally { b.restore(); }
});

test('an all-blank sample explains missing displayable text rather than drawing empty assistant cards', async () => {
  const b = await browser();
  try {
    const response = messages();
    response.messages = [{role: 'assistant', text: '', truncated: false},
      {role: 'assistant', text: '\t\n\u200b', truncated: true}];
    response.omitted_parts = true;
    response.truncated = true;
    b.open(); await b.respond(readPath, response);
    const rows = b.$('conversation-messages').children;
    assert.equal(rows.length, 1);
    assert.equal(rows[0].children.length, 0); // Empty-state text, not an assistant heading/pre pair.
    assert.equal(rows[0].textContent, 'No text messages to display.');
    assert.equal(b.$('conversation-status').textContent, 'Some content omitted.');
  } finally { b.restore(); }
});

test('an empty projection has a useful empty state and refresh retains real truncated text', async () => {
  const b = await browser();
  try {
    const empty = messages(); empty.messages = [];
    b.open(); await b.respond(readPath, empty);
    assert.match(b.$('conversation-messages').textContent, /No text messages to display/);
    assert.doesNotMatch(b.$('conversation-status').textContent, /entries without displayable text hidden/);
    b.$('read-conversation').emit('click');
    const response = messages(); response.messages[0].truncated = true;
    await b.respond(readPath, response);
    assert.equal(b.$('conversation-messages').children.length, 1);
    assert.equal(b.$('conversation-messages').children[0].children[0].textContent, 'Assistant / truncated');
    assert.equal(b.$('conversation-messages').children[0].children[1].textContent, text);
    assert.doesNotMatch(b.$('conversation-messages').textContent, /No displayable text/);
  } finally { b.restore(); }
});

test('multiple native history targets do not add cards or introduce a chooser for a verified root', async () => {
  const second = {...target, id: 'b'.repeat(64), team: 'other-team', label: 'Other source'};
  const b = await browser([linked(), second], endpoints());
  try {
    const grid = b.$('agent-grid');
    const a = grid.children[0].children[0], other = grid.children[1].children[0];
    a.focus(); a.emit('click');
    assert.ok(b.pending(readPath));
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.document.activeElement, b.$('close-conversation'));
    other.focus(); assert.equal(b.document.activeElement, b.$('close-conversation'));
    assert.equal(grid.children[0].querySelector('.card-title').textContent, target.label);
    assert.match(b.$('conversation-title').textContent, /Private target.*research/);
    b.$('close-conversation').emit('click'); await flush();
    assert.equal(b.document.activeElement, a);
    other.emit('click');
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.match(grid.children[0].textContent, /Chat available/);
    assert.match(grid.children[1].textContent, /Chat unavailable/);
    assert.equal(grid.children.length, 2);
  } finally { b.restore(); }
});

test('verified roots clear on expiry, hide, auth and reference removal, fencing deferred reads', async () => {
  for (const change of ['expiry', 'hide', 'auth', 'reference']) {
    const b = await browser([linked()], endpoints());
    try {
      b.open();
      const path = readPath;
      if (change === 'expiry') b.tick(15000);
      if (change === 'hide') { b.document.hidden = true; b.document.emit('visibilitychange'); }
      if (change === 'auth') await b.fleet(403);
      if (change === 'reference') { await b.fleet(); await b.respond('/api/v1/conversations', list([{...target, id: 'c'.repeat(64)}])); }
      await b.respond(path, messages());
      assert.equal(b.$('conversation-dialog').open, false);
      assert.equal(b.$('conversation-messages').textContent, '');
      assert.doesNotMatch(b.$('agent-grid').textContent, /Chat available/);
      assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
    } finally { b.restore(); }
  }
});

test('filtered roots cannot be opened through a global chooser or a different unlinked card', async () => {
  const b = await browser([linked()], endpoints());
  try {
    b.$('host-filter').value = 'other'; b.$('host-filter').emit('change');
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    b.open();
    b.tick(1000);
    assert.equal(b.$('conversation-dialog').open, false);
    assert.match(b.$('agent-grid').textContent, /Chat unavailable/);
    b.$('agent-grid').children[0].children[0].emit('click');
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    assert.equal(b.$('agent-grid').children.length, 1);
  } finally { b.restore(); }
});

test('competing exact verified claims fail closed with a concise reason and no read', async () => {
  const second = linked({id: 'b'.repeat(64), label: 'Competing source'});
  const b = await browser([linked(), second], endpoints());
  try {
    b.$('agent-grid').children[0].children[0].emit('click');
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    assert.match(b.$('agent-grid').textContent, /Multiple conversations claim this terminal/);
    b.$('agent-grid').children[0].children[1].emit('click');
    assert.match(b.$('detail-chat-status').textContent, /Multiple conversations claim this terminal/);
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    assert.equal(b.$('agent-grid').children.length, 2);
  } finally { b.restore(); }
});

test('zero fleet with a single shared session remains honest diagnostic metadata across renders', async () => {
  const b = await browser([target], []);
  try {
    b.open();
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.$('agent-grid').children.length, 0);
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    b.tick(1000);
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.$('conversation-list-status').textContent, '');
    assert.equal(b.requests.some(r => r.path.endsWith('/messages')), false);
    assert.equal(b.$('agent-grid').children.length, 0);
  } finally { b.restore(); }
});

test('a contradictory verified chat on the same root closes the old view and never restores or retargets it', async () => {
  const other = linked({id: 'b'.repeat(64), label: 'Verified other chat'});
  const b = await browser([linked()], endpoints());
  try {
    const primary = b.$('agent-grid').children[0].children[0];
    primary.focus(); b.open();
    b.$('prompt-text').value = 'discard';
    await b.fleet(); await b.respond('/api/v1/conversations', list([target, other]));
    assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.$('prompt-text').value, '');
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    await b.respond(readPath, messages());
    assert.equal(b.$('conversation-messages').textContent, '');
    await flush(); assert.equal(b.document.activeElement, primary);
    assert.match(b.$('agent-grid').children[0].textContent, /Chat available/);
    await b.fleet(); await b.respond('/api/v1/conversations', list([target]));
    assert.match(b.$('agent-grid').textContent, /Chat unavailable/);
    primary.emit('click'); assert.equal(b.$('conversation-dialog').open, false);
    assert.equal(b.requests.filter(r => r.path.endsWith('/messages')).length, 1);
    assert.equal(b.requests.some(r => r.options.method === 'POST'), false);
  } finally { b.restore(); }
});
