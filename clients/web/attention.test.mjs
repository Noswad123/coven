import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
import {validateAttention, attentionLabel, validateDecision, questionAnswers} from './app.js';

const hex = c => c.repeat(64);
const item = {id: hex('1'), alert_id: hex('2'), conversation_id: hex('a'), fleet_agent_id: hex('e'),
  fleet_endpoint_id: 'desktop', binding_id: hex('f'), team: 'research', kind: 'question', can_respond: true, details_available: true};
const metadata = (items = [item], extra = {}) => ({contract: 'coven.attention.v1', state: 'observed', items,
  omitted_count: 0, unavailable_count: 0, unlinked_count: 0, ...extra});
const question = {kind: 'question', questions: [{question: '<script>Full question</script>', header: 'Choose',
  options: [{label: 'One', description: '<b>literal</b>'}, {label: 'Two', description: 'Complete description'}], multiple: false, custom: true}]};
const detail = (row = item, decision = question) => ({contract: 'coven.decision.v1', id: row.id, alert_id: row.alert_id,
  kind: row.kind, can_respond: row.can_respond, decision});
const token = {contract: 'coven.csrf.v1', token: `0-${hex('a')}-${hex('b')}`};
const flush = () => new Promise(resolve => setImmediate(resolve));
async function waitFor(predicate, message = 'fixture completion timed out') {
  const deadline = Date.now() + 2000;
  while (!predicate() && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 2));
  assert.ok(predicate(), message);
}
const app = await readFile(new URL('./app.js', import.meta.url), 'utf8');
const html = await readFile(new URL('./index.html', import.meta.url), 'utf8');

// Each bootstrap has its own VM globals, including radio fields and timers.
async function browser({rows = [item], push = {}, hash = '', attentionExtra = {}} = {}) {
  let document;
  class Node {
    children = []; listeners = new Map(); dataset = {}; value = ''; open = false; text = ''; checked = false;
    constructor(tag = '') { this.tagName = tag; }
    get textContent() { return this.text + this.children.map(n => n.textContent).join(''); }
    set textContent(v) { this.replaceChildren(); this.text = v; }
    append(...nodes) { nodes.forEach(n => { n.parentNode = this; this.children.push(n); }); }
    replaceChildren(...nodes) { this.children.forEach(n => { n.parentNode = null; }); this.children = []; this.text = ''; this.append(...nodes); }
    contains(n) { return n === this || this.children.some(c => c.contains(n)); }
    focus() { document.activeElement = this; }
    addEventListener(e, h) { const handlers = this.listeners.get(e) ?? []; handlers.push(h); this.listeners.set(e, handlers); }
    emit(e) { this.listeners.get(e)?.forEach(h => h({target: this, preventDefault() {}})); }
    showModal() { this.open = true; }
    close() { const was = this.open; this.open = false; if (was) this.emit('close'); }
    setAttribute() {}
    querySelector(s) { return this.all().find(n => s.startsWith('.') ? n.className?.split(' ').includes(s.slice(1)) : n.tagName === s) ?? null; }
    all() { return this.children.flatMap(n => [n, ...n.all()]); }
    remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); this.parentNode = null; }
    insertBefore(n, before) { n.remove(); n.parentNode = this; this.children.splice(before ? this.children.indexOf(before) : this.children.length, 0, n); }
  }
  const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(m => [m[1], new Node()]));
  document = new Node(); document.hidden = false;
  document.getElementById = id => { assert.ok(nodes.has(id), `missing ${id}`); return nodes.get(id); };
  document.createElement = tag => new Node(tag);
  const requests = [], intervals = [], timers = new Map(); let timer = 0, now = Date.now();
  class Clock extends Date { static now() { return now; } }
  const context = vm.createContext({document, Date: Clock, TextEncoder, AbortController, URL, console,
    crypto: webcrypto, atob, location: {hash}, ...push,
    setTimeout: (handler, ms) => { timers.set(++timer, {handler, ms}); return timer; }, clearTimeout: id => timers.delete(id),
    setInterval: h => intervals.push(h),
    fetch: (path, options) => new Promise((resolve, reject) => requests.push({path, options, resolve, reject, done: false})),
  });
  const $ = id => nodes.get(id);
  const pending = path => requests.find(r => !r.done && r.path === path);
  const respond = async (path, value, status = 200) => {
    await waitFor(() => pending(path), `no pending ${path}`);
    const r = pending(path); assert.ok(r, `no pending ${path}`); r.done = true;
    r.resolve({ok: status === 200, status, json: async () => JSON.parse(JSON.stringify(value))}); await flush(); return r;
  };
  const list = {contract: 'coven.conversations.v1', conversations: [{id: item.conversation_id, label: 'Native', team: 'research', host: 'Local native',
    can_submit: false, association_status: 'verified', card: {fleet_agent_id: item.fleet_agent_id, fleet_endpoint_id: 'desktop', binding_id: item.binding_id}}]};
  const fleet = () => ({contract: 'coven.fleet-observer.v1', generated_at_ms: now, refresh_interval_ms: 3000, stale_after_ms: 15000,
    endpoints: [{id: 'desktop', label: 'Host', connection: 'reachable', last_attempt_at_ms: now, observed_at_ms: now, error: null,
      agents: ['e', 'd'].map(c => ({id: hex(c), title: 'Original root', provider: 'djinn', project: 'Project', activity: 'idle',
        evidence: 'kitsune_snapshot', observed_at_ms: now, output_available: false}))}]});
  vm.runInContext(app.replace(/^export /gm, ''), context);
  await respond('/api/v1/fleet', fleet()); await respond('/api/v1/conversations', list); await respond('/api/v1/attention', metadata(rows, attentionExtra));
  return {$, document, context, requests, pending, respond,
    open: () => $('agent-grid').children.find(n => n.textContent.includes('Open conversation')).children[0].emit('click'),
    fields: () => $('decision-details').all().filter(n => n.tagName === 'input'),
    button: text => $('decision-details').all().find(n => n.tagName === 'button' && n.textContent === text),
    tick: ms => { now += ms; intervals.forEach(h => h()); },
    deadline: async () => { for (const [id, timer] of [...timers]) if (timer.ms === 10000) { timers.delete(id); timer.handler(); } await flush(); },
    refresh: async (rows, extra) => { $('refresh').emit('click'); await respond('/api/v1/fleet', fleet()); await respond('/api/v1/conversations', list); await respond('/api/v1/attention', metadata(rows, extra)); },
    lose: async path => { const r = pending(path); assert.ok(r); r.done = true; r.reject(new Error('lost')); await flush(); },
    posts: () => requests.filter(r => r.options.method === 'POST'),
  };
}

