// cargo build, then node tests/opencode_extension_asset.test.mjs
// Also supports bun test tests/opencode_extension_asset.test.mjs.
// Uses only generated CLI installs, mocks, and controlled offline helpers.
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { EventEmitter } from "node:events";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { test } from "node:test";

const binary = resolve(process.env.COVEN_TEST_BINARY ?? "target/debug/coven");
const tempRoot = resolve("target/tmp");
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const bunMock = process.versions.bun ? (await import("bun:test")).mock : undefined;

function setSpawn(spawn) {
  childProcess.spawn = spawn;
  if (bunMock) bunMock.module("node:child_process", () => ({ ...childProcess, default: childProcess, spawn }));
  else syncBuiltinESMExports();
}

// SessionInfo and its nested schemas at the pinned v1.19.5 commit, not a
// partial SDK update or the changed dev API. Metadata contents stay opaque.
function info(id = "ses_root") {
  return {
    id, slug: "root-slug", projectID: "project", directory: "/private/directory",
    title: "private title", version: "1.19.5", time: { created: 0, updated: 1 },
  };
}
function full(id = "ses_full") {
  return {
    ...info(id), workspaceID: "wrk_test", path: "/private/path", parentID: undefined,
    summary: { additions: 1, deletions: 2, files: 1, diffs: [
      { file: "private file", patch: "private patch", additions: 1, deletions: 2, status: "modified" },
    ] },
    cost: 1.5, tokens: { input: 1, output: 2, reasoning: 3, cache: { read: 4, write: 5 } },
    share: { url: "private URL" }, agent: "private agent",
    model: { id: "model", providerID: "provider", variant: "variant" },
    metadata: { secret: "never inspect" }, time: { created: 0, updated: 1, compacting: 2, archived: 3.5 },
    permission: [{ permission: "read", pattern: "private pattern", action: "ask" }],
    revert: { messageID: "msg_test", partID: "prt_test", snapshot: "private snapshot", diff: "private diff" },
  };
}
function event(dto, type = "session.created", sessionID = dto.id) {
  return { event: { type, properties: { info: dto, sessionID } } };
}

