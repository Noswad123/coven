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
  {skip: !process.env.COVEN_WEB_FIREFOX, timeout: 45000}, async t => {
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
    let emptyHostState = null;
    let requestPending = true;
    let longHistory = false, holdMessages = false, holdDecisions = false, requestKind = 'question';
    const delayedMessages = [], delayedDecisions = [];
    const latestText = `${'Long latest message, not a nested scroll box.\n'.repeat(100)}LATEST MARKER`;
    assert.ok(Buffer.byteLength(latestText) + 1200 < 8192);
    const decisionID = '1'.repeat(64), alertID = '2'.repeat(64);
    let agents = ['e', 'f'].map((letter, index) => ({id: letter.repeat(64), pane_name: index ? 'Megatron' : 'OC | Bumblebee <b>literal</b>', title: index ? 'Megatron full root' : 'OC | Bumblebee full root',
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
          const linked = targets.find(row => row.card?.fleet_agent_id === agents[0]?.id);
          if (req.url === `/api/v1/decisions/${decisionID}/responses` && req.method === 'POST') {
            let body = '';
            req.on('data', chunk => { body += chunk; });
            req.on('end', () => {
              const value = JSON.parse(body);
              assert.deepEqual(value.action, {type: 'question_answer', answers: [['First'], ['Second', 'Custom literal']]});
              requestPending = false;
              res.end(JSON.stringify({contract: 'coven.decision-response.v1', id: decisionID, attempt_id: value.attempt_id,
                action: 'question_answer', outcome: 'applied', delivery: 'applied'}));
            }); return;
          }
          const value = req.url === '/api/v1/fleet'
            ? {contract: 'coven.fleet-observer.v1', generated_at_ms: now, refresh_interval_ms: 3000, stale_after_ms: 15000,
              endpoints: [{id: 'desktop', label: 'Synthetic host', connection: 'reachable', last_attempt_at_ms: now,
                observed_at_ms: now, error: null, agents: agents.map(agent => ({...agent, observed_at_ms: now}))},
                ...(emptyHostState ? [{id: 'empty-host', label: 'Megatron', connection: emptyHostState,
                  last_attempt_at_ms: now, observed_at_ms: now, error: null, agents: []}] : [])]}
            : req.url === '/api/v1/conversations' ? {contract: 'coven.conversations.v1', conversations: targets}
              : req.url === '/api/v1/attention' ? {contract: 'coven.attention.v1', state: 'observed', omitted_count: 0, unavailable_count: 0, unlinked_count: 0,
                items: linked && requestPending ? [{id: decisionID, alert_id: alertID, conversation_id: linked.id, team: linked.team,
                  ...linked.card, kind: requestKind, can_respond: true, details_available: true}] : []}
              : req.url === '/api/v1/csrf' ? {contract: 'coven.csrf.v1', token: `0-${'a'.repeat(64)}-${'b'.repeat(64)}`}
              : req.url === `/api/v1/decisions/${decisionID}` ? {contract: 'coven.decision.v1', id: decisionID, alert_id: alertID, kind: requestKind, can_respond: true,
                decision: requestKind === 'permission' ? {kind: 'permission', permission: 'Run deployment?', patterns: ['complete pattern'],
                  always_patterns: ['native pattern'], metadata_json: 'Diagnostic\n'.repeat(200), deny_scope: 'session_pending'}
                : {kind: 'question', questions: [{question: '<script>Full first question</script>', header: 'Single',
                  options: [{label: 'First', description: '<b>Complete description</b>'}], multiple: false, custom: false},
                {question: 'Full second question', header: 'Multiple', options: [{label: 'Second', description: 'Complete second choice'}], multiple: true, custom: true}]}}
              : {contract: 'coven.conversations.v1', id: selected.id, team: selected.team, source: 'djinn.conversation-read.v1',
                messages: longHistory ? [{role: 'user', text: 'Earlier history\n'.repeat(70), truncated: false},
                  {role: 'assistant', text: latestText, truncated: false}]
                  : [{role: 'assistant', text: '', truncated: false}, {role: 'user', text: '<script>literal</script>', truncated: false}],
                has_older: false, truncated: false, omitted_parts: true, projection_sha256: 'b'.repeat(64)};
          const send = () => res.end(JSON.stringify(value));
          if (req.url.endsWith('/messages') && holdMessages) delayedMessages.push(send);
          else if (req.url === `/api/v1/decisions/${decisionID}` && holdDecisions) delayedDecisions.push(send);
          else send();
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
      await wait(`document.querySelectorAll('.session-card').length === 2 && document.getElementById('conversation-list-status').textContent === ''`);
      assert.equal(reads().length, 0);
      assert.equal(await evaluate(`!document.getElementById('chat-picker') && !document.getElementById('shared-chats')`), true);
      assert.equal(await evaluate(`[...document.querySelectorAll('.agent-card')].every(node => node.getAttribute('aria-disabled') === 'true' && node.textContent.includes('Chat unavailable'))`), true);
      assert.equal(await evaluate(`document.querySelectorAll('.agent-grid').length === 1 && !document.querySelector('button button')`), true);
      assert.equal(await evaluate(`document.getElementById('alerts-toggle').textContent === 'Alerts off' && document.getElementById('alerts-toggle').getAttribute('aria-pressed') === 'false' && document.getElementById('alerts-error').hidden`), true);
      assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), true);
      for (const width of [390, 320, 1280]) {
        await call('browsingContext.setViewport', {context, viewport: {width, height: 844}, devicePixelRatio: 1});
        assert.equal(await evaluate(`(() => {
          const card = document.querySelector('.agent-card');
          const lines = ['.card-title', '.card-provider', '.card-session-title'].map(s => card.querySelector(s));
          return lines.map(n => n.textContent).join('\\n') === 'OC | Bumblebee <b>literal</b>\\ndjinn\\nBumblebee full root'
            && lines[0].getBoundingClientRect().bottom <= lines[1].getBoundingClientRect().top
            && lines[1].getBoundingClientRect().bottom <= lines[2].getBoundingClientRect().top
            && !card.querySelector('b') && !/Provider identity|Coven process evidence|Process family hints|Original project|Chat team/.test(card.textContent)
            && document.documentElement.scrollWidth <= innerWidth;
        })()`), true, `pane/provider/session hierarchy at ${width}px`);
      }
      const originalTitle = agents[0].title;
      for (const title of ['x'.repeat(4096), '😀'.repeat(1024)]) {
        agents[0].title = title;
        await evaluate(`document.getElementById('refresh').click()`);
        await wait(`document.querySelector('.card-session-title').textContent === ${JSON.stringify(title)}`);
        for (const width of [320, 390, 1280]) {
          await call('browsingContext.setViewport', {context, viewport: {width, height: 844}, devicePixelRatio: 1});
          assert.equal(await evaluate(`(() => {
            const line = document.querySelector('.card-session-title');
            return line.textContent === ${JSON.stringify(title)}
              && line.scrollHeight <= line.clientHeight + 1
              && getComputedStyle(line).textOverflow !== 'ellipsis'
              && getComputedStyle(line).maxHeight === 'none'
              && document.documentElement.scrollWidth <= innerWidth;
          })()`), true, `complete 4096-byte title wraps at ${width}px`);
        }
      }
      agents[0].title = originalTitle;
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelector('.card-session-title').textContent === 'Bumblebee full root'`);
      assert.equal(reads().length, 0);
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
      assert.equal(await evaluate(`document.getElementById('detail-title').textContent`), agents[0].pane_name);
      assert.match(await evaluate(`document.getElementById('detail-meta').textContent`), /Kitsune metadata.*Original project.*Process discovery/);
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
      await wait(`document.querySelector('.agent-card').textContent.includes('Needs response')`);
      assert.equal(reads().length, 0);
      assert.equal(requests.some(req => req.path.startsWith('/api/v1/decisions/')), false);
      await evaluate(`document.querySelector('.agent-card').focus(); document.querySelector('.agent-card').click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 1`);
      assert.equal(reads().length, 1);
      assert.equal(reads()[0].path, `/api/v1/agents/${linked.card.fleet_agent_id}/conversations/${target.id}/bindings/${linked.card.binding_id}/messages`);
      assert.equal(await evaluate(`document.getElementById('prompt-composer').hidden`), true);
      assert.equal(await evaluate(`document.querySelector('.conversation-message pre').textContent`), '<script>literal</script>');
      assert.equal(await evaluate(`document.querySelector('.card-title').textContent`), 'OC | Bumblebee <b>literal</b>');
      assert.equal(await evaluate(`document.getElementById('conversation-link-status') === null`), true);
      assert.equal(await evaluate(`document.activeElement.id`), 'close-conversation');
      await wait(`document.querySelectorAll('#decision-details fieldset').length === 2`);
      assert.equal(await evaluate(`document.querySelector('#decision-details pre').textContent`), '<script>Full first question</script>');
      assert.equal(await evaluate(`document.querySelector('#decision-details script') === null`), true);
      assert.equal(await evaluate(`document.querySelectorAll('#decision-details input[type=radio]').length`), 1);
      assert.equal(await evaluate(`document.querySelectorAll('#decision-details input[type=checkbox]').length`), 1);
      await evaluate(`document.querySelector('#decision-details input[type=radio]').click(); document.querySelector('#decision-details input[type=checkbox]').click(); document.querySelector('#decision-details textarea').value = 'Custom literal'; [...document.querySelectorAll('#decision-details button')].find(b => b.textContent === 'Answer once').click()`);
      await wait(`document.getElementById('decision-status').textContent.includes('native wait settled, not tool completion')`);
      assert.equal(requests.filter(req => req.method === 'POST').length, 1);
      const polish = async () => {
      // Long history and late layouts are exercised on the actual outer scroller.
      fleetStatus = 200; metadataStatus = 200; longHistory = true;
      agents = ['e', 'f'].map((letter, index) => ({id: letter.repeat(64), pane_name: index ? 'Megatron' : 'Bumblebee', title: index ? 'Megatron full root' : 'Bumblebee full root',
        provider: 'djinn', project: 'Original project', activity: 'unknown', evidence: 'kitsune_snapshot', output_available: false}));
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelectorAll('.agent-card').length === 2 && document.querySelectorAll('.agent-card')[1].getAttribute('aria-disabled') === 'false'`);
      const frames = () => evaluate(`new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))`);
      const visible = selector => evaluate(`(() => {
        const node = document.querySelector(${JSON.stringify(selector)}), r = node.getBoundingClientRect();
        const d = document.getElementById('conversation-dialog').getBoundingClientRect();
        return r.top >= d.top + 60 && r.bottom <= d.bottom && r.height > 0;
      })()`);
      const latestVisible = () => evaluate(`(() => {
        const pre = document.querySelector('.conversation-message:last-child pre');
        const range = document.createRange(); range.setStart(pre.firstChild, pre.textContent.indexOf('LATEST MARKER')); range.setEnd(pre.firstChild, pre.textContent.length);
        const r = range.getBoundingClientRect(), d = document.getElementById('conversation-dialog').getBoundingClientRect();
        return r.top >= d.top + 60 && r.bottom <= d.bottom;
      })()`);
      for (const width of [320, 390, 1280]) {
        await call('browsingContext.setViewport', {context, viewport: {width, height: width === 1280 ? 900 : 844}, devicePixelRatio: 1});
        assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth && document.getElementById('alerts-toggle').getBoundingClientRect().height >= 44`), true);
        assert.equal(await evaluate(`document.querySelectorAll('.masthead button').length === 1 && !document.getElementById('host-health')`), true);
        assert.equal(await evaluate(`document.getElementById('alerts-toggle').textContent === 'Alerts unknown' && document.getElementById('alerts-toggle').getAttribute('aria-pressed') === 'mixed' && !document.getElementById('alerts-error').hidden`), true, 'access loss keeps one honest header control');
      }
      for (const connection of ['unavailable', 'unsupported']) {
        emptyHostState = connection;
        await evaluate(`document.getElementById('refresh').click()`);
        await wait(`!document.getElementById('refresh').disabled && document.getElementById('notice').textContent === 'Megatron ${connection}.'`);
        assert.equal(await evaluate(`!document.getElementById('notice').hidden && document.getElementById('notice').getBoundingClientRect().height > 0 && document.querySelectorAll('.session-card').length === 2 && document.getElementById('empty-state').hidden`), true);
        await evaluate(`document.getElementById('host-filter').value = 'desktop'; document.getElementById('host-filter').dispatchEvent(new Event('change'))`);
        assert.equal(await evaluate(`document.getElementById('notice').hidden && document.getElementById('notice').textContent === ''`), true);
        await evaluate(`document.getElementById('host-filter').value = ''; document.getElementById('host-filter').dispatchEvent(new Event('change'))`);
        assert.equal(await evaluate(`!document.getElementById('notice').hidden`), true);
      }
      emptyHostState = 'reachable';
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`!document.getElementById('refresh').disabled && document.getElementById('notice').hidden`);
      assert.equal(await evaluate(`document.getElementById('notice').textContent === ''`), true);
      for (const width of [390, 1280]) {
        await call('browsingContext.setViewport', {context, viewport: {width, height: width === 1280 ? 900 : 844}, devicePixelRatio: 1});
        await evaluate(`document.querySelectorAll('.agent-card')[1].click()`);
        await wait(`document.querySelectorAll('.conversation-message').length === 2`); await frames();
        assert.equal(await latestVisible(), true, `latest marker visible at ${width}`);
        assert.equal(await visible('#prompt-text'), true, `composer visible at ${width}`);
        assert.equal(await visible('#send-prompt'), true);
        assert.equal(await evaluate(`document.activeElement.id`), 'close-conversation');
        assert.equal(await evaluate(`getComputedStyle(document.querySelector('.conversation-message pre')).maxHeight`), 'none');
        await evaluate(`(() => { const d = document.getElementById('conversation-dialog'); d.dispatchEvent(new WheelEvent('wheel', {deltaY: -600})); d.scrollTop = 250; })()`);
        await frames();
        const top = await evaluate(`document.getElementById('conversation-dialog').scrollTop`);
        const readCount = reads().length;
        await evaluate(`document.getElementById('refresh').click()`);
        await wait(`!document.getElementById('refresh').disabled`); await frames();
        assert.equal(await evaluate(`document.getElementById('conversation-dialog').scrollTop`), top, 'metadata never repositions history');
        assert.equal(reads().length, readCount, 'metadata refresh never reads transcripts');
        await evaluate(`document.getElementById('read-conversation').click()`);
        await wait(`document.querySelectorAll('.conversation-message').length === 2 && !document.getElementById('read-conversation').disabled`); await frames();
        assert.equal(await evaluate(`document.getElementById('conversation-dialog').scrollTop`), top, 'explicit refresh restores numeric history position');
        await evaluate(`document.getElementById('close-conversation').click(); document.querySelectorAll('.agent-card')[1].click()`);
        await wait(`document.querySelectorAll('.conversation-message').length === 2`); await frames();
        assert.equal(await latestVisible(), true, 'new opening resets to latest');
        await evaluate(`document.getElementById('prompt-text').focus()`);
        await call('browsingContext.setViewport', {context, viewport: {width, height: 650}, devicePixelRatio: 1});
        assert.equal(await evaluate(`document.activeElement.id`), 'prompt-text', 'resize preserves composer focus');
        await evaluate(`document.getElementById('close-conversation').click()`);
      }
      await call('browsingContext.setViewport', {context, viewport: {width: 390, height: 844}, devicePixelRatio: 1});
      requestPending = true;
      for (const kind of ['question', 'permission']) {
        requestKind = kind;
        if (kind === 'permission') agents[0].pane_name = 'Bumblebee coordinating the extended research review across several independent project workstreams';
        await evaluate(`document.getElementById('refresh').click()`);
        await wait(`document.querySelector('.agent-card').textContent.includes('Needs response')`);
        // Let the explicit refresh finish before opening the current occurrence.
        await wait(`!document.getElementById('refresh').disabled`); await frames();
        for (const width of kind === 'permission' ? [390, 320] : [390]) {
        await call('browsingContext.setViewport', {context, viewport: {width, height: 844}, devicePixelRatio: 1});
        for (const first of ['messages', 'decision']) {
          const assertMeaning = async phase => {
            const geometry = JSON.parse(await evaluate(`(() => {
              const n = document.querySelector('#decision-details legend, #decision-details h3'), r = n.getBoundingClientRect();
              const header = document.querySelector('#conversation-dialog > .dialog-heading').getBoundingClientRect();
              const title = document.getElementById('conversation-title'), d = document.getElementById('conversation-dialog').getBoundingClientRect();
              return JSON.stringify({top: r.top, bottom: r.bottom, headerBottom: header.bottom, dialogBottom: d.bottom,
                titleLines: Math.round(title.getBoundingClientRect().height / parseFloat(getComputedStyle(title).lineHeight))});
            })()`));
            assert.ok(geometry.top >= geometry.headerBottom + 7, `${kind} meaning clears the actual header: ${JSON.stringify(geometry)}`);
            assert.ok(geometry.bottom <= geometry.dialogBottom, 'full request heading is visible, not just response buttons');
            if (kind === 'permission') {
              assert.ok(geometry.titleLines >= 3, `long title really wraps at ${width}px`);
              assert.equal(await evaluate(`document.getElementById('conversation-title').textContent`), agents[0].pane_name);
              assert.equal(await evaluate(`document.activeElement.id`), 'close-conversation');
              t.diagnostic(`${width}px ${first}-first ${phase}: header bottom ${geometry.headerBottom.toFixed(1)}, permission top ${geometry.top.toFixed(1)}, title ${geometry.titleLines} lines`);
            }
          };
          holdMessages = true; holdDecisions = true;
          await evaluate(`document.querySelector('.agent-card').click()`);
          await wait(`document.getElementById('conversation-dialog').open`);
          for (let i = 0; i < 100 && (!delayedMessages.length || !delayedDecisions.length); i++) await new Promise(resolve => timerAfter(resolve, 10));
          assert.equal(delayedMessages.length, 1); assert.equal(delayedDecisions.length, 1);
          const release = which => (which === 'messages' ? delayedMessages : delayedDecisions).shift()();
          release(first);
          await wait(first === 'messages' ? `document.querySelectorAll('.conversation-message').length === 2` : `document.getElementById('decision-details').children.length > 0`);
          await frames();
          if (first === 'decision') await assertMeaning('before history');
          release(first === 'messages' ? 'decision' : 'messages');
          await wait(`document.querySelectorAll('.conversation-message').length === 2 && document.getElementById('decision-details').children.length > 0`); await frames();
          await assertMeaning('complete');
          assert.notEqual(await evaluate(`document.activeElement.tagName`), 'TEXTAREA');
          await evaluate(`document.getElementById('close-conversation').click()`);
          holdMessages = false; holdDecisions = false;
        }
        }
      }
      // User interaction after the request arrives must beat a later history read.
      holdMessages = true;
      await evaluate(`document.querySelector('.agent-card').click()`);
      await wait(`document.querySelector('#decision-details h3')?.textContent === 'Run deployment?'`); await frames();
      await evaluate(`(() => { const d = document.getElementById('conversation-dialog'); d.dispatchEvent(new KeyboardEvent('keydown', {key: 'Home'})); d.scrollTop = 0; })()`); await frames();
      delayedMessages.shift()();
      await wait(`document.querySelectorAll('.conversation-message').length === 2`); await frames();
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').scrollTop`), 0);
      await evaluate(`document.getElementById('close-conversation').click()`); holdMessages = false;
      // Hold an actual layout callback across close/reopen and a different target.
      await evaluate(`window.savedRAF = window.requestAnimationFrame; window.heldFrames = []; window.requestAnimationFrame = callback => { heldFrames.push(callback); return heldFrames.length; }; document.querySelector('.agent-card').click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 2 && heldFrames.length >= 1`);
      await evaluate(`window.staleFrames = heldFrames.splice(0); document.getElementById('close-conversation').click(); document.querySelectorAll('.agent-card')[1].click()`);
      await wait(`document.querySelectorAll('.conversation-message').length === 2`);
      await evaluate(`document.getElementById('conversation-dialog').scrollTop = 123; staleFrames.forEach(callback => callback()); window.requestAnimationFrame = savedRAF`);
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').scrollTop`), 123, 'closed-view frame cannot scroll a new target');
      await evaluate(`document.getElementById('close-conversation').click()`);
      assert.equal(requests.filter(req => req.method === 'POST').length, 1, 'scrolling, layout and metadata never perform actions');
      };
      assert.equal(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), true);
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
      // Composer visibility precedes the cross-process message request completing.
      await wait(`document.getElementById('prompt-composer').hidden === false && document.querySelectorAll('.conversation-message').length === 1 && !document.getElementById('read-conversation').disabled`);
      assert.equal(reads().length, 3);
      assert.equal(reads()[2].path, `/api/v1/agents/${second.card.fleet_agent_id}/conversations/${second.id}/bindings/${second.card.binding_id}/messages`);
      await evaluate(`document.getElementById('close-conversation').click()`);
      targets.push({...linked, id: '8'.repeat(64)});
      await evaluate(`document.getElementById('refresh').click()`);
      await wait(`document.querySelector('.agent-card').getAttribute('aria-disabled') === 'true'`);
      await evaluate(`document.querySelector('.agent-card').focus(); document.querySelector('.agent-card').click()`);
      assert.equal(reads().length, 3);
      assert.equal(await evaluate(`document.getElementById('conversation-dialog').open`), false);
      await evaluate(`document.querySelector('.card-details').click()`);
      assert.match(await evaluate(`document.getElementById('detail-chat-status').textContent`), /Multiple conversations claim this terminal/);
      await evaluate(`document.getElementById('close-detail').click()`);
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
      assert.equal(requests.filter(req => req.method === 'POST').length, 1);
      await polish();
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