test('attention metadata is independent of heuristic activity; unknown and partial never mean zero', () => {
  assert.equal(attentionLabel(null), 'Attention unknown');
  assert.equal(attentionLabel(validateAttention(metadata([]))), 'No pending requests observed');
  for (const extra of [{state: 'partial'}, {omitted_count: null}, {unlinked_count: 1}, {unavailable_count: 1}]) {
    assert.match(attentionLabel(validateAttention(metadata([], extra))), /unknown/);
    assert.match(attentionLabel(validateAttention(metadata([item], extra))), /partial/);
  }
  assert.equal(attentionLabel(metadata([item, item])), '1 needs response');
  assert.throws(() => validateAttention(metadata([item, item])));
  assert.throws(() => validateAttention(metadata([{...item, id: 'native-id'}])));
  validateAttention(metadata([item], {streams: [{id: hex('5'), complete: false, alert_ids: [item.alert_id]}]}));
});

test('full detail validation rejects selectors, incomplete patterns, oversized content and hidden tool IDs', () => {
  assert.equal(validateDecision(detail(), item).decision.questions.length, 1);
  for (const decision of [{...question, tool: {call_id: 'secret'}}, {...question, request_id: 'raw'},
    {...question, questions: [{...question.questions[0], question: 'x'.repeat(4097)}]}]) assert.throws(() => validateDecision(detail(item, decision), item));
  const row = {...item, kind: 'permission'};
  const permission = {kind: 'permission', permission: 'shell', patterns: ['all complete patterns'], always_patterns: ['native only'], metadata_json: '{}', deny_scope: 'session_pending'};
  validateDecision(detail(row, permission), row);
  for (const change of [{deny_scope: 'future'}, {patterns: null}, {metadata_json: 'x'.repeat(4097)}]) assert.throws(() => validateDecision(detail(row, {...permission, ...change}), row));
});

test('question answers preserve ordered single/multiple/custom selections with UTF8 and duplicate limits', () => {
  const qs = [{...question.questions[0], custom: false}, {...question.questions[0], multiple: true}];
  const fields = [{options: [{checked: true, value: 'One'}]}, {options: [{checked: true, value: 'Two'}], custom: {value: 'custom\nanswer'}}];
  assert.deepEqual(questionAnswers(qs, fields), [['One'], ['Two', 'custom\nanswer']]);
  fields[0].options[0].checked = false; assert.throws(() => questionAnswers(qs, fields));
  fields[0].options[0].checked = true; fields[1].custom.value = 'Two'; assert.throws(() => questionAnswers(qs, fields));
  fields[1].custom.value = '\u00e9'.repeat(2049); assert.throws(() => questionAnswers(qs, fields));
});