test("actual generated OpenCode plugin: pinned full DTO, bounded FIFO and child ownership", async (t) => {
  await mkdir(tempRoot, { recursive: true });
  const work = await mkdtemp(join(tempRoot, "opencode-extension-asset-"));
  const executable = join(work, 'helper space \' "quotes" \\ ${SOCKET} __EXECUTABLE__');
  const socket = join(work, 'socket space \' "quotes" \\ ${EXECUTABLE} __SOCKET__.sock');
  const originalSpawn = childProcess.spawn;
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const children = [];
  const timers = [];
  let sequence = 0;
  let spawnFailure = false;
  let stdinFailure = false;
  let spawnAttempts = 0;

  async function install(exe = executable) {
    const root = join(work, `extensions-${sequence++}`);
    await mkdir(root);
    const result = childProcess.spawnSync(binary, [
      "agent", "extensions", "install", "--provider", "opencode",
      "--extension-root", root, "--executable", exe, "--socket", socket, "--apply",
    ], { encoding: "utf8", timeout: 10_000 });
    assert.equal(result.status, 0, `install failed: ${result.stderr}\n${result.stdout}`);
    const files = (await readdir(root, { recursive: true })).filter((name) => name.endsWith(".js"));
    assert.equal(files.length, 1, "one generated JavaScript plugin");
    const file = join(root, files[0]);
    const source = await readFile(file);
    assert.ok(source.length <= 1024 * 1024);
    const module = await import(pathToFileURL(file).href);
    assert.deepEqual(Object.keys(module), ["CovenAgentReferencePlugin"], "no aliases or helper exports");
    assert.equal(module.CovenAgentReferencePlugin.constructor.name, "AsyncFunction");
    return { plugin: module.CovenAgentReferencePlugin, source };
  }

  async function factory(plugin) {
    const hooks = await plugin(new Proxy({}, { get() { throw Error("no context/SDK reads"); } }));
    assert.deepEqual(Object.keys(hooks), ["event", "dispose"]);
    return hooks;
  }
  function close(child = children.at(-1)) { child.emit("close", 0, null); }
  function expectReport(child, id, type = "session.created") {
    assert.equal(child.executable, executable);
    assert.deepEqual(child.argv, ["agent", "report-extension", "--provider", "opencode", "--socket", socket, "--event", type]);
    assert.deepEqual(child.options, { shell: false, stdio: ["pipe", "ignore", "ignore"] });
    assert.equal(child.input, JSON.stringify({ contract: "coven.opencode-extension.v1", event: type, session_id: id }));
  }

  try {
    setSpawn((executable, argv, options) => {
      spawnAttempts++;
      if (spawnFailure) throw Error("spawn failure");
      const child = new EventEmitter();
      Object.assign(child, { executable, argv, options, kills: [], input: undefined });
      child.stdin = new EventEmitter();
      child.stdin.end = (input) => {
        if (stdinFailure) throw Error("synchronous EPIPE");
        child.input = input;
      };
      child.kill = (signal) => { child.kills.push(signal); return true; };
      children.push(child);
      return child;
    });
    globalThis.setTimeout = (callback, delay) => {
      const timer = { callback, delay, cleared: false, unref() {} };
      timers.push(timer);
      return timer;
    };
    globalThis.clearTimeout = (timer) => { if (timer) timer.cleared = true; };
    const { plugin, source } = await install();
    assert.deepEqual((await install()).source, source, "deterministic generated source");

    await t.test("two interleaved roots, native children, no sticky root or partial fallback", async () => {
      const f = await factory(plugin);
      for (const [id, type] of [
        ["ses_a", "session.created"], ["ses_b", "session.updated"],
        ["ses_a", "session.updated"], ["ses_b", "session.created"],
      ]) {
        await f.event(event(full(id), type));
        expectReport(children.at(-1), id, type);
        close();
      }
      const before = children.length;
      for (const parentID of [null, "", "ses_parent", 1, false]) {
        await f.event(event({ ...full("ses_a"), parentID }));
      }
      for (const dto of [{ id: "ses_a" }, { id: "ses_other", parentID: undefined }, null, {}]) {
        await f.event({ event: { type: "session.updated", properties: { sessionID: "ses_a", info: dto } } });
      }
      for (const type of ["session.deleted", "session.idle", "session.status", "chat.message", "tool.execute.after", "unknown"]) {
        await f.event(event(full(), type));
      }
      for (const sessionID of [null, undefined, "ses_conflict", 1]) {
        const input = event(full());
        input.event.properties.sessionID = sessionID;
        await f.event(input);
      }
      await f.event({ event: { type: "session.created", properties: { sessionID: "ses_top_only" } } });
      for (const malformed of [undefined, null, {}, { event: null }]) await f.event(malformed);
      assert.equal(children.length, before);
      await f.event(event(info("ses_a")));
      expectReport(children.at(-1), "ses_a");
      close();
      await f.dispose();
    });

    await t.test("all required and known optional schema fields fail closed", async () => {
      const f = await factory(plugin);
      const before = children.length;
      for (const key of ["id", "slug", "projectID", "directory", "title", "version", "time"]) {
        const dto = info();
        delete dto[key];
        await f.event(event(dto));
        await f.event(event({ ...info(), [key]: null }));
      }
      const bad = [
        { id: "not-session" }, { workspaceID: "bad" }, { workspaceID: null }, { path: 1 },
        { summary: {} }, { summary: { additions: 1, deletions: 2, files: NaN } },
        { summary: { additions: 1, deletions: 2, files: 1, diffs: [{ additions: 1, deletions: 1, status: "bad" }] } },
        { cost: Infinity }, { tokens: { input: 1, output: 2, reasoning: 3, cache: { read: "4", write: 5 } } },
        { share: { url: 1 } }, { agent: false }, { model: { id: "x", providerID: 1 } },
        { model: { id: "x", providerID: "p", variant: null } }, { metadata: [] }, { metadata: null },
        { time: { created: -1, updated: 1 } }, { time: { created: 0.5, updated: 1 } },
        { time: { created: 0, updated: 1, compacting: 0.5 } },
        { time: { created: 0, updated: 1, archived: Infinity } },
        { permission: {} }, { permission: [{ permission: "x", pattern: "y", action: "bad" }] },
        { revert: { messageID: "bad" } }, { revert: { messageID: "msg_x", partID: "bad" } },
        { revert: { messageID: "msg_x", snapshot: 1 } },
      ];
      for (const fields of bad) await f.event(event({ ...full(), ...fields }));
      for (const path of [
        "slug", "projectID", "directory", "title", "version", "workspaceID", "path", "summary", "cost", "tokens",
        "share", "agent", "model", "metadata", "permission", "revert", "time.created", "time.updated",
        "time.compacting", "time.archived", "summary.additions", "summary.deletions", "summary.files",
        "summary.diffs", "summary.diffs.0.file", "summary.diffs.0.patch", "summary.diffs.0.additions",
        "summary.diffs.0.deletions", "summary.diffs.0.status", "tokens.input", "tokens.output", "tokens.reasoning",
        "tokens.cache", "tokens.cache.read", "tokens.cache.write", "share.url", "model.id", "model.providerID",
        "model.variant", "permission.0.permission", "permission.0.pattern", "permission.0.action",
        "revert.messageID", "revert.partID", "revert.snapshot", "revert.diff",
      ]) {
        const dto = full();
        const parts = path.split(".");
        const key = parts.pop();
        let target = dto;
        for (const part of parts) target = target[part];
        target[key] = null;
        await f.event(event(dto));
      }
      await f.event(event({ ...full(), permission: Array(1) }));
      await f.event(event({ ...full(), summary: { additions: 0, deletions: 0, files: 0, diffs: Array(1) } }));
      assert.equal(children.length, before);
      // Negative finite archived/cost/summary numbers are permitted by the pin.
      await f.event(event({ ...full(), cost: -1, time: { created: 0, updated: 0, archived: -1 } }));
      expectReport(children.at(-1), "ses_full");
      close();
      await f.dispose();
    });

    await t.test("safe integer timestamp boundaries; archived remains finite", async () => {
      const f = await factory(plugin);
      for (const key of ["created", "updated", "compacting"]) {
        const dto = full(`ses_safe_${key}`);
        dto.time[key] = Number.MAX_SAFE_INTEGER;
        await f.event(event(dto));
        expectReport(children.at(-1), dto.id);
        close();
        const count = children.length;
        dto.time[key] = Number.MAX_SAFE_INTEGER + 1;
        await f.event(event(dto));
        assert.equal(children.length, count, `unsafe time.${key} rejected`);
      }
      for (const archived of [-0.5, 0.5, Number.MAX_SAFE_INTEGER + 1]) {
        const dto = full("ses_archived");
        dto.time.archived = archived;
        await f.event(event(dto));
        expectReport(children.at(-1), dto.id);
        close();
      }
      await f.dispose();
    });

    await t.test("schema arrays bounded at 256; oversized entries and mismatched DTOs never read", async () => {
      const f = await factory(plugin);
      for (const field of ["diffs", "permission"]) {
        for (const length of [256, 257]) {
          const dto = full(`ses_${field}_${length}`);
          const sample = field === "diffs" ? dto.summary.diffs[0] : dto.permission[0];
          let lengthReads = 0;
          let entryReads = 0;
          let otherReads = 0;
          const items = new Proxy(Array(length).fill(sample), {
            get(target, key) {
              if (key === "length") lengthReads++;
              else if (typeof key === "string" && /^(0|[1-9][0-9]*)$/.test(key)) entryReads++;
              else otherReads++;
              return Reflect.get(target, key);
            },
          });
          if (field === "diffs") dto.summary.diffs = items;
          else dto.permission = items;
          const count = children.length;
          await f.event(event(dto));
          assert.equal(lengthReads, 1, `${field} length snapshotted once`);
          assert.equal(entryReads, length === 256 ? 256 : 0, `${field} traversal bounded`);
          assert.equal(otherReads, 0, "no iterator or other array reads");
          assert.equal(children.length, count + (length === 256 ? 1 : 0));
          if (length === 256) {
            expectReport(children.at(-1), dto.id);
            close();
          }
        }
      }
      let dtoReads = 0;
      let idReads = 0;
      const dto = new Proxy(full("ses_identity"), {
        get(target, key) {
          if (key === "id") idReads++;
          else dtoReads++;
          return Reflect.get(target, key);
        },
      });
      const count = children.length;
      await f.event(event(dto, "session.updated", "ses_mismatch"));
      assert.equal(idReads, 1);
      assert.equal(dtoReads, 0, "mismatched identity rejected before full DTO or arrays");
      assert.equal(children.length, count);
      await f.dispose();
    });

    await t.test("opaque extras never read, scanned or serialized; getter errors contained", async () => {
      const f = await factory(plugin);
      let reads = 0;
      const opaque = new Proxy({}, {
        get() { throw Error("opaque read"); }, ownKeys() { throw Error("opaque enumeration"); },
      });
      const dto = full();
      dto.metadata = opaque;
      for (const value of [dto, dto.time, dto.summary, dto.summary.diffs[0], dto.tokens, dto.tokens.cache,
        dto.share, dto.model, dto.permission[0], dto.revert]) {
        Object.defineProperty(value, "secret", { enumerable: true, get() { throw Error("extra read"); } });
        Object.defineProperty(value, "toJSON", { get() { throw Error("serialization"); } });
      }
      Object.defineProperty(dto, "id", { get() { reads++; return "ses_snapshot"; } });
      const input = event(new Proxy(dto, { ownKeys() { throw Error("DTO enumeration"); } }), "session.updated", "ses_snapshot");
      reads = 0;
      Object.defineProperty(input.event.properties, "secret", { get() { throw Error("properties leakage"); } });
      await f.event(input);
      assert.equal(reads, 1, "identity read once");
      expectReport(children.at(-1), "ses_snapshot", "session.updated");
      close();
      const count = children.length;
      await f.event({ get event() { throw Error("malformed"); } });
      await f.event(event({ ...info(), get time() { throw Error("malformed"); } }));
      assert.equal(children.length, count);
      await f.dispose();
    });

    await t.test("Rust-compatible ID controls, whitespace, UTF-8 and surrogate boundaries", async () => {
      const f = await factory(plugin);
      const before = children.length;
      for (const id of [null, undefined, 1, {}, "", " ", "\u0085", "\u2003", "-x", "\u2003-x", "\u0085-x",
        "ses_a\n", "ses_a\u007f", "ses_a\u009f", "ses_" + "x".repeat(509), "ses_" + "\u00e9".repeat(255), "ses_\ud800", "ses_\udfff"]) {
        await f.event(event({ ...info(), id }));
      }
      assert.equal(children.length, before);
      for (const id of ["ses", "ses_ x ", "ses_" + "x".repeat(508), "ses_" + "\u00e9".repeat(254),
        "ses_" + "\u{1f600}".repeat(127), "ses_\ufeff-x", "ses_x\u200b"]) {
        await f.event(event(info(id)));
        expectReport(children.at(-1), id);
        close();
      }
      await f.dispose();
    });

    await t.test("32 pending plus one active, FIFO snapshots, repeated updates, drop incoming", async () => {
      const f = await factory(plugin);
      const start = children.length;
      const pending = [];
      for (let i = 0; i < 40; i++) {
        const dto = info(`ses_${i}`);
        const type = i % 2 ? "session.updated" : "session.created";
        const input = event(dto, type);
        const returned = f.event(input);
        assert.ok(returned instanceof Promise);
        await returned; // Resolves without waiting for the active child.
        if (i % 5 === 0) {
          await f.event(event({ ...info(`ses_child_${i}`), parentID: "ses_parent" }));
          await f.event(event({ id: `ses_${i}` }, "session.updated"));
        }
        pending.push([dto.id, type]);
        dto.id = "ses_mutated";
        input.event.type = "session.deleted";
        input.event.properties.sessionID = "ses_mutated";
      }
      assert.equal(children.length, start + 1);
      let overflowReads = 0;
      await f.event({ get event() { overflowReads++; return event(full()).event; } });
      assert.equal(overflowReads, 0, "full queue rejects before any event/DTO getters");
      for (const [id, type] of pending.slice(0, 33)) {
        expectReport(children.at(-1), id, type);
        close();
      }
      assert.equal(children.length, start + 33);
      for (let i = 0; i < 3; i++) await f.event(event(info("ses_repeat"), "session.updated"));
      for (let i = 0; i < 3; i++) { expectReport(children.at(-1), "ses_repeat", "session.updated"); close(); }
      await f.dispose();
    });

    await t.test("timeout/error/EPIPE retain slot until close; stale close cannot drain", async () => {
      const f = await factory(plugin);
      await f.event(event(info("ses_first")));
      const first = children.at(-1);
      const timer = timers.at(-1);
      assert.equal(timer.delay, 2000);
      first.emit("error", Error("ENOENT"));
      first.stdin.emit("error", Error("EPIPE"));
      await f.event(event(info("ses_second")));
      first.emit("exit", 0, null);
      assert.equal(children.at(-1), first, "exit is not close");
      timer.callback();
      assert.deepEqual(first.kills, ["SIGKILL"]);
      assert.equal(children.at(-1), first);
      close(first);
      assert.ok(timer.cleared);
      const second = children.at(-1);
      expectReport(second, "ses_second");
      await f.event(event(info("ses_third")));
      close(first);
      assert.equal(children.at(-1), second);
      close(second);
      expectReport(children.at(-1), "ses_third");
      close();
      stdinFailure = true;
      await f.event(event(info("ses_broken")));
      stdinFailure = false;
      const broken = children.at(-1);
      assert.deepEqual(broken.kills, ["SIGKILL"]);
      await f.event(event(info("ses_after_broken")));
      assert.equal(children.at(-1), broken);
      close(broken);
      expectReport(children.at(-1), "ses_after_broken");
      close();
      await f.event(event(info("ses_kill_throws")));
      const unkillable = children.at(-1);
      unkillable.kill = () => { throw Error("kill failure"); };
      timers.at(-1).callback();
      await f.dispose();
      close(unkillable);
      await f.dispose();
    });

    await t.test("synchronous failures drain bounded queue without retry or recursion", async () => {
      const f = await factory(plugin);
      await f.event(event(info("ses_active")));
      const child = children.at(-1);
      for (let i = 0; i < 40; i++) await f.event(event(info(`ses_fail_${i}`)));
      const before = spawnAttempts;
      spawnFailure = true;
      close(child);
      assert.equal(spawnAttempts - before, 32);
      spawnFailure = false;
      await f.event(event(info("ses_recovered")));
      expectReport(children.at(-1), "ses_recovered");
      close();
      await f.dispose();
    });

    await t.test("two factories isolate queues, disposal clears pending and holds active until close", async () => {
      const a = await factory(plugin);
      const b = await factory(plugin);
      await a.event(event(info("ses_a")));
      const ac = children.at(-1);
      await b.event(event(info("ses_b")));
      const bc = children.at(-1);
      await a.event(event(info("ses_a_pending")));
      await b.event(event(info("ses_b_pending")));
      await a.dispose();
      await a.dispose();
      assert.deepEqual(ac.kills, ["SIGKILL"]);
      assert.deepEqual(bc.kills, []);
      const count = children.length;
      await a.event(event(info("ses_stopped")));
      close(ac);
      assert.equal(children.length, count);
      close(bc);
      expectReport(children.at(-1), "ses_b_pending");
      close();
      await b.dispose();
    });

    setSpawn(originalSpawn);
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;

    await t.test("real quoted executable wrapper, exact stdin/argv, timeout, dispose and offline failure", async () => {
      const record = join(work, "record.json");
      const helper = join(work, "normal.cjs");
      await writeFile(helper, `const fs = require('node:fs');
let input = '';
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => fs.writeFileSync(${JSON.stringify(record)}, JSON.stringify({ argv: process.argv.slice(2), input, pid: process.pid })));
setInterval(() => {}, 1000);
`);
      // Bun cannot resolve an entrypoint with a literal backslash. A shell
      // wrapper execs a normal entrypoint, preserving the owned child PID.
      const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
      await writeFile(executable, `#!/bin/sh\nexec ${quote(process.execPath)} ${quote(helper)} "$@"\n`);
      await chmod(executable, 0o700);
      const f = await factory((await install()).plugin);
      async function captured() {
        for (let i = 0; i < 200; i++) {
          try { return JSON.parse(await readFile(record, "utf8")); } catch {}
          await pause(10);
        }
        assert.fail("controlled helper did not receive report");
      }
      await f.event(event(info("ses_real"), "session.updated"));
      const first = await captured();
      assert.deepEqual(first.argv, ["agent", "report-extension", "--provider", "opencode", "--socket", socket, "--event", "session.updated"]);
      assert.equal(first.input, JSON.stringify({ contract: "coven.opencode-extension.v1", event: "session.updated", session_id: "ses_real" }));
      await pause(2200);
      assert.throws(() => process.kill(first.pid, 0), { code: "ESRCH" });
      await rm(record);
      await f.event(event(info("ses_dispose")));
      const second = await captured();
      await f.dispose();
      await pause(100);
      assert.throws(() => process.kill(second.pid, 0), { code: "ESRCH" });
      const offline = await factory((await install(join(work, "missing-executable"))).plugin);
      await offline.event(event(info("ses_offline")));
      await pause(50);
      await offline.event(event(info("ses_offline_again")));
      await pause(50);
      await offline.dispose();
    });
  } finally {
    setSpawn(originalSpawn);
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
    await rm(work, { recursive: true, force: true });
  }
});
