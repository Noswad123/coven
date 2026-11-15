import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { EventEmitter } from "node:events";
import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";

const base = process.env.COVEN_NATIVE_ASSETS;
function harness(provider, generated = !!base) {
  const children = [], timers = new Set();
  let now = 0, uuid = 0, fail = false;
  const spawn = (exe, argv, options) => {
    if (fail) throw Error("inert spawn failure");
    const c = new EventEmitter();
    Object.assign(c, { exe, argv, options, kills: 0,
      kill() { this.kills++; },
      stdin: Object.assign(new EventEmitter(), { end(text) { c.payload = JSON.parse(text); } }),
    });
    children.push(c);
    return c;
  };
  const dir = fileURLToPath(new URL("../src/native_extensions/", import.meta.url));
  const source = generated
    ? readFileSync(`${base}/${provider}/coven-native-events.${provider === "pi" ? "ts" : "js"}`, "utf8")
    : `const EXECUTABLE = '/literal/exe'; const ROUTE = ['--socket', '/literal/socket'];
       const PROVIDER = '${provider}'; const PROFILE = '${provider === "pi" ? "pi-context.v1" : "opencode-root.v1"}';\n` +
      readFileSync(`${dir}emitter.js`, "utf8") + "\n" +
      (provider === "opencode" ? readFileSync(`${dir}session.js`, "utf8") : "") + "\n" +
      readFileSync(`${dir}${provider}.js`, "utf8");
  const sandbox = { Buffer, AbortController, performance: { now: () => now }, spawn,
    randomUUID: () => `00000000-0000-4000-8000-${String(++uuid).padStart(12, "0")}`,
    setTimeout(fn, delay) { const t = { fn, at: now + delay, unref() {} }; timers.add(t); return t; },
    clearTimeout(t) { timers.delete(t); },
  };
  vm.createContext(sandbox);
  vm.runInContext(source.replace(/^import .*;\n/gm, "").replace("export default function", "function")
    .replace("export async function", "async function") +
    `\nglobalThis.factory = ${provider === "pi" ? "CovenNativeEvents" : "CovenNativeEventsPlugin"};`, sandbox);
  return { children, factory: sandbox.factory, timers,
    advance(ms) { now += ms; for (const t of [...timers]) if (t.at <= now) { timers.delete(t); t.fn(); } },
    fail(value) { fail = value; }, close() { children.at(-1)?.emit("close"); },
  };
}
const dto = (id, extra = {}) => ({ id, slug: "slug", projectID: "project", directory: "/private",
  title: "PRIVATE TITLE", version: "pin", time: { created: 1, updated: 2 }, ...extra });
const event = (plugin, type, properties) => plugin.event({ event: { type, properties } });
const lifecycle = (p, type, id, extra) => event(p, type, { sessionID: id, info: dto(id, extra) });
const decision = (type, sessionID, id) => ({ sessionID,
  ...(type.endsWith(".asked") ? { id } : { requestID: id }),
  ...(type === "permission.asked" ? { permission: "read", patterns: [], metadata: {}, always: [] } : {}),
  ...(type === "permission.replied" ? { reply: "once" } : {}),
  ...(type === "question.asked" ? { questions: [{ question: "PRIVATE", header: "PRIVATE", options: [] }] } : {}),
  ...(type === "question.replied" ? { answers: [["PRIVATE ANSWER"]] } : {}),
});
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const ctx = (id = "private-session", extra = {}) => ({ mode: "tui", hasUI: true,
  sessionManager: { getSessionId: () => id }, isIdle: () => true, ...extra });
function pi(h) { const callbacks = {}; h.factory({ on(name, fn) { callbacks[name] = fn; } }); return callbacks; }
async function dispose(h, p) { const done = p.dispose(); h.close(); await done; }

