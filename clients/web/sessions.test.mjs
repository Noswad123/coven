import {test} from 'node:test';
import assert from 'node:assert/strict';
import {projectSessions, receiveFleet, failFleet, initialState, validateConversations} from './app.js';

const now = 10000;
const agent = 'a'.repeat(64), binding = 'b'.repeat(64);
const row = (id = 'c'.repeat(64), changes = {}) => ({id, label: 'Same label', team: 'research', host: 'Local native',
  association_status: 'verified', card: {fleet_agent_id: agent, fleet_endpoint_id: 'one', binding_id: binding}, ...changes});
const state = receiveFleet(initialState(), {contract: 'coven.fleet-observer.v1', generated_at_ms: now,
  refresh_interval_ms: 3000, stale_after_ms: 15000, endpoints: ['one', 'two'].map((id, i) => ({id, label: 'Same label',
    connection: 'reachable', last_attempt_at_ms: now, observed_at_ms: now, error: null,
    agents: [{id: i ? 'd'.repeat(64) : agent, title: 'Same label', provider: 'djinn', project: '', activity: 'unknown',
      evidence: 'kitsune_snapshot', observed_at_ms: now, output_available: false}]}))}, now);

test('association projection never guesses by labels, endpoint aliases, provider or a single candidate', () => {
  for (const change of [{association_status: undefined, card: undefined}, {association_status: 'ambiguous'},
    {association_status: 'unavailable'}, {card: {...row().card, fleet_endpoint_id: 'two'}},
    {card: {...row().card, fleet_agent_id: 'e'.repeat(64)}}]) {
    const cards = projectSessions(state, [row(undefined, change)], '', '', now);
    assert.equal(cards.length, 2);
    assert.equal(cards.filter(c => c.endpoint && c.target).length, 0);
    assert.ok(cards.every(card => card.target === null));
  }
});

test('competing authorized claims suppress neither row; absent unauthorized claims cannot select or block a target', () => {
  const rows = [row(), row('f'.repeat(64))];
  assert.equal(projectSessions(state, rows, '', '', now).length, 2);
  assert.equal(projectSessions(state, rows, 'one', 'research', now).length, 0);
  const cards = projectSessions(state, [rows[1]], '', '', now);
  assert.equal(cards.length, 2);
  assert.equal(cards[0].target.id, rows[1].id);
  assert.equal(cards[0].target.binding.binding_id, binding);
  assert.equal(projectSessions(state, [], '', '', now).length, 2);
});

test('optional association schema rejects unsafe route segments without weakening legacy rows', () => {
  const validate = target => validateConversations({contract: 'coven.conversations.v1', conversations: [target]});
  assert.doesNotThrow(() => validate(row(undefined, {association_status: undefined, card: undefined})));
  for (const card of [{...row().card, fleet_agent_id: '/private'}, {...row().card, binding_id: '..'},
    {...row().card, fleet_endpoint_id: 'host/other'}]) assert.throws(() => validate(row(undefined, {card})));
  assert.throws(() => validate(row(undefined, {association_status: 'maybe'})));
});

test('aging and endpoint filtering do not reorder the remaining opaque card identities', () => {
  const keys = (time, host = '') => projectSessions(state, [row()], host, '', time).map(card => card.key);
  assert.deepEqual(keys(now), keys(now + 15000));
  assert.deepEqual(keys(now, 'two'), keys(now).filter(key => JSON.parse(key)[0] === 'two'));
});

test('only fresh verified roots link; native metadata never replaces fleet identity or adds cards', () => {
  const target = row(undefined, {association_status: 'unavailable', card: null});
  const cards = projectSessions(state, [target], '', '', now);
  assert.equal(cards.length, 2);
  assert.equal(cards[0].target, null);
  assert.match(cards[0].chatUnavailable, /No verified conversation link/);
  assert.equal(cards[0].agent.title, 'Same label');
  assert.equal(projectSessions(state, [target], '', 'research', now).length, 0);
  assert.equal(projectSessions(state, [row()], '', '', now + 15000)[0].target, null);
  assert.equal(projectSessions(state, [row(), target], '', '', now)[0].target.binding.binding_id, binding);
  assert.equal(projectSessions(state, [row(undefined, {card: {...row().card, fleet_endpoint_id: 'two'}})], '', '', now)[0].target, null);
});

test('failed fleet, unsupported endpoints and mixed competing association claims fail closed', () => {
  const unsupported = {...state, snapshot: {...state.snapshot, endpoints: state.snapshot.endpoints.map(endpoint => ({...endpoint, connection: 'unsupported'}))}};
  for (const invalid of [failFleet(state), unsupported]) {
    const cards = projectSessions(invalid, [row()], '', '', now);
    assert.equal(cards.length, 2);
    assert.equal(cards[0].target, null);
    assert.match(cards[0].chatUnavailable, /stale or unavailable/);
  }
  for (const association_status of ['verified', 'ambiguous', 'unavailable']) {
    const cards = projectSessions(state, [row(), row('f'.repeat(64), {association_status})], '', '', now);
    assert.equal(cards[0].target, null);
    assert.match(cards[0].chatUnavailable, /Multiple conversations claim/);
  }
});
