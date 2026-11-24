// Opt-in acceptance: COVEN_WEB_BINARY=/absolute/built/coven and
// COVEN_WEB_FIREFOX=/absolute/installed/firefox; skipped unless BOTH are set.
// Requires Unix, Node 22+ WebSocket, and a caller-provided disk-backed TMPDIR.
// Never builds/downloads anything or uses native sources, user sockets or services.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createServer, request} from 'node:http';
import {connect} from 'node:net';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {access, mkdir, mkdtemp, rm, statfs, writeFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';
import {tmpdir} from 'node:os';

test('Firefox native EventSource: actual gateway invalidation and reconnect never auto-read content',
  {skip: !process.env.COVEN_WEB_BINARY || !process.env.COVEN_WEB_FIREFOX, timeout: 60000}, async () => {
    assert.notEqual(process.platform, 'win32', 'owned Unix process groups required');
    assert.equal(typeof globalThis.WebSocket, 'function', 'Node 22+ WebSocket required');
    assert.ok(process.env.TMPDIR && isAbsolute(process.env.TMPDIR), 'provide an absolute disk-backed TMPDIR');
    for (const key of ['COVEN_WEB_BINARY', 'COVEN_WEB_FIREFOX']) {
      assert.ok(isAbsolute(process.env[key]), `${key} must be absolute`);
      await access(process.env[key]);
    }
    const disk = await statfs(tmpdir());
    assert.ok(![0x01021994, 0x858458f6].includes(disk.type), 'TMPDIR must not be tmpfs/ramfs');
    assert.ok(disk.bavail * disk.bsize > 64 * 1024 * 1024, 'TMPDIR needs at least 64 MiB free');
    let root, reservation, proxy, socket;
    const children = [], timers = new Set(), pending = new Map(), upstreams = new Set();
    const requests = [], streams = [], failures = [];
    const deadline = performance.now() + 40000;
    const later = (fn, ms) => {
      const timer = setTimeout(() => { timers.delete(timer); fn(); }, ms);
      timers.add(timer); return timer;
    };
    const cancel = timer => { clearTimeout(timer); timers.delete(timer); };
    const sleep = ms => new Promise(resolve => later(resolve, ms));
    const check = () => {
      if (failures.length) throw failures[0];
      for (const child of children) {
        assert.ok(child.exitCode === null && child.signalCode === null,
          `${child.label} exited: ${child.diagnostic}`);
      }
      assert.ok(performance.now() < deadline, 'acceptance deadline exceeded');
    };
    const until = async (condition, label, ms = 10000) => {
      const end = Math.min(deadline, performance.now() + ms);
      do {
        check();
        if (await condition()) return;
        await sleep(25);
      } while (performance.now() < end);
      assert.fail(`Timed out: ${label}`);
    };
    const launch = (label, executable, args, env) => {
      const child = spawn(executable, args, {detached: true, env, stdio: ['ignore', 'ignore', 'pipe']});
      child.label = label; child.diagnostic = '';
      children.push(child);
      child.stderr.on('data', data => {
        child.diagnostic = Buffer.concat([Buffer.from(child.diagnostic), data]).subarray(-8192).toString();
      });
      child.on('error', error => failures.push(error));
      return child;
    };
    const closeServer = async server => {
      if (!server) return;
      // Must destroy SSE sockets BEFORE awaiting close's callback.
      server.closeAllConnections();
      if (server.listening) await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    };
    try {
      root = await mkdtemp(join(tmpdir(), 'coven-live-events-'));
      const profile = join(root, 'profile'), browserTmp = join(root, 'firefox-tmp'), gatewayTmp = join(root, 'gateway-tmp');
      await Promise.all([profile, browserTmp, gatewayTmp].map(path => mkdir(path)));
      reservation = createServer();
      reservation.listen(0, '127.0.0.1'); await once(reservation, 'listening');
      const port = reservation.address().port;
      const config = join(root, 'observer.json');
      await writeFile(config, JSON.stringify({listen: `127.0.0.1:${port}`,
        public_origin: 'https://coven-web.example.invalid', allowed_logins: ['fixture@example.invalid'],
        endpoints: [{id: 'fixture', label: 'Absent fixture Kit', socket: join(root, 'absent-kit.sock')}],
        conversations: [], conversation_sources: []}));
      await closeServer(reservation); reservation = null;
      launch('gateway', process.env.COVEN_WEB_BINARY, ['observer', 'serve', '--config', config],
        {...process.env, TMPDIR: gatewayTmp});
      await until(() => new Promise(resolve => {
        const probe = connect({host: '127.0.0.1', port});
        probe.setTimeout(200);
        probe.once('connect', () => { probe.destroy(); resolve(true); });
        probe.once('error', () => { probe.destroy(); resolve(false); });
        probe.once('timeout', () => { probe.destroy(); resolve(false); });
      }), 'isolated gateway startup', 5000);

      proxy = createServer((req, res) => {
        const row = {method: req.method, path: req.url, at: performance.now(), done: false};
        requests.push(row);
        res.on('finish', () => { row.done = true; });
        // Forward bytes, not fixture JSON or locally loaded assets. Only the trust
        // boundary headers are supplied here; browser Origin is not gateway Origin.
        const headers = {host: 'coven-web.example.invalid', 'Tailscale-User-Login': 'fixture@example.invalid'};
        if (req.headers.accept) headers.accept = req.headers.accept;
        const upstream = request({host: '127.0.0.1', port, path: req.url, method: req.method, headers}, response => {
          response.on('error', () => res.destroy());
          res.writeHead(response.statusCode, response.headers);
          if (req.url !== '/api/v1/events') { response.pipe(res); return; }
          res.flushHeaders();
          const stream = {res, upstream, headers: response.headers, status: response.statusCode,
            frames: [], held: [], released: false, tail: '',
            initialCounts: ['/api/v1/fleet', '/api/v1/conversations', '/api/v1/attention'].map(path =>
              requests.filter(request => request.path === path).length)};
          streams.push(stream);
          // Split only at SSE frame boundaries. Holding a genuine metadata frame
          // isolates its effect from initial ready GETs without inventing events.
          response.setEncoding('utf8');
          response.on('data', chunk => {
            stream.tail += chunk;
            if (stream.tail.length > 8192) { failures.push(new Error('unbounded SSE frame')); upstream.destroy(); return; }
            let boundary;
            while ((boundary = stream.tail.indexOf('\n\n')) !== -1) {
              const frame = stream.tail.slice(0, boundary + 2);
              stream.tail = stream.tail.slice(boundary + 2);
              if (stream.frames.length < 128) stream.frames.push(frame);
              else { failures.push(new Error('too many SSE frames')); upstream.destroy(); return; }
              if (!stream.released && (stream.held.length || frame.startsWith('event: metadata\n'))) stream.held.push(frame);
              else res.write(frame);
            }
          });
          response.on('end', () => res.end());
        });
        upstreams.add(upstream);
        upstream.on('close', () => upstreams.delete(upstream));
        upstream.on('error', () => res.destroy());
        res.on('close', () => upstream.destroy());
        req.pipe(upstream);
      });
      proxy.listen(0, '127.0.0.1'); await once(proxy, 'listening');
      const browser = launch('Firefox', process.env.COVEN_WEB_FIREFOX,
        ['--headless', '--no-remote', '--profile', profile, '--remote-debugging-port', '0', 'about:blank'],
        {...process.env, TMPDIR: browserTmp});
      await until(() => /WebDriver BiDi listening on (ws:\/\/[^\s]+)/.test(browser.diagnostic), 'Firefox BiDi startup', 15000);
      const endpoint = browser.diagnostic.match(/WebDriver BiDi listening on (ws:\/\/[^\s]+)/)[1];
      socket = new WebSocket(`${endpoint}/session`);
      const connection = new AbortController();
      const connectionTimer = later(() => connection.abort(), 5000);
      try { await once(socket, 'open', {signal: connection.signal}); }
      finally { cancel(connectionTimer); }
      let serial = 0;
      socket.addEventListener('message', event => {
        const message = JSON.parse(event.data), callback = pending.get(message.id);
        if (callback) {
          pending.delete(message.id);
          message.type === 'error' ? callback.reject(new Error(JSON.stringify(message))) : callback.resolve(message.result);
        }
      });
      const call = (method, params) => new Promise((resolve, reject) => {
        check();
        const id = ++serial;
        const timer = later(() => { pending.delete(id); reject(new Error(`BiDi timeout: ${method}`)); }, 5000);
        pending.set(id, {resolve: value => { cancel(timer); resolve(value); }, reject: error => { cancel(timer); reject(error); }});
        socket.send(JSON.stringify({id, method, params}));
      });
      await call('session.new', {capabilities: {alwaysMatch: {acceptInsecureCerts: false}}});
      await call('script.addPreloadScript', {functionDeclaration: `() => {
        const Native = globalThis.EventSource;
        globalThis.eventTrace = {instances: [], events: [], closes: []};
        globalThis.EventSource = class extends Native {
          constructor(...args) {
            super(...args);
            const id = eventTrace.instances.length;
            eventTrace.instances.push({id, url: this.url, native: this instanceof Native, at: performance.now()});
            for (const type of ['ready', 'metadata', 'fleet', 'error']) this.addEventListener(type, event => {
              eventTrace.events.push({id, type, data: event.data ?? null, at: performance.now()});
            });
            this.traceID = id;
          }
          close() { eventTrace.closes.push(this.traceID); return super.close(); }
        };
      }`});
      const {context} = await call('browsingContext.create', {type: 'tab'});
      await call('browsingContext.navigate', {context, url: `http://127.0.0.1:${proxy.address().port}/`, wait: 'complete'});
      const evaluate = async expression => {
        const result = await call('script.evaluate', {expression, target: {context}, awaitPromise: true});
        assert.equal(result.type, 'success', JSON.stringify(result)); return result.result.value;
      };
      const trace = async () => JSON.parse(await evaluate('JSON.stringify(eventTrace)'));
      const metadataPaths = ['/api/v1/conversations', '/api/v1/attention'];
      const count = path => requests.filter(row => row.path === path && row.method === 'GET').length;
      const idle = async () => requests.filter(row => row.path.startsWith('/api/') && row.path !== '/api/v1/events').every(row => row.done)
        && await evaluate(`!document.getElementById('refresh').disabled`);
      const exerciseMetadata = async index => {
        await until(async () => (await trace()).events.some(event => event.id === index && event.type === 'ready')
          && streams[index]?.held.some(frame => frame.startsWith('event: metadata\n')) && await idle(),
        `stream ${index} ready, real metadata queued and app reads settled`, 7000);
        const stream = streams[index];
        assert.equal(stream.status, 200);
        assert.equal(stream.headers['content-type'], 'text/event-stream');
        assert.equal(stream.headers['cache-control'], 'no-store');
        assert.equal(stream.headers['access-control-allow-origin'], undefined);
        if (index === 1) {
          for (const [i, path] of ['/api/v1/fleet', ...metadataPaths].entries()) {
            assert.ok(count(path) > stream.initialCounts[i], `new ready resynchronizes ${path} before releasing metadata`);
          }
        }
        const baseline = metadataPaths.map(count);
        const lastRead = Math.max(...requests.filter(row => metadataPaths.includes(row.path)).map(row => row.at));
        const releasedAt = performance.now();
        assert.ok(releasedAt - lastRead < 4500, 'metadata released before six-second fallback is eligible');
        stream.released = true;
        for (const frame of stream.held.splice(0)) stream.res.write(frame);
        await until(async () => (await trace()).events.some(event => event.id === index && event.type === 'metadata')
          && metadataPaths.every((path, i) => count(path) > baseline[i]) && await idle(),
        `stream ${index} metadata invalidation drives both normal GETs`, 1200);
        for (const path of metadataPaths) {
          const read = requests.find(row => row.path === path && row.at >= releasedAt);
          assert.ok(read && read.method === 'GET' && read.at - lastRead < 6000,
            `${path} fetched after metadata and before fallback, without manual refresh`);
        }
      };
      await exerciseMetadata(0);
      const beforeReconnect = ['/api/v1/fleet', ...metadataPaths].map(count);
      const disconnectedAt = performance.now();
      streams[0].res.destroy(); streams[0].upstream.destroy();
      await until(async () => (await trace()).events.some(event => event.id === 0 && event.type === 'error'), 'native disconnect error');
      await until(async () => (await trace()).instances.length >= 2 && streams.length >= 2, 'app six-second reconnect', 9500);
      assert.ok(requests.filter(row => row.path === '/api/v1/events')[1].at - disconnectedAt >= 5500,
        'app reconnect delay, not native automatic retry');
      await exerciseMetadata(1);
      for (const [i, path] of ['/api/v1/fleet', ...metadataPaths].entries()) {
        assert.ok(count(path) > beforeReconnect[i], `disconnect/reconnect resynchronizes ${path}`);
      }
      const observed = await trace();
      assert.equal(observed.instances.length, 2, 'only app-created native EventSources, no independent probe');
      assert.ok(observed.instances.every(instance => instance.native && instance.url.endsWith('/api/v1/events')));
      assert.ok(observed.closes.includes(0), 'app explicitly closes failed EventSource');
      for (const id of [0, 1]) {
        const events = observed.events.filter(event => event.id === id && event.type !== 'error');
        assert.equal(events[0].type, 'ready');
        assert.ok(events.some(event => event.type === 'metadata'));
        for (const event of events) assert.equal(event.data, '{"contract":"coven.events.v1"}');
      }
      for (const stream of streams) for (const frame of stream.frames) {
        assert.ok([': heartbeat\n\n', ...['ready', 'fleet', 'metadata'].map(type =>
          `event: ${type}\ndata: {"contract":"coven.events.v1"}\n\n`)].includes(frame),
        `exact static content-free SSE frame: ${JSON.stringify(frame)}`);
      }
      assert.equal(await evaluate(`document.getElementById('poll-status').textContent.includes('Live metadata updates.')`), false,
        'transport readiness does not imply native metadata availability');
      assert.equal(requests.some(row => row.method === 'POST'), false, 'no automatic actions');
      assert.ok(requests.filter(row => row.path.startsWith('/api/')).every(row => row.method === 'GET'),
        'background traffic is GET-only');
      const allowed = new Set(['/api/v1/events', '/api/v1/fleet', ...metadataPaths, '/api/v1/push/config']);
      assert.deepEqual(requests.filter(row => row.path.startsWith('/api/') && !allowed.has(row.path)), [],
        'no transcript, output, decision details or other content requests');
    } finally {
      const errors = [];
      try { socket?.close(); } catch (error) { errors.push(error); }
      for (const upstream of upstreams) upstream.destroy();
      for (const server of [proxy, reservation]) {
        try { await closeServer(server); } catch (error) { errors.push(error); }
      }
      let stopped = true;
      for (const child of [...children].reverse()) {
        if (!child.pid) continue;
        const signal = value => {
          try { process.kill(-child.pid, value); return true; }
          catch (error) { if (error.code === 'ESRCH') return false; throw error; }
        };
        const reaped = async ms => {
          const end = performance.now() + ms;
          do {
            if (!signal(0) && (child.exitCode !== null || child.signalCode !== null)) return true;
            await sleep(50);
          } while (performance.now() < end);
          return false;
        };
        try {
          signal(child.label === 'gateway' ? 'SIGINT' : 'SIGTERM');
          if (!await reaped(3000)) {
            // Gateway is only ever sent INT/TERM; never touch unrelated processes.
            signal(child.label === 'gateway' ? 'SIGTERM' : 'SIGKILL');
            if (!await reaped(3000)) throw new Error(`${child.label} owned group ${child.pid} not reaped`);
          }
        } catch (error) { stopped = false; errors.push(error); }
      }
      for (const timer of timers) cancel(timer);
      pending.clear();
      if (root && stopped) {
        try { await rm(root, {recursive: true, force: true}); } catch (error) { errors.push(error); }
      }
      if (errors.length) throw new AggregateError(errors,
        `Live-event cleanup failed: ${root}${stopped ? '' : ' retained until owned process groups are stopped'}`);
    }
  });