test("Pi source-faithful contexts positively report metadata/busy/idle in tui and rpc, FIFO on close", async () => {
  for (const mode of ["tui", "rpc"]) {
    const h = harness("pi"), c = pi(h), context = ctx(`private-${mode}`, { mode });
    assert.deepEqual(Object.keys(context.sessionManager), ["getSessionId"]);
    c.session_start({ reason: "startup" }, context);
    c.agent_start({}, context);
    c.agent_settled({}, context);
    assert.equal(h.children.length, 1);
    for (let i = 0; i < 3; i++) {
      h.advance(10);
      assert.equal(h.children.length, i + 1);
      assert.equal(h.children[i].payload.sequence, i + 1);
      assert.equal(h.children[i].kills, 0);
      h.close();
    }
    assert.deepEqual(h.children.map(c => [c.payload.event, c.payload.status]), [
      ["session.updated", undefined], ["session.status", "busy"], ["session.status", "idle"],
    ]);
    await c.session_shutdown({ reason: "quit" });
    assert.equal(h.timers.size, 0);
  }
  for (const mode of ["print", "json"]) {
    const h = harness("pi"), c = pi(h), context = ctx("private", { mode });
    c.session_start({}, context); c.agent_start({}, context); c.agent_settled({}, context);
    await c.session_shutdown({ reason: "quit" });
    assert.equal(h.children.length, 0);
  }
});

test("actual generated Pi module default export uses only getSessionId, without source rewriting", { skip: !base }, async () => {
  const original = childProcess.spawn;
  const children = [];
  childProcess.spawn = (exe, argv, options) => {
    const child = new EventEmitter();
    Object.assign(child, { exe, argv, options, kills: 0, kill() { this.kills++; },
      stdin: Object.assign(new EventEmitter(), { end(text) { child.payload = JSON.parse(text); } }),
    });
    children.push(child);
    return child;
  };
  syncBuiltinESMExports();
  const callbacks = [];
  try {
    const source = readFileSync(`${base}/pi/coven-native-events.ts`);
    const module = await import(`data:text/javascript;base64,${source.toString("base64")}`);
    assert.equal(typeof module.default, "function");
    for (const mode of ["print", "json", "tui", "rpc"]) {
      const c = {};
      module.default({ on(name, fn) { c[name] = fn; } }); callbacks.push(c);
      const context = ctx(`private-${mode}`, { mode });
      const start = children.length;
      c.session_start({ reason: "startup" }, context); c.agent_start({}, context); c.agent_settled({}, context);
      if (["print", "json"].includes(mode)) assert.equal(children.length, start);
      else {
        assert.equal(children.length, start + 1);
        for (let i = 0; i < 3; i++) {
          assert.equal(children[start + i].payload.sequence, i + 1);
          children[start + i].emit("close");
        }
        assert.deepEqual(children.slice(start).map(c => c.payload.status ?? c.payload.event), ["session.updated", "busy", "idle"]);
      }
      await c.session_shutdown({ reason: "quit" });
    }
  } finally {
    const closed = callbacks.map(c => c.session_shutdown({ reason: "quit" }));
    for (const child of children) child.emit("close");
    await Promise.all(closed);
    childProcess.spawn = original;
    syncBuiltinESMExports();
  }
});

