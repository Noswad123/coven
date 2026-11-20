import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const source = await readFile(new URL('./service-worker.js', import.meta.url), 'utf8');
const id = 'a'.repeat(64);
function worker(windows = []) {
  const handlers = new Map(), shown = [], opened = [];
  const self = {location: {origin: 'https://coven.example'}, addEventListener: (event, handler) => handlers.set(event, handler),
    registration: {showNotification: async (title, options) => shown.push({title, options})},
    clients: {matchAll: async () => windows, openWindow: async url => opened.push(url)}};
  vm.runInNewContext(source, {self, URL, TextEncoder});
  return {handlers, shown, opened, push: async text => { let work; handlers.get('push')({data: {text: () => text}, waitUntil: p => { work = p; }}); await work; },
    click: async (data = {alert_id: id}, action = '') => { let work, closed = false;
      handlers.get('notificationclick')({action, notification: {data, close() { closed = true; }}, waitUntil: p => { work = p; }});
      await work; assert.ok(closed);
    }};
}
test('closed-tab worker accepts only tiny closed generic payload; no HTML/provider text/actions/cache/fetch', async () => {
  const w = worker();
  await w.push(JSON.stringify({version: 1, alert_id: id}));
  assert.equal(w.shown.length, 1);
  assert.equal(w.shown[0].title, 'Coven needs your attention');
  assert.equal(w.shown[0].options.body, 'An agent is waiting for your response');
  assert.equal(w.shown[0].options.tag, id); assert.equal(w.shown[0].options.actions, undefined);
  await w.push(JSON.stringify({alert_id: id, version: 1}));
  assert.equal(w.shown.length, 2);
  assert.equal(w.handlers.has('fetch'), false);
  for (const payload of [{version: 2, alert_id: id}, {version: 1, alert_id: '<script>'}, {version: 1, alert_id: id, title: 'secret'},
    {version: 1, alert_id: id, url: 'https://evil.example'}, {version: 1, alert_id: id, action: 'permission_once'}]) await w.push(JSON.stringify(payload));
  await w.push(`{"version":1,"alert_id":"${id}","alert_id":"${id}"}`);
  await w.push(' '.repeat(257)); await w.push('null'); await w.push('{');
  assert.equal(w.shown.length, 2);
  assert.doesNotMatch(source, /fetch\(|caches\.|localStorage|permission_once|question_answer/);
});
test('notificationclick opens only same-origin locator and never approves or follows foreign URLs', async () => {
  let foreignUsed = false, focused = 0, navigation;
  const w = worker([{url: 'https://evil.example/', navigate: async () => { foreignUsed = true; }},
    {url: 'https://coven.example/old', navigate: async url => { navigation = url; return {focus: async () => { focused++; }}; }}]);
  await w.click({alert_id: id, url: 'https://evil.example', action: 'approve'});
  assert.equal(foreignUsed, false); assert.equal(focused, 1);
  assert.equal(navigation, `https://coven.example/#attention=${id}`); assert.equal(w.opened.length, 0);
  const closed = worker(); await closed.click(); assert.equal(closed.opened[0], `https://coven.example/#attention=${id}`);
  await closed.click({alert_id: id}, 'approve'); await closed.click({alert_id: '../secret'});
  assert.equal(closed.opened.length, 1);
});
