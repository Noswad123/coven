import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { CONTRACT, validateFleet, initialState, receiveFleet, failFleet, projectFleet,
  ageLabel, validateOutput, createOutputGate, setText, errorMessage, endpointCoverage, responseError } from './app.js';

const NOW = 1900000000000;
test('Coven Web assets and package retain their canonical branding and API contract', () => {
  const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
  const metadata = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
  assert.match(import.meta.url, /\/clients\/web\/app\.test\.mjs$/);
  assert.match(html, /<title>Coven Web · Sessions<\/title>/);
  assert.equal((html.match(/class="agent-grid"/g) ?? []).length, 1);
  assert.doesNotMatch(html, /id="(?:conversation-list|refresh-conversations|conversations-title|chat-picker|shared-chats)"/);
  assert.match(html, /<h1>Coven Web<\/h1>/);
  assert.match(html, /class="brand-mark" aria-hidden="true">C<span>/);
  assert.match(html, /<noscript><p class="notice">Coven Web needs JavaScript/);
  assert.equal(metadata.name, 'coven-web-client');
  assert.equal(CONTRACT, 'coven.fleet-observer.v1');
});

test('user-marked explanatory elements are removed while retained copy and compact spacing remain', () => {
  const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
  const css = readFileSync(new URL('./styles.css', import.meta.url), 'utf8');
  assert.doesNotMatch(html, /delete this/i);
  assert.doesNotMatch(html, /<footer\b/i);
  assert.match(html, /<p class="eyebrow">Agent Management<\/p>/);
  assert.match(html, /<span class="read-only">Private<\/span>/);
  assert.match(html, /Coverage is incomplete\./);
  assert.match(html, /A quiet view of your sessions\./);
  assert.doesNotMatch(css, /\bfooter\b|\.intro > p:last-child/);
  assert.match(css, /\.shell \{[^}]*padding: 0 20px 32px;/);
  assert.match(css, /\.shell \{ padding: 0 16px 24px; \}/);
  assert.match(css, /\.shell \{ padding: 0 36px 40px; \}/);
});

const A = 'a'.repeat(64);
const B = 'b'.repeat(64);
function agent(id = A, changes = {}) {
  return { id, title: 'Agent', provider: 'djinn', project: 'Project', activity: 'idle',
    evidence: 'kitsune_snapshot', observed_at_ms: NOW, output_available: true, ...changes };
}
function endpoint(id = 'desktop', agents = [agent()], changes = {}) {
  return { id, label: id, connection: 'reachable', last_attempt_at_ms: NOW,
    observed_at_ms: NOW, error: null, agents, ...changes };
}
function fleet(endpoints = [endpoint()], changes = {}) {
  return { contract: CONTRACT, generated_at_ms: NOW, refresh_interval_ms: 3000,
    stale_after_ms: 15000, endpoints, ...changes };
}
function loaded(value = fleet()) { return receiveFleet(initialState(), value, NOW); }
function output(changes = {}) {
  return { contract: CONTRACT, agent_id: A, observed_at_ms: NOW, text: 'Text\n',
    truncated: false, attribution: 'matching_snapshot_brackets_best_effort', ...changes };
}
function processAgent(id = A, changes = {}) {
  return agent(id, { evidence: 'coven_process_discovery', activity: 'unknown',
    output_available: false, provider_candidates: ['opencode'], ...changes });
}
function discoveredEndpoint(agents = [processAgent()], changes = {}) {
  return endpoint('test-host', agents, { discovery_status: 'observed', unclassified_panes: 0, ...changes });
}

test('module imports without a browser and projects exact DTO metadata', () => {
  const state = loaded();
  const [card] = projectFleet(state, '', NOW).cards;
  assert.equal(state.failed, false);
  assert.equal(card.agent.id, A);
  assert.equal(card.endpoint.id, 'desktop');
  assert.equal(card.fresh, true);
  assert.equal(card.outputEnabled, true);
  assert.equal(card.activityLabel, 'Idle (not completion)');
  assert.equal(card.age, 'Observed just now');
});

test('sort is fresh first, then host/title with identity tie breaker; filtering is endpoint-scoped', () => {
  const state = loaded(fleet([
    endpoint('z', [agent(A, { title: 'Alpha' })]),
    endpoint('a', [agent(B, { title: 'Zeta', observed_at_ms: NOW - 15000 }), agent('c'.repeat(64), { title: 'Beta' })])
  ]));
  const view = projectFleet(state, '', NOW);
  assert.deepEqual(view.cards.map(card => card.agent.id), ['c'.repeat(64), A, B]);
  assert.equal(projectFleet(state, 'z', NOW).cards.length, 1);
  assert.equal(projectFleet(state, 'missing', NOW).cards.length, 0);
});