test("OpenCode interleaved roots, sticky children, missing SDK, closed events and privacy", async () => {
  const h = harness("opencode"), p = await h.factory();
  await event(p, "session.status", { sessionID: "ses_resumed", status: { type: "busy" } });
  await event(p, "session.created", { sessionID: "ses_bad", info: { id: "ses_bad" } });
  await event(p, "session.created", { sessionID: "ses_bad", info: dto("ses_other") });
  await lifecycle(p, "session.created", "ses_child", { parentID: "ses_root" });
  await lifecycle(p, "session.updated", "ses_child");
  assert.equal(h.children.length, 0);
  await lifecycle(p, "session.created", "ses_a"); h.close();
  await lifecycle(p, "session.updated", "ses_b"); h.close();
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "retry", attempt: 1, next: 1, message: "PRIVATE RETRY" } }); h.close();
  for (const type of ["permission.asked", "permission.replied", "question.asked", "question.replied", "question.rejected"]) {
    await event(p, type, { ...decision(type, "ses_b", type.startsWith("permission") ? "per_private" : "que_private"), prompt: "PRIVATE PROMPT" });
    h.close();
  }
  const before = h.children.length;
  for (const type of ["session.error", "session.idle", "permission.cancelled", "question.cancelled", "kitsune:blocked"]) {
    await event(p, type, { sessionID: "ses_a", requestID: "que_private", status: { type: "busy" } });
  }
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "error" } });
  await event(p, "session.status", { status: { type: "busy" } });
  assert.equal(h.children.length, before);
  await lifecycle(p, "session.deleted", "ses_a"); h.close();
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  assert.equal(h.children.length, before + 1);
  const text = JSON.stringify(h.children.map(c => c.payload));
  assert(!text.includes("PRIVATE"));
  h.children.forEach((c, i) => {
    assert.equal(c.payload.sequence, i + 1);
    assert.equal(c.payload.contract, "coven.provider-events.v1");
    assert.equal(c.payload.profile, "opencode-root.v1");
    assert.equal(c.options.shell, false);
    assert.deepEqual(Array.from(c.options.stdio), ["pipe", "ignore", "ignore"]);
    assert.equal(c.argv[1], "report-native");
    if (base) { assert.equal(c.exe, '/absolute/coven "${literal}'); assert.equal(c.argv[3], "/absolute/socket ' literal"); }
  });
  await dispose(h, p);
  await lifecycle(p, "session.created", "ses_late");
  assert.equal(h.timers.size, 0);
});

test("Pi context lifecycle, RPC, switches, settled predicate, unsupported/missing/throwing context", async () => {
  const h = harness("pi"), c = pi(h);
  assert.deepEqual(Object.keys(c).sort(), ["agent_settled", "agent_start", "session_shutdown", "session_start"]);
  for (const mode of ["print", "json", undefined]) c.session_start({}, ctx("private", { mode }));
  c.session_start({}, ctx("private", { hasUI: false }));
  c.session_start({}, {});
  assert.equal(h.children.length, 0);
  c.session_start({}, ctx()); h.close();
  assert.equal(h.children[0].payload.event, "session.updated");
  c.agent_settled({}, ctx("private-session", { isIdle: () => false }));
  c.agent_settled({}, ctx("private-session", { isIdle: undefined }));
  c.agent_settled({}, ctx("private-session", { isIdle() { throw Error(); } }));
  assert.equal(h.children.length, 1);
  c.agent_start({}, ctx());
  c.agent_settled({}, ctx());
  c.session_start({}, ctx("rpc-session", { mode: "rpc" }));
  assert.equal(h.children.at(-1).kills, 1);
  h.close();
  assert.equal(h.children.at(-1).payload.session_id, "rpc-session"); h.close();
  c.agent_start({}, ctx("rpc-session", { mode: "rpc" })); h.close();
  c.agent_settled({}, ctx("rpc-session", { mode: "rpc" })); h.close();
  assert.equal(h.children.at(-1).payload.status, "idle");
  await c.session_shutdown({ reason: "resume" });
  c.agent_start({}, ctx("rpc-session", { mode: "rpc" }));
  const n = h.children.length;
  c.session_start({}, ctx("next")); h.close();
  await c.session_shutdown({ reason: "quit" });
  c.session_start({}, ctx("late"));
  assert.equal(h.children.length, n + 1);
  assert(h.children.every(c => ["session.updated", "session.status"].includes(c.payload.event)));
  assert(h.children.every(c => c.payload.profile === "pi-context.v1"));
});