test('first primary card remains one click; metadata polls never read details and form renders inert complete text', async () => {
  const b = await browser();
  assert.equal(b.$('agent-grid').children.length, 2);
  assert.match(b.$('agent-grid').textContent, /Needs response/);
  assert.equal(b.requests.some(r => r.path.startsWith('/api/v1/decisions/')), false);
  b.open(); assert.equal(b.$('conversation-dialog').open, true);
  await b.respond(`/api/v1/decisions/${item.id}`, detail());
  assert.match(b.$('decision-details').textContent, /<script>Full question<\/script>/);
  assert.match(b.$('decision-details').textContent, /<b>literal<\/b>/);
  assert.equal(b.fields()[0].type, 'radio'); assert.equal(b.fields()[0].name, 'question-0');
  assert.notEqual(b.fields()[0].id, b.fields()[1].id);
  assert.equal(b.$('prompt-composer').hidden, true);
  b.fields()[0].checked = true; b.button('Answer once').emit('click');
  assert.equal(b.posts().length, 0); await b.respond('/api/v1/csrf', token);
  const post = b.posts()[0], body = JSON.parse(post.options.body);
  assert.deepEqual(body.action, {type: 'question_answer', answers: [['One']]});
  assert.equal(b.fields().length, 0);
  await b.respond(post.path, {contract: 'coven.decision-response.v1', id: item.id, attempt_id: body.attempt_id, action: body.action.type, outcome: 'applied', delivery: 'applied'});
  assert.match(b.$('decision-status').textContent, /native wait settled, not tool completion/);
});

test('reader sees full read-only detail independent of follow-up submission permission', async () => {
  const row = {...item, can_respond: false}; const b = await browser({rows: [row]}); b.open();
  await b.respond(`/api/v1/decisions/${item.id}`, detail(row));
  assert.match(b.$('decision-status').textContent, /Read-only/);
  assert.ok(b.fields().every(f => f.disabled)); assert.equal(b.button('Answer once'), undefined);
  assert.equal(b.posts().length, 0);
});

test('permission details disclose all patterns and denial scope with only allow-once/deny actions', async () => {
  const row = {...item, kind: 'permission'}; const b = await browser({rows: [row]}); b.open();
  await b.respond(`/api/v1/decisions/${item.id}`, detail(row, {kind: 'permission', permission: 'shell', patterns: ['a', '<path>'],
    always_patterns: ['native-rule'], metadata_json: '{"command":"literal"}', deny_scope: 'session_pending'}));
  assert.match(b.$('decision-details').textContent, /<path>/); assert.match(b.$('decision-details').textContent, /not future/);
  assert.equal(b.button('Always'), undefined); b.button('Deny pending session permissions').emit('click');
  await b.respond('/api/v1/csrf', token);
  assert.deepEqual(JSON.parse(b.posts()[0].options.body).action, {type: 'permission_deny', scope: 'session_pending'});
});

test('stale occurrence fences late detail, clears drafts and never retargets a newer request', async () => {
  const b = await browser(); b.open();
  const newer = {...item, id: hex('3'), alert_id: hex('4')};
  await b.refresh([newer]); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  assert.equal(b.$('decision-details').textContent, '');
  assert.equal(b.requests.some(r => r.path === `/api/v1/decisions/${newer.id}`), false);
  assert.equal(b.posts().length, 0);
});

test('metadata ACL revocation and hide fence token callbacks before POST', async () => {
  for (const hide of [false, true]) {
    const b = await browser(); b.open(); await b.respond(`/api/v1/decisions/${item.id}`, detail());
    b.fields()[0].checked = true; b.button('Answer once').emit('click');
    if (hide) { b.document.hidden = true; b.document.emit('visibilitychange'); }
    else await b.refresh([{...item, can_respond: false}]);
    await b.respond('/api/v1/csrf', token); assert.equal(b.posts().length, 0); assert.equal(b.$('decision-details').textContent, '');
  }
});