test('colliding titles do not merge distinct endpoint-scoped references', () => {
  const view = projectFleet(loaded(fleet([endpoint('one'), endpoint('two', [agent(B)])])), '', NOW);
  assert.equal(view.cards.length, 2);
  assert.notEqual(view.cards[0].key, view.cards[1].key);
});

test('duplicate endpoint IDs and duplicate agent IDs reject the entire response, never merge', () => {
  const duplicates = [
    fleet([endpoint('same'), endpoint('same', [agent(B)])]),
    fleet([endpoint('one'), endpoint('two')]),
    fleet([endpoint('one', [agent(), agent()])])
  ];
  const state = loaded();
  for (const invalid of duplicates) {
    assert.throws(() => validateFleet(invalid));
    const rejected = receiveFleet(state, invalid, NOW);
    assert.equal(rejected.snapshot, state.snapshot);
    assert.equal(rejected.failed, true);
    assert.equal(projectFleet(rejected, '', NOW).cards[0].outputEnabled, false);
  }
});

test('failure preserves metadata immediately stale; a valid empty snapshot removes it', () => {
  const state = loaded();
  const failed = failFleet(state);
  assert.equal(failed.snapshot, state.snapshot);
  assert.equal(projectFleet(failed, '', NOW).cards[0].fresh, false);
  const empty = receiveFleet(failed, fleet([]), NOW);
  assert.equal(empty.failed, false);
  assert.equal(projectFleet(empty, '', NOW).empty, true);
  assert.deepEqual(projectFleet(empty, '', NOW).cards, []);
  assert.equal(projectFleet(failFleet(initialState()), '', NOW).empty, false);
});

test('evidence ages out at the threshold, independently of fetches and server forwarding', () => {
  const state = loaded();
  assert.equal(projectFleet(state, '', NOW + 14999).cards[0].fresh, true);
  assert.equal(projectFleet(state, '', NOW + 15000).cards[0].fresh, false);
  const forwarded = loaded(fleet([endpoint('desktop', [agent(A, { observed_at_ms: NOW - 16000 })])]));
  assert.equal(projectFleet(forwarded, '', NOW).cards[0].fresh, false);
  const oldEndpoint = loaded(fleet([endpoint('desktop', [agent()], { observed_at_ms: NOW - 15000 })]));
  assert.equal(projectFleet(oldEndpoint, '', NOW).cards[0].fresh, false);
  const oldGeneration = loaded(fleet(undefined, { generated_at_ms: NOW - 15000 }));
  assert.equal(projectFleet(oldGeneration, '', NOW).cards[0].fresh, false);
});

test('freshness never overrides endpoint health or the server output flag', () => {
  for (const connection of ['unavailable', 'unsupported']) {
    const state = loaded(fleet([endpoint('desktop', [agent()], { connection, error: 'unavailable' })]));
    assert.equal(projectFleet(state, '', NOW).cards[0].outputEnabled, false);
  }
  const disabled = projectFleet(loaded(fleet([endpoint('desktop', [agent(A, { output_available: false })])])), '', NOW).cards[0];
  assert.equal(disabled.fresh, true);
  assert.equal(disabled.outputEnabled, false);
  const neverObserved = loaded(fleet([endpoint('desktop', [], { connection: 'unavailable', observed_at_ms: null })]));
  assert.equal(neverObserved.failed, false);
});

test('fresh snapshots recover from failure, but older generations and far-future clocks do not', () => {
  const state = loaded();
  assert.equal(receiveFleet(failFleet(state), fleet(), NOW).failed, false);
  for (const generated_at_ms of [NOW - 1, NOW + 5001]) {
    const rejected = receiveFleet(state, fleet(undefined, { generated_at_ms }), NOW);
    assert.equal(rejected.failed, true);
    assert.equal(rejected.snapshot, state.snapshot);
  }
  const badAgentClock = loaded(fleet([endpoint('desktop', [agent(A, { observed_at_ms: NOW + 5001 })])]));
  assert.equal(projectFleet(badAgentClock, '', NOW).cards[0].fresh, false);
});