test("queue bounds, deadline includes observation age, only close frees slot, disposal reaps", async () => {
  const h = harness("opencode"), p = await h.factory();
  await lifecycle(p, "session.created", "ses_a");
  for (let i = 0; i < 40; i++) await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  h.children[0].emit("error", Error()); h.children[0].emit("exit");
  assert.equal(h.children.length, 1);
  h.advance(1900);
  assert.equal(h.children[0].kills, 1);
  assert.equal(h.children.length, 1);
  h.close();
  assert.equal(h.children.length, 1); // queued observations expired, no replay
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
  assert.equal(h.children.at(-1).payload.sequence, 34); // active + 32, overflow drops
  h.advance(1000);
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  h.advance(500); h.close();
  h.advance(1400);
  assert.equal(h.children.at(-1).kills, 1); // 1900 since observation, not spawn
  let resolved = false;
  const done = p.dispose().then(() => { resolved = true; });
  await Promise.resolve(); assert.equal(resolved, false);
  h.close(); await done;
  assert.equal(h.timers.size, 0);
});

test("synchronous spawn failures are never retried and instances are per factory", async () => {
  const h = harness("opencode"), p = await h.factory();
  h.fail(true); await lifecycle(p, "session.created", "ses_a");
  h.fail(false); await lifecycle(p, "session.updated", "ses_a"); h.close();
  assert.equal(h.children.length, 1); assert.equal(h.children[0].payload.sequence, 2);
  const q = await h.factory(); await lifecycle(q, "session.created", "ses_b"); h.close();
  assert.notEqual(h.children[0].payload.instance_id, h.children[1].payload.instance_id);
  await dispose(h, p); await dispose(h, q);
});

test("malformed full DTOs and identifier bounds fail closed without touching opaque metadata", async () => {
  const h = harness("opencode"), p = await h.factory();
  for (const invalid of [
    { time: { created: 0 } }, { time: { created: -1, updated: 0 } }, { title: null },
    { workspaceID: null }, { summary: {} }, { cost: NaN }, { tokens: {} },
    { model: {} }, { share: {} }, { metadata: [] }, { permission: [{}] },
    { permission: new Array(1) }, { revert: {} }, { path: null }, { agent: 7 },
  ]) await lifecycle(p, "session.created", "ses_bad", invalid);
  for (const id of ["ses\nprivate", "ses" + "x".repeat(510), "ses" + "é".repeat(255), "ses\ud800"]) {
    await lifecycle(p, "session.created", id);
  }
  assert.equal(h.children.length, 0);
  const metadata = Object.defineProperty({}, "raw", { get() { throw Error("must not inspect metadata"); } });
  await lifecycle(p, "session.created", "ses_valid", { metadata }); h.close();
  await event(p, "question.asked", { sessionID: "ses_valid", id: "que" + "x".repeat(510) });
  assert.equal(h.children.length, 1);
  assert(!JSON.stringify(h.children[0].payload).includes("metadata"));
  await dispose(h, p);
});

test("original callback age includes synchronous validation and root tracking is bounded", async () => {
  const h = harness("opencode"), p = await h.factory();
  const info = dto("ses_old");
  Object.defineProperty(info, "title", { get() { h.advance(1901); return "title"; } });
  await event(p, "session.created", { sessionID: "ses_old", info });
  assert.equal(h.children.length, 0);
  for (let i = 0; i < 511; i++) {
    await lifecycle(p, "session.created", `ses_${i}`, { parentID: "ses_parent" });
  }
  await lifecycle(p, "session.created", "ses_overflow");
  await event(p, "session.status", { sessionID: "ses_overflow", status: { type: "busy" } });
  assert.equal(h.children.length, 0);
  await dispose(h, p);
});