test('uncertainty is per original occurrence/attempt, contains no answers and a different applied request cannot clear it', async () => {
  const b = await browser(); b.open(); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  b.fields()[0].checked = true; b.button('Answer once').emit('click'); await b.respond('/api/v1/csrf', token);
  const post = b.posts()[0]; await b.lose(post.path);
  assert.match(b.$('decision-attempt-status').textContent, /1 unresolved/);
  assert.doesNotMatch(b.$('decision-attempt-status').textContent, /One/);
  b.$('decision-requests').children[0].emit('click'); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  assert.equal(b.button('Answer once').disabled, true);
  assert.match(b.$('decision-details').textContent, /separately choose/);
  const newer = {...item, id: hex('3'), alert_id: hex('4')}; await b.refresh([newer]);
  b.$('decision-requests').children[0].emit('click'); await b.respond(`/api/v1/decisions/${newer.id}`, detail(newer));
  b.button('Reject question').emit('click'); await b.respond('/api/v1/csrf', token);
  const p = b.posts()[1], body = JSON.parse(p.options.body);
  await b.respond(p.path, {contract: 'coven.decision-response.v1', id: newer.id, attempt_id: body.attempt_id, action: 'question_reject', outcome: 'applied', delivery: 'applied'});
  assert.match(b.$('decision-attempt-status').textContent, /1 unresolved/); assert.equal(b.posts().length, 2);
});

test('notification navigation verifies stable alert against current exact root, never a newer latest request', async () => {
  const b = await browser({hash: `#attention=${item.alert_id}`});
  assert.equal(b.$('conversation-dialog').open, true);
  assert.ok(b.pending(`/api/v1/decisions/${item.id}`)); assert.equal(b.posts().length, 0);
  const missing = await browser({rows: [{...item, id: hex('3'), alert_id: hex('4')}], hash: `#attention=${item.alert_id}`});
  assert.match(missing.$('attention-status').textContent, /Cannot currently verify/);
  assert.equal(missing.$('conversation-dialog').open, false);
  assert.equal(missing.requests.some(r => r.path.startsWith('/api/v1/decisions/')), false);
});

test('alerts never request permission or register at page load; unsupported and denied are honest', async () => {
  const b = await browser(); b.$('enable-alerts').emit('click');
  assert.match(b.$('alerts-status').textContent, /unsupported/);
  let prompts = 0, registrations = 0;
  const denied = await browser({push: {Notification: {requestPermission: async () => { prompts++; return 'denied'; }}, PushManager: class {},
    navigator: {serviceWorker: {register: async () => { registrations++; }}}}});
  assert.equal(prompts, 0); assert.equal(registrations, 0); denied.$('enable-alerts').emit('click'); await flush();
  assert.equal(prompts, 1); assert.equal(registrations, 0); assert.match(denied.$('alerts-status').textContent, /denied/);
});

test('explicit push opt-in and disable use browser-only keys and fresh CSRF for each mutation', async () => {
  let permissions = 0, subscriptions = 0, unsubscribed = 0, registrations = 0;
  const sub = {endpoint: 'https://push.services.mozilla.com/fixture', toJSON: () => ({keys: {p256dh: 'browser-key', auth: 'browser-auth'}}), unsubscribe: async () => { unsubscribed++; return true; }};
  const id = Buffer.from(await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(sub.endpoint))).toString('hex');
  const registration = {pushManager: {getSubscription: async () => subscriptions ? sub : null, subscribe: async options => { assert.equal(options.userVisibleOnly, true); subscriptions++; return sub; }}};
  const b = await browser({push: {Notification: {requestPermission: async () => { permissions++; return 'granted'; }}, PushManager: class {},
    navigator: {serviceWorker: {ready: Promise.resolve(registration), register: async (path, options) => { assert.equal(path, '/service-worker.js'); assert.equal(options.scope, '/'); registrations++; return registration; }, getRegistration: async () => registration}}}});
  b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: []});
  await b.respond('/api/v1/csrf', token);
  const body = JSON.parse(b.posts()[0].options.body); assert.deepEqual(body.subscription, {endpoint: sub.endpoint, keys: sub.toJSON().keys});
  await b.respond('/api/v1/push/subscriptions', {contract: 'coven.web-push.v1', subscription_id: id, subscribed: true});
  assert.equal(permissions, 1); assert.equal(registrations, 1); assert.match(b.$('alerts-status').textContent, /enabled/);
  b.$('disable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: [id]});
  await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: id, subscribed: false});
  assert.equal(unsubscribed, 1); assert.equal(b.requests.filter(r => r.path === '/api/v1/csrf').length, 2);
  assert.match(b.$('alerts-status').textContent, /disabled/);
});

