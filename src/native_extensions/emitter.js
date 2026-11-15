import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { performance } from "node:perf_hooks";

const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const identifier = (v) => typeof v === "string" && v.trim().length > 0 &&
  Buffer.byteLength(v, "utf8") <= 512 && !/[\u0000-\u001f\u007f-\u009f\ud800-\udfff]/u.test(v);

function emitter() {
  const instance_id = randomUUID();
  const queue = [];
  let sequence = 0, active, stopped = false, draining = false;
  function kill(owned) {
    if (owned.killed) return;
    owned.killed = true;
    try { owned.child.kill("SIGKILL"); } catch {}
  }
  function drain() {
    if (draining) return;
    draining = true;
    try {
      while (!stopped && !active && queue.length) {
        const item = queue.shift();
        const remaining = 1900 - (performance.now() - item.at);
        if (remaining <= 0) continue;
        let child;
        try {
          child = spawn(EXECUTABLE, ["agent", "report-native", ...ROUTE],
            { shell: false, stdio: ["pipe", "ignore", "ignore"] });
        } catch { continue; }
        const owned = { child, killed: false, timer: undefined, closed: undefined };
        active = owned;
        owned.closed = new Promise((resolve) => {
          child.on("error", () => {});
          // Neither error, exit, nor timeout frees the active child slot.
          child.on("close", () => {
            clearTimeout(owned.timer);
            if (active === owned) active = undefined;
            resolve();
            drain();
          });
        });
        owned.timer = setTimeout(() => kill(owned), Math.max(0, 1900 - (performance.now() - item.at)));
        owned.timer.unref?.();
        try {
          child.stdin.on("error", () => {});
          if (performance.now() - item.at >= 1900) kill(owned);
          else child.stdin.end(JSON.stringify(item.payload));
        } catch { kill(owned); }
      }
    } finally { draining = false; }
  }
  return {
    send(session_id, event, extra = {}, at = performance.now()) {
      if (stopped || !identifier(session_id) || queue.length >= 32 || sequence >= Number.MAX_SAFE_INTEGER) return;
      queue.push({ at, payload: {
        contract: "coven.provider-events.v1", provider: PROVIDER, profile: PROFILE,
        instance_id, session_id, sequence: ++sequence, event, ...extra,
      } });
      drain();
    },
    clear() {
      queue.length = 0;
      if (!active) return Promise.resolve();
      const owned = active;
      clearTimeout(owned.timer);
      kill(owned);
      return owned.closed;
    },
    dispose() { stopped = true; return this.clear(); },
  };
}
