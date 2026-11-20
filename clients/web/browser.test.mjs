import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdir, mkdtemp, readFile, rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';

// Opt-in real DOM coverage using an installed Firefox, without browser downloads.
test('Firefox real DOM: unavailable roots, one-click bound reads, exact routes and focus fences',
  {skip: !process.env.COVEN_WEB_FIREFOX, timeout: 45000}, async () => {
    assert.notEqual(process.platform, 'win32', 'COVEN_WEB_FIREFOX automation requires Unix process-group cleanup; Windows is unsupported');
    assert.equal(typeof globalThis.WebSocket, 'function', 'COVEN_WEB_FIREFOX requires Node with built-in WebSocket support (Node 22+; validated with Node 26)');
    assert.ok(process.env.TMPDIR, 'run with a unique disk-backed TMPDIR');
    const profile = await mkdtemp(join(tmpdir(), 'firefox-'));
    let browser, socket, server;
    const timers = new Set();
    const pending = new Map();
    const cancelTimer = timer => { clearTimeout(timer); timers.delete(timer); };
    const timerAfter = (callback, ms) => {
      const timer = setTimeout(() => { timers.delete(timer); callback(); }, ms);
      timers.add(timer); return timer;
    };
    const requests = [];
    const target = {id: 'a'.repeat(64), label: 'Native source', team: 'research', host: 'Local native'};
    const targets = [target];
    let metadataStatus = 200;
    let fleetStatus = 200;
    let agents = ['e', 'f'].map((letter, index) => ({id: letter.repeat(64), title: index ? 'Megatron full root' : 'Bumblebee full root',
      provider: 'djinn', project: 'Original project', activity: 'unknown', evidence: 'kitsune_snapshot', output_available: false}));
    try {
      const browserTmp = join(profile, 'tmp');
      await mkdir(browserTmp);
      const assets = new Map(await Promise.all(['index.html', 'app.js', 'styles.css'].map(async name =>
        [`/${name}`, await readFile(new URL(name, import.meta.url), 'utf8')])));
      server = createServer((req, res) => {
        requests.push({method: req.method, path: req.url});
        res.setHeader('Cache-Control', 'no-store');
        if (req.url.startsWith('/api/')) {
          res.setHeader('Content-Type', 'application/json');
          const status = req.url === '/api/v1/conversations' ? metadataStatus : req.url === '/api/v1/fleet' ? fleetStatus : 200;
          if (status !== 200) { res.writeHead(status); res.end(JSON.stringify({error: {code: 'unavailable'}})); return; }
          const now = Date.now();
          const selected = targets.find(row => req.url.includes(`/conversations/${row.id}/bindings/`)) ?? target;
          const value = req.url === '/api/v1/fleet'
            ? {contract: 'coven.fleet-observer.v1', generated_at_ms: now, refresh_interval_ms: 3000, stale_after_ms: 15000,
              endpoints: [{id: 'desktop', label: 'Synthetic host', connection: 'reachable', last_attempt_at_ms: now,
                observed_at_ms: now, error: null, agents: agents.map(agent => ({...agent, observed_at_ms: now}))}]}
            : req.url === '/api/v1/conversations' ? {contract: 'coven.conversations.v1', conversations: targets}
              : {contract: 'coven.conversations.v1', id: selected.id, team: selected.team, source: 'djinn.conversation-read.v1',
                messages: [{role: 'assistant', text: '', truncated: false}, {role: 'user', text: '<script>literal</script>', truncated: false}],
                has_older: false, truncated: false, omitted_parts: true, projection_sha256: 'b'.repeat(64)};
          res.end(JSON.stringify(value));
        } else {
          const path = req.url === '/' ? '/index.html' : req.url;
          res.setHeader('Content-Type', path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : 'text/html');
          res.end(assets.get(path) ?? '');
        }
      });
      server.listen(0, '127.0.0.1'); await once(server, 'listening');
      browser = spawn(process.env.COVEN_WEB_FIREFOX, ['--headless', '--no-remote', '--profile', profile,
        '--remote-debugging-port', '0', 'about:blank'], {detached: true,
        env: {...process.env, TMPDIR: browserTmp}, stdio: ['ignore', 'ignore', 'pipe']});
      const endpoint = await new Promise((resolve, reject) => {
        const timer = timerAfter(() => reject(new Error('Firefox BiDi startup timeout')), 15000);
        let diagnostic = '';
        browser.stderr.on('data', data => {
          diagnostic = (diagnostic + data).slice(-8192);
          const match = diagnostic.match(/WebDriver BiDi listening on (ws:\/\/[^\s]+)/);
          if (match) { cancelTimer(timer); resolve(`${match[1]}/session`); }
        });
        browser.once('error', error => { cancelTimer(timer); reject(error); });
        browser.once('exit', code => { cancelTimer(timer); reject(new Error(`Firefox exited ${code}: ${diagnostic}`)); });
      });
      socket = new WebSocket(endpoint);
      const connection = new AbortController();
      const connectionTimer = timerAfter(() => connection.abort(), 10000);
      try { await once(socket, 'open', {signal: connection.signal}); }
      finally { cancelTimer(connectionTimer); }
      let serial = 0;
      socket.addEventListener('message', event => {
        const response = JSON.parse(event.data);
        const callback = pending.get(response.id);
        if (callback) { pending.delete(response.id); response.type === 'error' ? callback.reject(new Error(JSON.stringify(response))) : callback.resolve(response.result); }
      });
      const call = (method, params) => new Promise((resolve, reject) => {
        const id = ++serial;
        const timer = timerAfter(() => { pending.delete(id); reject(new Error(`BiDi timeout: ${method}`)); }, 10000);
        pending.set(id, {resolve: value => { cancelTimer(timer); resolve(value); }, reject: error => { cancelTimer(timer); reject(error); }});
        socket.send(JSON.stringify({id, method, params}));
      });
      await call('session.new', {capabilities: {alwaysMatch: {acceptInsecureCerts: false}}});
      const {context} = await call('browsingContext.create', {type: 'tab'});
      await call('browsingContext.setViewport', {context, viewport: {width: 390, height: 844}, devicePixelRatio: 1});
      await call('browsingContext.navigate', {context, url: `http://127.0.0.1:${server.address().port}/`, wait: 'complete'});
      const evaluate = async expression => {
        const value = await call('script.evaluate', {expression, target: {context}, awaitPromise: true});
        assert.equal(value.type, 'success', JSON.stringify(value)); return value.result.value;
      };
      const wait = async expression => {
        for (let i = 0; i < 100; i++) {
          if (await evaluate(expression)) return;
          await new Promise(resolve => timerAfter(resolve, 25));
        }
        assert.fail(`DOM condition timed out: ${expression}`);
      };
      const reads = () => requests.filter(req => req.path.endsWith('/messages'));
      await wait(`document.querySelectorAll('.session-card').length === 2 && document.getElementById('conversation-list-status').textContent.includes('1 shared session awaiting association')`);
      assert.equal(reads().length, 0);
      assert.equal(await evaluate(`!document.getElementById('chat-picker') && !document.getElementById('shared-chats')`), true);
      assert.equal(await evaluate(`[...document.querySelectorAll('.agent-card')].every(node => node.getAttribute('aria-disabled') === 'true' && node.textContent.includes('Chat unavailable'))`), true);
      assert.equal(await evaluate(`document.querySelectorAll('.agent-grid').length === 1 && !document.querySelector('button button')`), true);
      assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), true);
      await call('browsingContext.setViewport', {context, viewport: {width: 1280, height: 900}, devicePixelRatio: 1});
      assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), true);
      await evaluate(`document.querySelector('.agent-card').click()`);
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').open`), false);
      assert.equal(reads().length, 0);
      await call('browsingContext.setViewport', {context, viewport: {width: 390, height: 844}, devicePixelRatio: 1});
      await evaluate(`document.querySelector('.agent-card').focus(); document.querySelector('.agent-card').click()`);
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').open`), false);
      assert.equal(reads().length, 0);
      assert.equal(await evaluate(`document.activeElement === document.querySelector('.agent-card')`), true);
      await evaluate(`document.querySelector('.card-details').click()`);
      assert.match(await evaluate(`document.getElementById('detail-chat-status').textContent`), /Chat unavailable/);
      assert.equal(reads().length, 0);
      await evaluate(`document.getElementById('close-detail').click()`);
      const linked = {...target, association_status: 'verified', card: {fleet_agent_id: agents[0].id,
        fleet_endpoint_id: 'desktop', binding_id: 'c'.repeat(64)}};
      const second = {...target, id: 'd'.repeat(64), team: 'other-team', can_submit: true, max_prompt_bytes: 65536,
        association_status: 'verified', card: {fleet_agent_id: agents[1].id, fleet_endpoint_id: 'desktop', binding_id: 'b'.repeat(64)}};
      targets.push(linked, second);
      targets.splice(0, 1, {...target, id: '9'.repeat(64)}); // Unlinked history never creates another root or a chooser.
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelector('.agent-card').getAttribute('aria-disabled') === 'false'`);
      assert.equal(reads().length, 0);
      await evaluate(`document.querySelector('.agent-card').focus(); document.querySelector('.agent-card').click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 1`);
      assert.equal(reads().length, 1);
      assert.equal(reads()[0].path, `/api/v1/agents/${linked.card.fleet_agent_id}/conversations/${target.id}/bindings/${linked.card.binding_id}/messages`);
      assert.equal(await evaluate(`document.getElementById('prompt-composer').hidden`), true);
      assert.equal(await evaluate(`document.querySelector('.conversation-message pre').textContent`), '<script>literal</script>');
      assert.equal(await evaluate(`document.querySelector('.card-title').textContent`), 'Bumblebee full root');
      assert.match(await evaluate(`document.getElementById('conversation-link-status').textContent`), /Verified terminal link/);
      assert.equal(await evaluate(`document.activeElement.id`), 'close-conversation');
      await evaluate(`document.getElementById('close-conversation').click()`);
      await wait(`document.activeElement === document.querySelector('.agent-card')`);
      await call('browsingContext.setViewport', {context, viewport: {width: 1280, height: 900}, devicePixelRatio: 1});
      await evaluate(`document.querySelector('.agent-card').click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 1`);
      assert.equal(reads().length, 2);
      assert.equal(reads()[1].path, reads()[0].path);
      assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), true);
      await call('browsingContext.setViewport', {context, viewport: {width: 390, height: 844}, devicePixelRatio: 1});
      await evaluate(`document.getElementById('close-conversation').click(); document.querySelectorAll('.agent-card')[1].click()`);
      await wait(`document.getElementById('prompt-composer').hidden === false`);
      assert.equal(reads().length, 3);
      assert.equal(reads()[2].path, `/api/v1/agents/${second.card.fleet_agent_id}/conversations/${second.id}/bindings/${second.card.binding_id}/messages`);
      await evaluate(`document.getElementById('close-conversation').click()`);
      targets.push({...linked, id: '8'.repeat(64)});
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelector('.agent-card').textContent.includes('Multiple conversations claim')`);
      await evaluate(`document.querySelector('.agent-card').focus(); document.querySelector('.agent-card').click()`);
      assert.equal(reads().length, 3);
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').open`), false);
      targets.pop();
      metadataStatus = 503;
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`[...document.querySelectorAll('.agent-card')].every(node => node.getAttribute('aria-disabled') === 'true')`);
      await evaluate(`document.querySelectorAll('.agent-card')[1].click()`);
      assert.equal(reads().length, 3);
      metadataStatus = 200;
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelector('.agent-card').getAttribute('aria-disabled') === 'false'`);
      await evaluate(`document.querySelector('.agent-card').click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 1`);
      await evaluate(`document.getElementById('conversation-team').value = 'other-team'; document.getElementById('conversation-team').dispatchEvent(new Event('change'))`);
      await wait(`!document.getElementById('conversation-dialog').open && document.activeElement.id === 'host-filter'`);
      assert.equal(await evaluate(`document.querySelectorAll('.session-card').length`), 1);
      await evaluate(`document.getElementById('conversation-team').value = ''; document.getElementById('conversation-team').dispatchEvent(new Event('change'))`);
      fleetStatus = 503;
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`[...document.querySelectorAll('.agent-card')].every(node => node.getAttribute('aria-disabled') === 'true')`);
      await evaluate(`document.querySelector('.agent-card').click()`);
      assert.equal(reads().length, 4);
      fleetStatus = 200;
      agents = [];
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelectorAll('.session-card').length === 0`);
      assert.equal(reads().length, 4);
      fleetStatus = 403;
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.getElementById('conversation-list-status').textContent.includes('Access denied')`);
      assert.equal(reads().length, 4);
      assert.equal(requests.some(req => req.method === 'POST'), false);
    } finally {
      const errors = [];
      for (const timer of timers) cancelTimer(timer);
      pending.clear();
      try { socket?.close(); } catch (error) { errors.push(error); }
      // Closing the mock server must not depend on successful browser teardown.
      try {
        if (server) { server.closeAllConnections(); await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())); }
      } catch (error) { errors.push(error); }
      let stopped = !browser?.pid;
      try {
        if (browser?.pid) {
          // detached:true makes this child's PID its owned Unix process-group ID.
          const signalGroup = signal => {
            try { process.kill(-browser.pid, signal); return true; }
            catch (error) { if (error.code === 'ESRCH') return false; throw error; }
          };
          const waitForGroup = async ms => {
            const deadline = Date.now() + ms;
            do {
              const alive = signalGroup(0);
              const reaped = browser.exitCode !== null || browser.signalCode !== null;
              if (!alive && reaped) return true;
              await new Promise(resolve => timerAfter(resolve, 50));
            } while (Date.now() < deadline);
            return false;
          };
          signalGroup('SIGTERM');
          stopped = await waitForGroup(3000);
          if (!stopped) {
            signalGroup('SIGKILL');
            stopped = await waitForGroup(3000);
          }
          if (!stopped) throw new Error(`Owned Firefox group ${browser.pid} or its unreaped main process remains after TERM/KILL; profile retained: ${profile}`);
        }
      } catch (error) { errors.push(error); }
      finally { for (const timer of timers) cancelTimer(timer); }
      if (stopped) {
        try { await rm(profile, {recursive: true, force: true}); }
        catch (error) { errors.push(error); }
      }
      if (errors.length) throw new AggregateError(errors, `Firefox fixture cleanup failed; profile path: ${profile}${stopped ? ' (process group stopped)' : ' (retained; verify owned group has stopped before removal)'}`);
    }
  });