test('a late applied receipt after closing a decision remains uncertain, never restores answers', async () => {
  const b = await browser(); b.open(); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  b.fields()[0].checked = true; b.button('Answer once').emit('click'); await b.respond('/api/v1/csrf', token);
  const post = b.posts()[0], body = JSON.parse(post.options.body);
  b.$('close-conversation').emit('click');
  await b.respond(post.path, {contract: 'coven.decision-response.v1', id: item.id, attempt_id: body.attempt_id,
    action: 'question_answer', outcome: 'applied', delivery: 'applied'});
  assert.equal(b.$('decision-details').textContent, ''); assert.equal(b.$('conversation-dialog').open, false);
  assert.match(b.$('decision-attempt-status').textContent, /1 unresolved/); assert.equal(b.posts().length, 1);
  b.open(); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  assert.equal(b.button('Answer once').disabled, true);
  assert.equal(b.fields().some(f => f.checked), false);
});

test('grant refresh can reauthorize the same stable alert but cannot erase its uncertain attempt', async () => {
  const b = await browser(); b.open(); await b.respond(`/api/v1/decisions/${item.id}`, detail());
  b.button('Reject question').emit('click'); await b.respond('/api/v1/csrf', token); await b.lose(b.posts()[0].path);
  const refreshed = {...item, id: hex('3')}; await b.refresh([refreshed]);
  b.$('decision-requests').children[0].emit('click'); await b.respond(`/api/v1/decisions/${refreshed.id}`, detail(refreshed));
  assert.equal(b.button('Reject question').disabled, true);
  const deliberate = b.fields().find(f => f.type === 'checkbox'); deliberate.checked = true; deliberate.emit('change');
  assert.equal(b.button('Reject question').disabled, false); assert.equal(b.posts().length, 1);
  assert.match(b.$('decision-attempt-status').textContent, /1 unresolved/);
});

test('unsupported request details remain visible as source-only metadata without a speculative read', async () => {
  const b = await browser({rows: [{...item, details_available: false}]}); b.open();
  assert.equal(b.$('decision-requests').children[0].disabled, true);
  assert.match(b.$('decision-requests').textContent, /use source UI/);
  assert.equal(b.requests.some(r => r.path.startsWith('/api/v1/decisions/')), false);
  assert.match(b.$('attention-status').textContent, /1 needs response/);
});

test('existing owned subscriptions are inspected on reload without permission, registration or resubscription', async () => {
  let actions = 0;
  const endpoint = 'https://push.services.mozilla.com/reload-fixture';
  const id = Buffer.from(await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(endpoint))).toString('hex');
  const registration = {pushManager: {getSubscription: async () => ({endpoint}), subscribe: () => { actions++; }}};
  const b = await browser({push: {Notification: {permission: 'granted', requestPermission: () => { actions++; }}, PushManager: class {},
    navigator: {serviceWorker: {getRegistration: async () => registration, register: () => { actions++; }}}}});
  await b.respond('/api/v1/push/config', {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: [id]});
  for (let n = 0; n < 100 && b.$('disable-alerts').hidden !== false; n++) await flush();
  assert.equal(actions, 0); assert.equal(b.posts().length, 0); assert.equal(b.$('disable-alerts').hidden, false);
  assert.match(b.$('alerts-status').textContent, /enabled/);
});

test('explicit re-enable replaces an unowned browser subscription rather than attaching it to another principal', async () => {
  let removed = 0, created = 0;
  const old = {endpoint: 'https://push.services.mozilla.com/other-owner', unsubscribe: async () => { removed++; return true; }};
  const fresh = {endpoint: 'https://push.services.mozilla.com/fresh', toJSON: () => ({keys: {p256dh: 'fresh', auth: 'fresh'}})};
  const registration = {pushManager: {getSubscription: async () => old, subscribe: async () => { created++; return fresh; }}};
  const b = await browser({push: {Notification: {requestPermission: async () => 'granted'}, PushManager: class {},
    navigator: {serviceWorker: {ready: Promise.resolve(registration), register: async () => registration}}}});
  assert.equal(removed, 0); assert.equal(created, 0);
  b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: []});
  await b.respond('/api/v1/csrf', token);
  assert.equal(removed, 1); assert.equal(created, 1);
  assert.equal(JSON.parse(b.posts()[0].options.body).subscription.endpoint, fresh.endpoint);
});