test("OpenCode on-demand legacy SDK lookup is silent, deduplicated and globally FIFO across roots", async () => {
  const h = harness("opencode"), a = deferred(), b = deferred(), calls = [];
  const p = await h.factory({ client: { session: { get(options) {
    calls.push(options);
    assert.deepEqual(Object.keys(options).sort(), ["path", "signal", "throwOnError"]);
    assert.deepEqual(Object.keys(options.path), ["id"]);
    assert.equal(options.throwOnError, true); assert.equal(options.signal.aborted, false);
    return options.path.id === "ses_a" ? a.promise : b.promise;
  } } } });
  assert.equal(calls.length, 0, "no initialization IO");
  await lifecycle(p, "session.created", "ses_known"); h.close();
  await event(p, "session.status", { sessionID: "ses_known", status: { type: "busy" } }); h.close();
  assert.equal(calls.length, 0, "known roots need no lookup");
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  await event(p, "permission.asked", decision("permission.asked", "ses_a", "per_one"));
  await event(p, "question.asked", decision("question.asked", "ses_b", "que_one"));
  await event(p, "permission.replied", decision("permission.replied", "ses_a", "per_one"));
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
  assert.equal(calls.length, 1); assert.equal(h.children.length, 2);
  a.resolve({ data: dto("ses_a") }); await flush();
  assert.equal(calls.length, 2); assert.equal(calls[1].path.id, "ses_b");
  b.resolve({ data: dto("ses_b") }); await flush();
  for (let i = 0; i < 5; i++) { h.advance(10); h.close(); }
  assert.deepEqual(h.children.slice(2).map(c => [c.payload.session_id, c.payload.event, c.payload.sequence]), [
    ["ses_a", "session.status", 3], ["ses_a", "permission.asked", 4], ["ses_b", "question.asked", 5],
    ["ses_a", "permission.replied", 6], ["ses_a", "session.status", 7],
  ]);
  assert(h.children.every(c => !JSON.stringify(c.payload).includes("PRIVATE")));
  await dispose(h, p); assert.equal(h.timers.size, 0);
});

test("resumed first permission/question ask and replies share one lookup and emit no synthetic lifecycle", async () => {
  for (const kind of ["permission", "question"]) {
    const h = harness("opencode"), response = deferred(); let calls = 0;
    const p = await h.factory({ client: { session: { get() { calls++; return response.promise; } } } });
    const types = [`${kind}.asked`, `${kind}.replied`, ...(kind === "question" ? ["question.rejected"] : [])];
    for (const type of types) await event(p, type, decision(type, "ses_resumed", `${kind === "permission" ? "per" : "que"}_private`));
    assert.equal(calls, 1); assert.equal(h.children.length, 0);
    response.resolve({ data: dto("ses_resumed") }); await flush();
    for (const _ of types) h.close();
    assert.deepEqual(h.children.map(c => c.payload.event), types);
    assert.deepEqual(h.children.map(c => c.payload.sequence), types.map((_, i) => i + 1));
    await dispose(h, p);
  }
});

test("malformed/unreportable native callbacks never trigger lookup or occupy pending capacity", async () => {
  const h = harness("opencode"), response = deferred(); let calls = 0;
  const p = await h.factory({ client: { session: { get() { calls++; return response.promise; } } } });
  for (let i = 0; i < 40; i++) {
    for (const [type, properties] of [
      ["session.status", { sessionID: "ses_bad", status: { type: "error" } }],
      ["session.status", { sessionID: "ses_bad", status: { type: "retry" } }],
      ["session.status", { status: { type: "busy" } }],
      ["permission.asked", { sessionID: "ses_bad", id: "per_one" }],
      ["permission.replied", { sessionID: "ses_bad", requestID: "per_one", reply: "cancel" }],
      ["question.asked", { sessionID: "ses_bad", id: "que_one", questions: [{}] }],
      ["question.replied", { sessionID: "ses_bad", requestID: "que_one", answers: [7] }],
      ["question.rejected", { sessionID: "ses_bad", requestID: "per_wrong_kind" }],
      ["permission.cancelled", { sessionID: "ses_bad", requestID: "per_one" }],
      ["question.cancelled", { sessionID: "ses_bad", requestID: "que_one" }],
      ["session.error", { sessionID: "ses_bad" }],
    ]) await event(p, type, properties);
  }
  assert.equal(calls, 0); assert.equal(h.timers.size, 0);
  await event(p, "session.status", { sessionID: "ses_valid", status: { type: "busy" } });
  response.resolve({ data: dto("ses_valid") }); await flush();
  assert.equal(calls, 1); assert.equal(h.children.length, 1);
  assert.equal(h.children[0].payload.sequence, 1);
  await dispose(h, p);
});

