// Run after cargo build: TMPDIR="$PWD/target/tmp" node tests/pi_extension_asset.test.mjs [COVEN_BINARY]
// Bun: TMPDIR="$PWD/target/tmp" bun test tests/pi_extension_asset.test.mjs
// No native Pi, live apps, receiver, or user settings.
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { EventEmitter } from "node:events";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { test } from "node:test";

const binary = resolve(process.argv[2] ?? process.env.COVEN_TEST_BINARY ?? "target/debug/coven");
const tempRoot = resolve("target/tmp");
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const bunMock = process.versions.bun ? (await import("bun:test")).mock : undefined;

function setSpawn(spawn) {
  childProcess.spawn = spawn;
  // Bun's syncBuiltinESMExports is a no-op; mock the built-in module there.
  if (bunMock) bunMock.module("node:child_process", () => ({ ...childProcess, default: childProcess, spawn }));
  else syncBuiltinESMExports();
}

test("actual generated Pi asset: reference-only and bounded child ownership", async (t) => {
  await mkdir(tempRoot, { recursive: true });
  const work = await mkdtemp(join(tempRoot, "pi-extension-asset-"));
  const originalSpawn = childProcess.spawn;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let sequence = 0;
  const socket = join(work, 'socket "quotes" \\ ${EXECUTABLE} __SOCKET__.sock');
  const executable = join(work, 'helper "quotes" \\ ${SOCKET} __EXECUTABLE__');
  const children = [];
  let spawnFailure;
  let stdinFailure;
  let timers = [];

  async function install(exe = executable) {
    const root = join(work, `extensions-${sequence++}`);
    await mkdir(root);
    const result = childProcess.spawnSync(binary, [
      "agent", "extensions", "install", "--provider", "pi",
      "--extension-root", root, "--executable", exe, "--socket", socket, "--apply",
    ], { encoding: "utf8", timeout: 10_000 });
    assert.equal(result.status, 0, `install failed: ${result.stderr}\n${result.stdout}`);
    const files = (await readdir(root, { recursive: true })).filter((name) => name.endsWith(".ts"));
    assert.equal(files.length, 1, "one generated TypeScript asset");
    const file = join(root, files[0]);
    assert.ok((await readFile(file)).length <= 1024 * 1024);
    return (await import(pathToFileURL(file).href)).default;
  }

  function factory(extension) {
    const callbacks = new Map();
    extension({ on(name, callback) {
      assert.ok(!callbacks.has(name));
      callbacks.set(name, callback);
    } });
    assert.deepEqual([...callbacks.keys()], ["session_start", "session_shutdown"]);
    return {
      start(id, hasUI = true, reason = "startup") {
        const event = new Proxy({ reason }, { get() { throw Error("event leakage"); } });
        const sessionManager = new Proxy({ getSessionId() { return id; } }, {
          get(target, key) {
            assert.equal(key, "getSessionId", "no session file or other manager lookup");
            return target[key];
          },
        });
        const ctx = new Proxy({ hasUI, sessionManager }, {
          get(target, key) {
            assert.ok(key === "hasUI" || key === "sessionManager", "no prompt/cwd/context lookup");
            return target[key];
          },
        });
        assert.equal(callbacks.get("session_start")(event, ctx), undefined, "never await child exit");
      },
      callbacks,
      shutdown() { callbacks.get("session_shutdown")(); },
    };
  }

  function close(child = children.at(-1)) { child.emit("close", 0, null); }
  function expectReport(child, id) {
    assert.equal(child.executable, executable);
    assert.deepEqual(child.argv, ["agent", "report-extension", "--provider", "pi", "--socket", socket, "--event", "session_start"]);
    assert.deepEqual(child.options, { shell: false, stdio: ["pipe", "ignore", "ignore"] });
    assert.equal(child.input, JSON.stringify({ contract: "coven.pi-extension.v1", event: "session_start", session_id: id }));
  }

  try {
    setSpawn((executable, argv, options) => {
      if (spawnFailure) throw spawnFailure;
      const child = new EventEmitter();
      Object.assign(child, { executable, argv, options, kills: [], input: undefined });
      child.stdin = new EventEmitter();
      child.stdin.end = (input) => {
        if (stdinFailure) throw stdinFailure;
        child.input = input;
      };
      child.kill = (signal) => { child.kills.push(signal); return true; };
      children.push(child);
      return child;
    });
    globalThis.setTimeout = (callback, delay) => {
      const timer = { callback, delay, unref() {}, cleared: false };
      timers.push(timer);
      return timer;
    };
    globalThis.clearTimeout = (timer) => { if (timer) timer.cleared = true; };
    const extension = await install();

    await t.test("strict UI, snapshot privacy, native start reasons", () => {
      const f = factory(extension);
      const before = children.length;
      for (const ui of [false, undefined, null, 1, "true"]) f.start("ignored", ui === undefined ? null : ui);
      assert.equal(children.length, before);
      for (const reason of ["startup", "reload", "new", "resume", "fork"]) {
        f.start(`sid-${reason}`, true, reason);
        expectReport(children.at(-1), `sid-${reason}`);
        close();
      }
      // RPC hasUI=true is eligible too; no root/TUI proof is inferred.
      f.start("rpc-reference");
      expectReport(children.at(-1), "rpc-reference");
      close();
      const callback = f.callbacks.get("session_start");
      callback({}, { get hasUI() { throw Error("private"); } });
      callback({}, { hasUI: true, get sessionManager() { throw Error("private"); } });
      callback({}, { hasUI: true, sessionManager: { getSessionId() { throw Error("private"); } } });
      let reads = 0;
      callback({}, { hasUI: true, sessionManager: { getSessionId() { reads++; return "snapshot"; } } });
      assert.equal(reads, 1);
      expectReport(children.at(-1), "snapshot");
      close();
      f.shutdown();
    });

    await t.test("Rust-compatible session ID validation and exact UTF-8 boundary", () => {
      const f = factory(extension);
      const before = children.length;
      for (const id of [null, undefined, 1, {}, "", " ", "\u0085", "\u2003", "-x", "\u2003-x", "\u0085-x", "a\n", "a\u007f", "a\u009f", "x".repeat(513), "\u00e9".repeat(257), "\ud800"]) f.start(id);
      assert.equal(children.length, before);
      for (const id of ["x", " x ", "\u00e9".repeat(256), "\u{1f600}".repeat(128), "\ufeff-x", "x\u200b"]) {
        f.start(id);
        expectReport(children.at(-1), id);
        close();
      }
      f.shutdown();
    });

    await t.test("overlap skipped, timeout retains slot until close, stale close cannot clear new child", () => {
      const f = factory(extension);
      f.start("first");
      const first = children.at(-1);
      const timer = timers.at(-1);
      assert.equal(timer.delay, 2000);
      f.start("overlap");
      assert.equal(children.at(-1), first);
      timer.callback();
      assert.deepEqual(first.kills, ["SIGKILL"]);
      f.start("still-overlap");
      assert.equal(children.at(-1), first);
      close(first);
      assert.ok(timer.cleared);
      f.start("next");
      const next = children.at(-1);
      close(first);
      f.start("stale-close-overlap");
      assert.equal(children.at(-1), next);
      f.shutdown();
      assert.deepEqual(next.kills, ["SIGKILL"]);
      close(next);
      const count = children.length;
      f.start("after-shutdown");
      assert.equal(children.length, count);
    });

    await t.test("silent synchronous spawn failure, asynchronous errors and EPIPE", () => {
      const f = factory(extension);
      spawnFailure = Error("spawn failed");
      f.start("failed");
      spawnFailure = undefined;
      f.start("async-failed");
      const failed = children.at(-1);
      failed.emit("error", Error("ENOENT"));
      failed.stdin.emit("error", Error("EPIPE"));
      f.start("skip-before-close");
      assert.equal(children.at(-1), failed);
      close(failed);
      stdinFailure = Error("synchronous EPIPE");
      f.start("sync-stdin-error");
      stdinFailure = undefined;
      const broken = children.at(-1);
      assert.deepEqual(broken.kills, ["SIGKILL"]);
      f.start("skip-broken-before-close");
      assert.equal(children.at(-1), broken);
      close(broken);
      f.start("recovered");
      expectReport(children.at(-1), "recovered");
      close();
      f.shutdown();
    });

    await t.test("four factories retain independent session and child ownership", () => {
      const factories = Array.from({ length: 4 }, () => factory(extension));
      const owned = factories.map((f, i) => { f.start(`independent-${i}`); return children.at(-1); });
      factories[0].shutdown();
      assert.deepEqual(owned.map((child) => child.kills.length), [1, 0, 0, 0]);
      factories.forEach((f, i) => {
        expectReport(owned[i], `independent-${i}`);
        close(owned[i]);
        f.shutdown();
      });
    });

    setSpawn(originalSpawn);
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;

    await t.test("real explicit quoted executable, offline exit and actual two-second timeout", async () => {
      const record = join(work, "record.json");
      const helper = join(work, "report-helper.cjs");
      await writeFile(helper, `const fs = require('node:fs');\nlet input = '';\nprocess.stdin.on('data', chunk => input += chunk);\nprocess.stdin.on('end', () => { fs.writeFileSync(${JSON.stringify(record)}, JSON.stringify({argv: process.argv.slice(2), input, pid: process.pid})); });\nsetInterval(() => {}, 1000);\n`);
      // Bun cannot resolve a JS entrypoint with a literal backslash in its name.
      // Keep that executable-path coverage while exec preserves the owned PID.
      const shellQuote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
      await writeFile(executable, `#!/bin/sh\nexec ${shellQuote(process.execPath)} ${shellQuote(helper)} "$@"\n`);
      await chmod(executable, 0o700);
      const realExtension = await install();
      const f = factory(realExtension);
      f.start("real-reference");
      let captured;
      for (let i = 0; i < 100; i++) {
        try { captured = JSON.parse(await readFile(record, "utf8")); break; } catch {}
        await pause(10);
      }
      assert.ok(captured, "controlled helper received stdin");
      assert.deepEqual(captured.argv, ["agent", "report-extension", "--provider", "pi", "--socket", socket, "--event", "session_start"]);
      assert.equal(captured.input, JSON.stringify({ contract: "coven.pi-extension.v1", event: "session_start", session_id: "real-reference" }));
      await pause(2200);
      assert.throws(() => process.kill(captured.pid, 0), { code: "ESRCH" });
      f.shutdown();
      const offline = factory(await install(join(work, "does-not-exist")));
      offline.start("offline");
      await pause(50);
      offline.start("offline-again");
      await pause(50);
      offline.shutdown();
    });
  } finally {
    setSpawn(originalSpawn);
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
    await rm(work, { recursive: true, force: true });
  }
});