test('known authentication loss fences a pending subscription token before any registration POST', async () => {
  const sub = {endpoint: 'https://push.services.mozilla.com/fixture', toJSON: () => ({keys: {p256dh: 'key', auth: 'key'}})};
  const registration = {pushManager: {getSubscription: async () => null, subscribe: async () => sub}};
  const b = await browser({push: {Notification: {requestPermission: async () => 'granted'}, PushManager: class {},
    navigator: {serviceWorker: {ready: Promise.resolve(registration), register: async () => registration}}}});
  b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: []});
  for (let n = 0; n < 100 && !b.pending('/api/v1/csrf'); n++) await flush();
  assert.ok(b.pending('/api/v1/csrf'));
  b.$('refresh').emit('click'); await b.respond('/api/v1/fleet', {error: {code: 'access_denied'}}, 403);
  await b.respond('/api/v1/csrf', token);
  assert.equal(b.posts().length, 0); assert.match(b.$('alerts-status').textContent, /unverified/);
});

test('an initially missing partial alert retries exact verification on refresh, never opens a newer occurrence and stays dismissed after success', async () => {
  const b = await browser({rows: [], attentionExtra: {state: 'partial', omitted_count: null}, hash: `#attention=${item.alert_id}`});
  assert.match(b.$('attention-status').textContent, /Cannot currently verify/);
  assert.equal(b.$('conversation-dialog').open, false);
  await b.refresh([{...item, id: hex('3'), alert_id: hex('4')}]);
  assert.equal(b.$('conversation-dialog').open, false);
  assert.equal(b.requests.some(r => r.path.startsWith('/api/v1/decisions/')), false);
  const reauthorized = {...item, id: hex('5')};
  await b.refresh([reauthorized]);
  assert.equal(b.$('conversation-dialog').open, true);
  await b.respond(`/api/v1/decisions/${reauthorized.id}`, detail(reauthorized));
  b.$('close-conversation').emit('click');
  await b.refresh([reauthorized]); b.tick(1000);
  assert.equal(b.$('conversation-dialog').open, false);
  assert.equal(b.requests.filter(r => r.path.startsWith('/api/v1/decisions/')).length, 1);
  assert.equal(b.posts().length, 0);
});

async function alertsFixture({local = true, stall = '', passive = false} = {}) {
  let removals = 0, hashes = 0, present = stall !== 'subscribe', finishRemoval;
  const endpoint = 'https://push.services.mozilla.com/disable-fixture';
  const id = Buffer.from(await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(endpoint))).toString('hex');
  const never = () => new Promise(() => {});
  const sub = {endpoint, toJSON: () => ({keys: {p256dh: 'browser', auth: 'browser'}}), unsubscribe: async () => {
    removals++;
    if (stall === 'unsubscribe') return new Promise(resolve => { finishRemoval = () => { present = false; resolve(true); }; });
    if (local) present = false; return local;
  }};
  const registration = {pushManager: {getSubscription: async () => stall === 'getSubscription' ? never() : present ? sub : null,
    subscribe: async () => stall === 'subscribe' ? never() : sub}};
  const b = await browser({push: {crypto: {randomUUID: () => webcrypto.randomUUID(), subtle: {digest: async (...args) => {
    const bytes = await webcrypto.subtle.digest(...args); hashes++; return bytes;
  }}}, Notification: {permission: passive ? 'granted' : 'default', requestPermission: async () => 'granted'}, PushManager: class {},
    navigator: {serviceWorker: {ready: stall === 'ready' ? never() : Promise.resolve(registration),
      register: async () => stall === 'register' ? never() : registration,
      getRegistration: async () => stall === 'getRegistration' ? never() : registration}}}});
  const config = {contract: 'coven.web-push.v1', enabled: true, public_key: 'A'.repeat(87), subscriptions: [id]};
  return {...b, config, id, removals: () => removals, hashes: () => hashes, completeRemoval: () => finishRemoval()};
}

test('Disable removes the browser subscription even when config, CSRF or server removal fails; server cleanup needs another click', async () => {
  for (const failure of ['config', 'csrf', 'post']) {
    const b = await alertsFixture(); b.$('disable-alerts').emit('click');
    await b.respond('/api/v1/push/config', failure === 'config' ? {error: {code: 'unavailable'}} : b.config, failure === 'config' ? 503 : 200);
    assert.equal(b.removals(), 1);
    if (failure !== 'config') await b.respond('/api/v1/csrf', failure === 'csrf' ? {} : token, failure === 'csrf' ? 503 : 200);
    if (failure === 'post') await b.respond('/api/v1/push/unsubscribe', {}, 503);
    assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
    assert.equal(b.$('disable-alerts').disabled, false);
    const posts = b.posts().length;
    await b.refresh([item]); assert.equal(b.posts().length, posts, 'polling must not retry server cleanup');
    b.$('disable-alerts').emit('click');
    await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', token);
    const post = b.posts().at(-1);
    assert.deepEqual(JSON.parse(post.options.body), {contract: 'coven.web-push.v1', execute: true, subscription_id: b.id});
    await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
    assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled');
  }
});