test('invalid schemas cannot erase known cards or enable output', () => {
  const invalids = [null, {}, [], { error: { code: 'denied' } },
    fleet(undefined, { contract: 'other' }), fleet(undefined, { refresh_interval_ms: 0 }),
    fleet(undefined, { stale_after_ms: -1 }), fleet(undefined, { generated_at_ms: NaN }),
    fleet([endpoint('desktop', [agent('arbitrary/path')])]),
    fleet([endpoint('desktop', [agent(A, { output_available: 'true' })])]),
    fleet([endpoint('desktop', [agent(A, { activity: 'done' })])]),
    fleet([endpoint('desktop', [agent(A, { evidence: 'native_approval' })])]),
    fleet([endpoint('desktop', [agent(A, { title: null })])]),
    fleet([endpoint('desktop', [], { connection: 'connected' })])];
  const state = loaded();
  for (const value of invalids) {
    const next = receiveFleet(state, value, NOW);
    assert.equal(next.error, 'invalid_snapshot');
    assert.equal(next.snapshot, state.snapshot);
    assert.equal(projectFleet(next, '', NOW).cards[0].outputEnabled, false);
  }
});

test('heuristic labels do not turn blocked into approval or idle into success', () => {
  const state = loaded(fleet([endpoint('desktop', [agent(A, { activity: 'blocked' }), agent(B, { activity: 'unknown' })])]));
  const labels = projectFleet(state, '', NOW).cards.map(card => card.activityLabel);
  assert.ok(labels.includes('Blocked hint'));
  assert.ok(labels.includes('Activity unknown'));
  assert.ok(labels.every(label => !/approved|complete|success/i.test(label)));
});

test('HTML, ANSI, OSC hyperlinks and control text stay literal through projection and text sink', () => {
  const hostile = '<img src=x onerror=alert(1)><script>evil()</script>\n\u001b[31mred\u001b[0m\u001b]8;;https://invalid.example\u0007link\u001b]8;;\u0007\r\b';
  const value = fleet([endpoint('desktop', [agent(A, { title: hostile, project: hostile, provider: hostile })], { label: hostile })]);
  const card = projectFleet(loaded(value), '', NOW).cards[0];
  assert.equal(card.agent.title, hostile);
  assert.equal(card.endpoint.label, hostile);
  const node = { set innerHTML(_) { assert.fail('HTML sink used'); }, textContent: '' };
  setText(node, validateOutput(output({ text: hostile }), A, NOW, 15000).text);
  assert.equal(node.textContent, hostile);
});

test('output validates identity, contract, attribution, age, flag and UTF-8 byte cap', () => {
  assert.equal(validateOutput(output(), A, NOW, 15000).text, 'Text\n');
  assert.equal(validateOutput(output({ text: '', truncated: true }), A, NOW, 15000).truncated, true);
  assert.equal(validateOutput(output({ text: 'x'.repeat(65536) }), A, NOW, 15000).text.length, 65536);
  for (const value of [null, output({ agent_id: B }), output({ contract: 'wrong' }),
    output({ attribution: 'verified' }), output({ text: null }), output({ truncated: 'false' }),
    output({ observed_at_ms: NOW - 15000 }), output({ observed_at_ms: NOW + 5001 }),
    output({ text: 'x'.repeat(65537) }), output({ text: '\u00e9'.repeat(32769) })]) {
    assert.throws(() => validateOutput(value, A, NOW, 15000));
  }
});

test('selection tickets reject a different card, close/reopen, superseding reads and invalidated references', () => {
  const gate = createOutputGate();
  const first = gate.begin('host/a');
  assert.equal(gate.accepts(first, 'host/a'), true);
  assert.equal(gate.accepts(first, 'host/b'), false);
  gate.invalidate();
  const reopened = gate.begin('host/a');
  assert.equal(gate.accepts(first, 'host/a'), false);
  assert.equal(gate.accepts(reopened, 'host/a'), true);
  const secondRead = gate.begin('host/a');
  assert.equal(gate.accepts(reopened, 'host/a'), false);
  assert.equal(gate.accepts(secondRead, 'host/a'), true);
  gate.begin('host/b');
  assert.equal(gate.accepts(secondRead, 'host/b'), false);
  gate.invalidate();
  assert.equal(gate.accepts(secondRead, null), false);
});

test('asynchronous late responses cannot win over a newly selected card', async () => {
  const gate = createOutputGate();
  let resolveFirst;
  const firstRequest = new Promise(resolve => { resolveFirst = resolve; });
  let selected = 'host/a';
  let visible = '';
  const ticket = gate.begin(selected);
  const pending = firstRequest.then(text => { if (gate.accepts(ticket, selected)) visible = text; });
  gate.invalidate();
  selected = 'host/b';
  const latest = gate.begin(selected);
  if (gate.accepts(latest, selected)) visible = 'B output';
  resolveFirst('A output');
  await pending;
  assert.equal(visible, 'B output');
});