test("metadata must be a matching full root DTO; child, malformed, missing, thrown and rejected responses fail closed", async () => {
  for (const [name, get] of [
    ["child", () => ({ data: dto("ses_a", { parentID: "ses_parent" }) })],
    ["mismatch", () => ({ data: dto("ses_other") })],
    ["missing", () => ({})], ["partial", () => ({ data: { id: "ses_a" } })],
    ["invalid optional", () => ({ data: dto("ses_a", { tokens: {} }) })],
    ["throw", () => { throw Error("PRIVATE failure"); }],
    ["reject", () => Promise.reject(Error("PRIVATE failure"))],
  ]) {
    const h = harness("opencode"); let calls = 0;
    const p = await h.factory({ client: { session: { get(options) { calls++; return get(options); } } } });
    await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
    await flush();
    assert.equal(calls, 1, name); assert.equal(h.children.length, 0, name);
    if (name === "child") {
      await lifecycle(p, "session.updated", "ses_a");
      await event(p, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
      assert.equal(calls, 1); assert.equal(h.children.length, 0, "child classification is sticky");
    }
    await dispose(h, p); assert.equal(h.timers.size, 0);
  }
});

test("lookup timeout races an abort-ignoring SDK, drops its cohort and ignores late success", async () => {
  const h = harness("opencode"), response = deferred(), next = deferred(), calls = [];
  const p = await h.factory({ client: { session: { get(options) { calls.push(options); return calls.length === 1 ? response.promise : next.promise; } } } });
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  await event(p, "permission.asked", decision("permission.asked", "ses_a", "per_one"));
  h.advance(1900); await flush();
  assert.equal(calls[0].signal.aborted, true); assert.equal(h.timers.size, 0);
  response.resolve({ data: dto("ses_a") }); await flush();
  assert.equal(h.children.length, 0); assert.equal(calls.length, 1, "no retry or late registration");
  await event(p, "session.status", { sessionID: "ses_b", status: { type: "idle" } });
  assert.equal(calls.length, 2, "non-cooperative SDK cannot hold the work slot");
  next.resolve({ data: dto("ses_b") }); await flush();
  assert.equal(h.children.length, 1); assert.equal(h.children[0].payload.sequence, 1);
  await dispose(h, p);
});

test("deletion, child reclassification and disposal cancel lookups without reviving old session facts", async () => {
  for (const ending of ["deleted", "incomplete deletion", "child", "dispose"]) {
    const h = harness("opencode"), response = deferred(); let signal, calls = 0;
    const input = { client: { session: { get(options) { calls++; signal = options.signal; return response.promise; } } } };
    const p = await h.factory(input);
    await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
    await event(p, "question.asked", decision("question.asked", "ses_a", "que_one"));
    if (ending === "dispose") await p.dispose();
    else if (ending === "incomplete deletion") await event(p, "session.deleted", { sessionID: "ses_a" });
    else await lifecycle(p, ending === "deleted" ? "session.deleted" : "session.updated", "ses_a",
      ending === "child" ? { parentID: "ses_parent" } : {});
    await flush(); assert.equal(signal.aborted, true);
    response.resolve({ data: dto("ses_a") }); await flush();
    await lifecycle(p, "session.updated", "ses_a");
    await event(p, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
    assert.equal(calls, 1);
    assert.deepEqual(h.children.map(c => c.payload.event), ending === "deleted" ? ["session.deleted"] : []);
    h.close();
    // A replacement directory/plugin factory cannot inherit old classification.
    const replacement = await h.factory(input);
    await event(replacement, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
    await flush(); assert.equal(calls, 2);
    h.close(); await dispose(h, p); await dispose(h, replacement);
    assert.equal(h.timers.size, 0);
  }
});

test("full lifecycle root metadata supersedes lookup without reordering events or accepting late child metadata", async () => {
  const h = harness("opencode"), response = deferred(); let calls = 0, signal;
  const p = await h.factory({ client: { session: { get(options) { calls++; signal = options.signal; return response.promise; } } } });
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  await lifecycle(p, "session.updated", "ses_a");
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
  await flush(); assert.equal(signal.aborted, true);
  response.resolve({ data: dto("ses_a", { parentID: "ses_child" }) }); await flush();
  for (let i = 0; i < 3; i++) h.close();
  assert.deepEqual(h.children.map(c => [c.payload.event, c.payload.status]), [
    ["session.status", "busy"], ["session.updated", undefined], ["session.status", "idle"],
  ]);
  assert.equal(calls, 1);
  await dispose(h, p);
});

test("lookup queue is one active plus 32 pending and original age includes lookup plus child queue", async () => {
  const h = harness("opencode"), response = deferred(); let calls = 0;
  const p = await h.factory({ client: { session: { get() { calls++; return response.promise; } } } });
  await event(p, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  for (let i = 0; i < 40; i++) await event(p, "permission.asked", decision("permission.asked", "ses_a", `per_${i}`));
  h.advance(1500); response.resolve({ data: dto("ses_a") }); await flush();
  assert.equal(calls, 1); assert.equal(h.children.length, 1);
  for (let i = 0; i < 33; i++) h.close();
  assert.equal(h.children.length, 33, "overflow never enters emitter or allocates sequence");
  assert.equal(h.children.at(-1).payload.request_id, "per_31");
  await dispose(h, p);

  const g = harness("opencode"), metadata = deferred();
  const q = await g.factory({ client: { session: { get() { return metadata.promise; } } } });
  await event(q, "session.status", { sessionID: "ses_a", status: { type: "busy" } });
  await event(q, "session.status", { sessionID: "ses_a", status: { type: "idle" } });
  g.advance(1500); metadata.resolve({ data: dto("ses_a") }); await flush();
  g.advance(300); g.close();
  assert.equal(g.children.length, 2);
  g.advance(100);
  assert.equal(g.children[1].kills, 1, "queued child gets only the remaining original observation budget");
  await dispose(g, q);
});

test("full metadata DTO qualification retains the 512-ID bound and cannot refresh capture age", async () => {
  const h = harness("opencode"); let calls = 0;
  const p = await h.factory({ client: { session: { async get({ path: { id } }) {
    calls++;
    return { data: dto(id, { parentID: "ses_parent" }) };
  } } } });
  for (let i = 0; i < 512; i++) {
    await event(p, "session.status", { sessionID: `ses_${i}`, status: { type: "busy" } });
    await flush();
  }
  await event(p, "session.status", { sessionID: "ses_overflow", status: { type: "busy" } });
  assert.equal(calls, 512); assert.equal(h.children.length, 0);
  await dispose(h, p);

  const g = harness("opencode"); let lookups = 0;
  const q = await g.factory({ client: { session: { async get({ path: { id } }) {
    lookups++;
    const info = dto(id);
    Object.defineProperty(info, "title", { get() { g.advance(1900); return "PRIVATE"; } });
    return { data: info };
  } } } });
  for (let i = 0; i < 2; i++) {
    await event(q, "session.status", { sessionID: "ses_slow", status: { type: "busy" } });
    await flush();
    assert.equal(g.children.length, 0);
  }
  assert.equal(lookups, 2, "expired validation must not silently register a root");
  await dispose(g, q);
});