test('failed local unsubscription never reports browser disabled even after verified server removal', async () => {
  const b = await alertsFixture({local: false}); b.$('disable-alerts').emit('click');
  await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
  assert.match(b.$('alerts-status').textContent, /Browser removal unverified/);
  assert.doesNotMatch(b.$('alerts-status').textContent, /disabled/);
  assert.equal(b.$('disable-alerts').disabled, false);
});

test('stalled config, token, POST headers and POST body release controls at the deadline, without retry or late enabled claims', async () => {
  for (const stage of ['config', 'csrf', 'headers', 'body']) {
    const b = await alertsFixture(); b.$('disable-alerts').emit('click');
    for (let n = 0; n < 100 && !b.pending('/api/v1/push/config'); n++) await flush();
    assert.equal(b.removals(), 1);
    if (stage !== 'config') await b.respond('/api/v1/push/config', b.config);
    if (['headers', 'body'].includes(stage)) await b.respond('/api/v1/csrf', token);
    let finishBody;
    if (stage === 'body') {
      const request = b.pending('/api/v1/push/unsubscribe'); request.done = true;
      request.resolve({ok: true, status: 200, json: () => new Promise(resolve => { finishBody = resolve; })}); await flush();
    }
    await b.deadline();
    assert.equal(b.$('enable-alerts').disabled, false); assert.equal(b.$('disable-alerts').disabled, false);
    assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
    const posts = b.posts().length;
    if (finishBody) { finishBody({contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false}); await flush(); }
    await b.refresh([item]); assert.equal(b.posts().length, posts);
  }
});

test('stalled browser registration, ready, subscription lookup and local unsubscription also release controls', async () => {
  for (const stall of ['register', 'ready', 'subscribe', 'getSubscription', 'getRegistration', 'unsubscribe']) {
    const b = await alertsFixture({stall});
    const enable = ['register', 'ready', 'subscribe'].includes(stall);
    b.$(enable ? 'enable-alerts' : 'disable-alerts').emit('click');
    if (enable) { await flush(); await b.respond('/api/v1/push/config', b.config); }
    for (let n = 0; n < 10; n++) await flush();
    await b.deadline();
    assert.equal(b.$('enable-alerts').disabled, false); assert.equal(b.$('disable-alerts').disabled, false);
    assert.match(b.$('alerts-status').textContent, /unverified/);
    assert.equal(b.posts().length, 0);
  }
});

test('subscription creation with a lost receipt stays unverified, never auto-reposts, and can be explicitly disabled', async () => {
  const b = await alertsFixture(); b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', token);
  assert.equal(b.posts().length, 1);
  await b.deadline(); assert.match(b.$('alerts-status').textContent, /Background state unverified/);
  assert.equal(b.$('enable-alerts').disabled, false);
  await b.respond('/api/v1/push/subscriptions', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: true});
  await b.refresh([item]); assert.equal(b.posts().length, 1);
  assert.match(b.$('alerts-status').textContent, /unverified/);
  b.$('disable-alerts').emit('click'); await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
  assert.equal(b.removals(), 1); assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
});

test('subscription enabled state is separate from closed delivery degradation and arbitrary provider errors never display', async () => {
  for (const [code, label] of [['sent', 'advisory'], ['retrying', 'degraded'], ['delivery_failed', 'degraded'], ['private/path/provider-error', 'unverified']]) {
    const b = await alertsFixture(); b.$('enable-alerts').emit('click'); await flush();
    await b.respond('/api/v1/push/config', {...b.config, delivery_status: code}); await b.respond('/api/v1/csrf', token);
    await b.respond('/api/v1/push/subscriptions', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: true});
    assert.match(b.$('alerts-status').textContent, new RegExp(`subscription enabled / delivery ${label}`));
    assert.doesNotMatch(b.$('alerts-status').textContent, /private|provider-error/);
  }
});