test('age labels are deterministic, and auth failures give private access guidance', () => {
  assert.equal(ageLabel(null, NOW), 'Not observed');
  assert.equal(ageLabel(NOW + 1, NOW), 'Observed just now');
  assert.equal(ageLabel(NOW - 3000, NOW), 'Observed 3s ago');
  assert.equal(ageLabel(NOW - 60000, NOW), 'Observed 1m ago');
  assert.equal(ageLabel(NOW - 3600000, NOW), 'Observed 1h ago');
  assert.equal(ageLabel(NOW - 86400000, NOW), 'Observed 1d ago');
  assert.match(errorMessage('auth'), /Tailscale Serve/);
  assert.match(errorMessage('auth'), /allowed identity and origin/);
});
test('operator-disabled output is not a fleet authentication failure', async () => {
  const { responseError } = await import('./app.js');
  assert.equal(responseError(403, { error: { code: 'output_disabled' } }), 'output_disabled');
  assert.equal(responseError(403, { error: { code: 'access_denied' } }), 'auth');
  assert.equal(responseError(403, null), 'auth');
  assert.equal(responseError(401, { error: { code: 'output_disabled' } }), 'auth');
  assert.equal(responseError(503, { error: { code: '<script>' } }), 'request_failed');
});

test('two process observations render even with zero Kitsune metadata agents', () => {
  // Synthetic gateway fixture: upstream metadata inventory was empty; the backend
  // supplied two recognized terminal references through its own process discovery.
  const state = loaded(fleet([discoveredEndpoint([
    processAgent(A, { title: 'First observation' }), processAgent(B, { title: 'Second observation' })
  ])]));
  assert.equal(state.failed, false);
  const view = projectFleet(state, '', NOW);
  assert.equal(view.cards.length, 2);
  assert.equal(view.cards.filter(card => card.agent.evidence === 'kitsune_snapshot').length, 0);
  for (const card of view.cards) {
    assert.equal(card.fresh, true);
    assert.equal(card.evidenceLabel, 'Coven process evidence');
    assert.equal(card.activityLabel, 'Activity unknown');
    assert.equal(card.outputEnabled, false);
    assert.equal(card.providerLabel, 'Provider identity unconfirmed');
  }
});

test('process family ambiguity remains hints, never provider identity or working activity', () => {
  const candidates = ['claude', 'opencode'];
  const state = loaded(fleet([discoveredEndpoint([processAgent(A, { provider_candidates: candidates, provider: 'ambiguous' })])]));
  const [card] = projectFleet(state, '', NOW).cards;
  assert.equal(card.familyHints, 'Process family hints (not identity): claude, opencode');
  assert.equal(card.providerLabel, 'Provider identity unconfirmed');
  assert.deepEqual(card.agent.provider_candidates, candidates);
  assert.equal(card.activityLabel, 'Activity unknown');
  const metadata = projectFleet(loaded(), '', NOW).cards[0];
  assert.equal(metadata.evidenceLabel, 'Kitsune metadata');
  assert.equal(metadata.providerLabel, 'djinn');
  assert.equal(metadata.familyHints, '');
});

test('process evidence rejects invented activity and output capability, retaining known observations', () => {
  const known = loaded(fleet([discoveredEndpoint()]));
  for (const changes of [{ activity: 'working' }, { activity: 'idle' }, { activity: 'blocked' }, { output_available: true },
    { provider_candidates: undefined }, { evidence: 'terminal_title_guess' }]) {
    const next = receiveFleet(known, fleet([discoveredEndpoint([processAgent(A, changes)])]), NOW);
    assert.equal(next.failed, true);
    assert.equal(next.snapshot, known.snapshot);
    assert.equal(projectFleet(next, '', NOW).cards[0].fresh, false);
    assert.equal(projectFleet(next, '', NOW).cards[0].outputEnabled, false);
  }
});

test('candidate arrays are optional for old metadata, bounded and required for process evidence', () => {
  assert.equal(loaded().failed, false);
  assert.equal(loaded(fleet([discoveredEndpoint([processAgent(A, { provider_candidates: [] })])])).failed, false);
  const max = Array.from({ length: 22 }, (_, index) => `family_${index}`);
  assert.equal(loaded(fleet([discoveredEndpoint([processAgent(A, { provider_candidates: max })])])).failed, false);
  for (const provider_candidates of [null, 'claude', [null], [1], [''], [...max, 'extra']]) {
    assert.throws(() => validateFleet(fleet([discoveredEndpoint([processAgent(A, { provider_candidates })])])));
    assert.throws(() => validateFleet(fleet([endpoint('desktop', [agent(A, { provider_candidates })])])));
  }
});

