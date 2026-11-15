// Real generated modules, reporter CLI and normal receiver. Only native callbacks
// and the Kitsune integration-view endpoint are fixtures; no provider is launched.
import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { createServer } from "node:net";
import { chmod } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const [root, executable] = process.argv.slice(2);
const socket = `${root}/r.sock`, kitSocket = `${root}/k.sock`;
const exec = promisify(execFile);
const frames = [], peers = new Set(), factories = [];
let receiver, receiverClosed, cleanupPromise, kitFailure, revision = 0;
const kit = createServer(peer => {
  peers.add(peer);
  peer.on("close", () => peers.delete(peer));
  peer.on("error", () => {});
  let input = "";
  peer.on("data", chunk => {
    try {
      input += chunk;
      if (input.length > 128 * 1024) { peer.destroy(); return; }
      if (!input.includes("\n")) return;
      const request = JSON.parse(input.slice(0, input.indexOf("\n")));
      const p = request.params;
      let result;
      switch (request.method) {
        case "session.snapshot":
          result = { type: "session_snapshot", snapshot: { version: "fixture", protocol: 18,
            integration_views: { version: 1, sources: [{ id: "nativeFixture", epoch: null }] } } };
          break;
        case "integration.view.open":
          result = { type: "view_opened", lease: "fixture-lease", epoch: "fixture-epoch", revision: 0, ttl_ms: 5000 };
          break;
        case "integration.view.replace":
          assert.equal(p.revision, ++revision);
          assert(p.items.length <= 128);
          assert(frames.length < 512, "bounded fixture frames");
          frames.push(p.items);
          result = { type: "view_replaced", epoch: "fixture-epoch", revision };
          break;
        case "integration.view.poll":
          result = { type: "view_intents", epoch: "fixture-epoch", revision, ttl_ms: 5000, intents: [] };
          break;
        case "integration.view.close": result = { type: "view_closed" }; break;
        default: throw Error(`unexpected Kit method: ${request.method}`);
      }
      if (request.method !== "session.snapshot") assert.equal(p.source, "nativeFixture");
      peer.end(JSON.stringify({ id: request.id, result }) + "\n");
    } catch (error) {
      kitFailure = error;
      peer.destroy();
    }
  });
});

async function until(check, message) {
  const deadline = performance.now() + 5000;
  while (performance.now() < deadline) {
    if (kitFailure) throw kitFailure;
    const result = await check();
    if (result) return result;
    await delay(10);
  }
  throw Error(message);
}

async function observations() {
  const { stdout, stderr } = await exec(executable, ["agent", "observations", "--socket", socket, "--json"],
    { timeout: 2000, maxBuffer: 128 * 1024 });
  assert.equal(stderr, "");
  assert(!stdout.includes("PRIVATE"), "projection must redact session/request IDs and callback data");
  return JSON.parse(stdout).observations;
}

async function state(id, expected, pending = 0) {
  const row = await until(async () => {
    const row = (await observations()).find(r => r.observation_id === id);
    return row?.reported_state === expected && row.fresh_pending_count === pending ? row : undefined;
  }, `missing ${expected} observation with ${pending} pending`);
  assert.equal(row.origin, "provider_native_event");
  return row;
}

async function presented(id, provider, status) {
  await until(() => frames.some(items => items.some(item => {
    const text = JSON.stringify(item);
    return item.id === `observation:${id}` && text.includes(`Coven / ${provider} /`) && text.includes(status);
  })), `missing redacted ${provider} ${status} Kit projection`);
  assert(!JSON.stringify(frames).includes("PRIVATE"));
}

function cleanup() {
  return cleanupPromise ??= (async () => {
    try {
      await Promise.all(factories.map(dispose => dispose()));
    } finally {
      try {
        if (receiver) {
          receiver.stdin.end();
          const timer = setTimeout(() => receiver.kill("SIGKILL"), 3000);
          try { assert.equal(await receiverClosed, 0, "normal receiver must join cleanly on EOF"); }
          finally { clearTimeout(timer); }
        }
      } finally {
        for (const peer of peers) peer.destroy();
        if (kit.listening) await new Promise(resolve => kit.close(resolve));
      }
    }
  })();
}
process.once("SIGTERM", () => { void cleanup().finally(() => process.exit(143)); });
process.once("SIGINT", () => { void cleanup().finally(() => process.exit(130)); });