test('an unknown create may commit after absent config and a successful delete; neither clears its uncertainty', async () => {
  const b = await alertsFixture(); b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', token);
  const create = b.pending('/api/v1/push/subscriptions'); assert.ok(create);
  await b.deadline();
  b.$('disable-alerts').emit('click');
  await b.respond('/api/v1/push/config', {...b.config, subscriptions: []});
  await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
  assert.equal(b.removals(), 1);
  assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
  assert.equal(b.$('disable-alerts').hidden, false);
  // The original blocking create completes only after that delete/config sample.
  await b.respond('/api/v1/push/subscriptions', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: true});
  await b.refresh([item]);
  assert.equal(b.posts().length, 2); // No automatic creation or cleanup retries.
  assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
  b.$('disable-alerts').emit('click'); await b.respond('/api/v1/push/config', b.config);
  await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
  assert.equal(b.posts().length, 3);
  assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
  assert.doesNotMatch(b.$('alerts-status').textContent, /mozilla|browser-key|browser-auth/);
});

test('a local unsubscribe timeout retains its hashed ID before the wait, including after late success and getSubscription null', async () => {
  const b = await alertsFixture({stall: 'unsubscribe'}); b.$('disable-alerts').emit('click');
  await waitFor(() => b.removals() === 1 && b.hashes() === 1);
  await flush(); // Store the completed SHA256 while unsubscribe stays pending.
  await b.deadline();
  assert.match(b.$('alerts-status').textContent, /Browser removal unverified/);
  b.completeRemoval(); await flush();
  b.$('disable-alerts').emit('click'); await b.respond('/api/v1/push/config', b.config);
  await b.respond('/api/v1/csrf', token);
  const post = b.pending('/api/v1/push/unsubscribe'); assert.ok(post);
  assert.equal(JSON.parse(post.options.body).subscription_id, b.id);
  await b.respond('/api/v1/push/unsubscribe', {}, 503);
  assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled; server removal unverified');
  assert.equal(b.$('disable-alerts').hidden, false);
  assert.equal(b.removals(), 1, 'later cleanup must use saved ID without another browser subscription');
});

test('passive inspection started before Disable cannot restore old subscription state after the user operation finishes', async () => {
  for (const status of [200, 503]) {
    const b = await alertsFixture({passive: true});
    for (let n = 0; n < 100 && !b.pending('/api/v1/push/config'); n++) await flush();
    const inspection = b.pending('/api/v1/push/config'); assert.ok(inspection); inspection.done = true;
    b.$('disable-alerts').emit('click'); await b.respond('/api/v1/push/config', b.config);
    await b.respond('/api/v1/csrf', token);
    await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
    const before = b.$('alerts-status').textContent;
    assert.equal(before, 'Browser alerts disabled');
    inspection.resolve({ok: status === 200, status, json: async () => b.config});
    for (let n = 0; n < 10; n++) await flush();
    assert.equal(b.$('alerts-status').textContent, before);
    assert.equal(b.$('enable-alerts').hidden, false); assert.equal(b.$('disable-alerts').hidden, true);
    assert.equal(b.posts().length, 1);
  }
});

test('CSRF failure before any subscription POST does not invent an unresolved creation', async () => {
  const b = await alertsFixture(); b.$('enable-alerts').emit('click'); await flush();
  await b.respond('/api/v1/push/config', b.config); await b.respond('/api/v1/csrf', {}, 503);
  assert.equal(b.posts().length, 0);
  b.$('disable-alerts').emit('click'); await b.respond('/api/v1/push/config', b.config);
  await b.respond('/api/v1/csrf', token);
  await b.respond('/api/v1/push/unsubscribe', {contract: 'coven.web-push.v1', subscription_id: b.id, subscribed: false});
  assert.equal(b.$('alerts-status').textContent, 'Browser alerts disabled');
});

test('browser endpoint identity hashing refuses oversized or nonstandard URLs before invoking crypto', async () => {
  let hashes = 0;
  const context = vm.createContext({TextEncoder, URL, crypto: {subtle: {digest: (...args) => { hashes++; return webcrypto.subtle.digest(...args); }}}});
  vm.runInContext(app.replace(/^export /gm, ''), context);
  for (const endpoint of ['https://push.services.mozilla.com/' + 'x'.repeat(512 * 1024), 'http://push.services.mozilla.com/a',
    'https://user:password@push.services.mozilla.com/a', 'https://push.services.mozilla.com:8443/a', 'https://push.services.mozilla.com/a#fragment']) {
    await assert.rejects(context.subscriptionID(endpoint));
  }
  assert.equal(hashes, 0);
  assert.match(await context.subscriptionID('https://push.services.mozilla.com/bounded'), /^[a-f0-9]{64}$/);
  assert.equal(hashes, 1);
});