test('discovery fields validate closed status and nonnegative count; legacy metadata remains valid', () => {
  assert.equal(loaded().failed, false);
  for (const discovery_status of ['observed', 'unsupported', 'unavailable']) {
    for (const unclassified_panes of [0, 2, null]) {
      assert.equal(loaded(fleet([discoveredEndpoint([], { discovery_status, unclassified_panes })])).failed, false);
    }
  }
  for (const changes of [{ discovery_status: null }, { discovery_status: 'complete' },
    { unclassified_panes: -1 }, { unclassified_panes: 1.5 }, { unclassified_panes: '2' }]) {
    assert.throws(() => validateFleet(fleet([discoveredEndpoint([], changes)])));
  }
  for (const changes of [{ discovery_status: undefined }, { unclassified_panes: undefined }]) {
    assert.throws(() => validateFleet(fleet([discoveredEndpoint([processAgent()], changes)])));
  }
});

test('reachable empty fleets describe discovery status and incomplete coverage, not absence of running agents', () => {
  for (const discovery_status of ['observed', 'unsupported', 'unavailable']) {
    const state = loaded(fleet([discoveredEndpoint([], { discovery_status, unclassified_panes: 2 })]));
    const view = projectFleet(state, '', NOW);
    assert.equal(state.failed, false);
    assert.equal(view.empty, true);
    assert.equal(view.emptyTitle, 'No recognized agent evidence');
    assert.match(view.emptyDescription, new RegExp(`Process discovery: ${discovery_status}`));
    assert.match(view.emptyDescription, /Coverage is incomplete/);
    assert.match(view.emptyDescription, /Scan completeness unknown/);
    assert.match(view.emptyDescription, /2 unclassified terminals \(not identified as agents\)/);
    assert.match(view.emptyDescription, /does not establish that no agents are running/);
    assert.equal(view.cards.length, 0);
  }
  const legacy = projectFleet(loaded(fleet([endpoint('desktop', [])])), '', NOW);
  assert.match(legacy.emptyDescription, /Process discovery: not reported/);
  assert.match(legacy.emptyDescription, /Unclassified terminal count unknown/);
  assert.match(endpointCoverage(discoveredEndpoint([], { unclassified_panes: 1 })), /1 unclassified terminal \(/);
  assert.match(endpointCoverage(discoveredEndpoint([], { unclassified_panes: null })), /count unknown/);
});

test('unhealthy discovery and retries retain stale process cards; metadata remains independent', () => {
  const known = loaded(fleet([discoveredEndpoint()]));
  let failed = known;
  for (let attempt = 1; attempt <= 3; attempt++) {
    failed = failFleet(failed);
    const [card] = projectFleet(failed, '', NOW + attempt * 3000).cards;
    assert.equal(failed.snapshot, known.snapshot);
    assert.equal(card.fresh, false);
    assert.equal(card.outputEnabled, false);
    assert.equal(card.activityLabel, 'Activity unknown');
    assert.equal(card.age, `Observed ${attempt * 3}s ago`);
  }
  for (const discovery_status of ['unsupported', 'unavailable']) {
    const state = loaded(fleet([discoveredEndpoint([processAgent(), agent(B)], { discovery_status })]));
    const cards = projectFleet(state, '', NOW).cards;
    assert.equal(cards.find(card => card.agent.id === A).fresh, false);
    assert.equal(cards.find(card => card.agent.id === B).fresh, true);
  }
  assert.equal(projectFleet(receiveFleet(failed, fleet([discoveredEndpoint()]), NOW), '', NOW).cards[0].fresh, true);
});

test('process hint strings remain literal at the same inert text sink', () => {
  const hint = '<img src=x onerror=alert(1)>\u001b[31m';
  const view = projectFleet(loaded(fleet([discoveredEndpoint([processAgent(A, { provider_candidates: [hint] })])])), '', NOW);
  const node = { set innerHTML(_) { assert.fail('HTML sink used'); }, textContent: '' };
  setText(node, view.cards[0].familyHints);
  assert.equal(node.textContent, `Process family hints (not identity): ${hint}`);
});

test('403 output_unsupported is a capability error, never fleet auth failure', () => {
  assert.equal(responseError(403, { error: { code: 'output_unsupported' } }), 'output_unsupported');
  assert.equal(responseError(401, { error: { code: 'output_unsupported' } }), 'auth');
  assert.equal(responseError(403, { error: { code: 'access_denied' } }), 'auth');
  assert.equal(responseError(503, { error: { code: 'output_unsupported' } }), 'request_failed');
});