try {
  await new Promise((resolve, reject) => { kit.once("error", reject); kit.listen(kitSocket, resolve); });
  await chmod(kitSocket, 0o600);
  receiver = spawn(executable, ["agent", "run", root, "--socket", socket,
    "--kitsune-socket", kitSocket, "--source", "nativeFixture", "--json"],
    { shell: false, stdio: ["pipe", "ignore", "inherit"] });
  receiver.stdin.on("error", () => {});
  receiverClosed = new Promise((resolve, reject) => { receiver.once("error", reject); receiver.once("close", resolve); });
  await until(async () => { try { return (await observations()).length === 0; } catch { return false; } }, "normal receiver not ready");

  // The pure-JS Pi asset is copied byte-for-byte from .ts to .mjs by the Rust
  // fixture, so Node imports the real default export and all actual node imports.
  const { default: pi } = await import(pathToFileURL(`${root}/pi.mjs`));
  for (const mode of ["print", "json", "tui", "rpc"]) {
    const callbacks = {};
    pi({ on(name, callback) { callbacks[name] = callback; } });
    factories.push(() => callbacks.session_shutdown({ reason: "quit" }));
    const context = { hasUI: true, mode, sessionManager: { getSessionId() { return `PRIVATE-${mode}`; } }, isIdle() { return true; } };
    assert.deepEqual(Object.keys(context.sessionManager), ["getSessionId"]);
    const before = (await observations()).map(row => row.observation_id);
    callbacks.session_start({ reason: "startup", prompt: "PRIVATE-PROMPT" }, context);
    if (["print", "json"].includes(mode)) {
      callbacks.agent_start({}, context); callbacks.agent_settled({}, context);
      await callbacks.session_shutdown({ reason: "quit" });
      assert.deepEqual((await observations()).map(row => row.observation_id), before);
      continue;
    }
    const row = await until(async () => (await observations()).find(row => !before.includes(row.observation_id)), "Pi metadata missing");
    assert.equal(row.reported_state, "unknown");
    assert.equal(row.profile, "pi-context.v1");
    assert.equal(row.root_eligibility, "ui_rpc_context");
    callbacks.agent_start({}, context);
    await state(row.observation_id, "working"); await presented(row.observation_id, "pi", "WORKING");
    callbacks.agent_settled({}, context);
    await state(row.observation_id, "idle"); await presented(row.observation_id, "pi", "IDLE");
    await callbacks.session_shutdown({ reason: "quit" });
  }

  const { CovenNativeEventsPlugin } = await import(pathToFileURL(`${root}/opencode.mjs`));
  const sessionID = "ses_PRIVATE-root";
  let metadataCalls = 0;
  const plugin = await CovenNativeEventsPlugin({ client: { session: { async get(options) {
    metadataCalls++;
    assert.deepEqual(Object.keys(options).sort(), ["path", "signal", "throwOnError"]);
    assert.deepEqual(options.path, { id: sessionID });
    assert.equal(options.throwOnError, true);
    assert.equal(options.signal.aborted, false);
    return { data: { id: sessionID, slug: "slug", projectID: "project", directory: "/PRIVATE-directory",
      title: "PRIVATE-title", version: "pinned", time: { created: 1, updated: 2 } } };
  } } } });
  assert.equal(metadataCalls, 0, "no SDK IO during module initialization");
  factories.push(() => plugin.dispose());
  const event = (type, properties) => plugin.event({ event: { type, properties } });
  await event("session.status", { sessionID, status: { type: "busy" } });
  const row = await until(async () => (await observations()).find(r => r.provider === "opencode"), "resumed OpenCode status missing");
  assert.equal(row.reported_state, "working"); assert.equal(row.root_eligibility, "producer_checked");
  assert.equal(metadataCalls, 1);
  await state(row.observation_id, "working"); await presented(row.observation_id, "opencode", "WORKING");
  for (const id of ["per_PRIVATE-one", "per_PRIVATE-two"]) {
    await event("permission.asked", { sessionID, id, permission: "read", patterns: [], metadata: {}, always: [] });
  }
  await state(row.observation_id, "blocked", 2); await presented(row.observation_id, "opencode", "BLOCKED");
  await event("permission.replied", { sessionID, requestID: "per_PRIVATE-other", reply: "once" });
  await event("session.status", { sessionID, status: { type: "idle" } });
  await state(row.observation_id, "blocked", 2);
  await event("permission.replied", { sessionID, requestID: "per_PRIVATE-one", reply: "once" });
  await state(row.observation_id, "blocked", 1);
  await event("permission.replied", { sessionID, requestID: "per_PRIVATE-two", reply: "reject" });
  await state(row.observation_id, "idle"); await presented(row.observation_id, "opencode", "IDLE");
  assert.equal(metadataCalls, 1, "qualified root never needs another metadata lookup");
  assert.equal((await observations()).length, 3);
  assert.ifError(kitFailure);
  console.log("Real generated modules -> report-native CLI -> normal receiver -> redacted observations/Kit projection passed (Pi tui/rpc; resumed OpenCode status-first, silent SDK metadata, matched decisions).");
} finally {
  await cleanup();
}
