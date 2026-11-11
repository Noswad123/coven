const EXECUTABLE = "/abs/coven";
const SOCKET = "/abs/socket";
import { spawn } from "node:child_process";

// Native contract: earendil-works/pi 6e11a00a01a568ef0ae0df5074acf02fc107c34b,
// package 1.5.2 on main (not a release tag). hasUI includes TUI and RPC;
// it is eligibility, not proof of a root session. Reports are best effort:
// load-time events, overlapping callbacks, and timed-out reports can be lost.
export default function (pi) {
  let active;
  let stopped = false;

  pi.on("session_start", (_event, ctx) => {
    if (stopped || active) return;
    let sessionId;
    try {
      if (ctx.hasUI !== true) return;
      sessionId = ctx.sessionManager.getSessionId();
      // Rust str::trim uses Unicode White_Space, not ECMAScript trim's BOM rule.
      if (typeof sessionId !== "string" || sessionId.length > 512 ||
          Buffer.byteLength(sessionId, "utf8") > 512 ||
          /[\u0000-\u001f\u007f-\u009f\ud800-\udfff]/u.test(sessionId) ||
          /^[\p{White_Space}]*$/u.test(sessionId) ||
          /^[\p{White_Space}]*-/u.test(sessionId)) return;
    } catch {
      return;
    }

    let child;
    try {
      child = spawn(EXECUTABLE, [
        "agent", "report-extension", "--provider", "pi", "--socket", SOCKET,
        "--event", "session_start",
      ], { shell: false, stdio: ["pipe", "ignore", "ignore"] });
    } catch {
      return;
    }
    const owned = { child, timer: undefined };
    active = owned;
    const kill = () => {
      try { child.kill("SIGKILL"); } catch {}
    };
    // An error or kill request is not evidence of exit. Only close releases
    // this factory's slot, including failed asynchronous spawn attempts.
    child.on("error", () => {});
    child.on("close", () => {
      clearTimeout(owned.timer);
      if (active === owned) active = undefined;
    });
    owned.timer = setTimeout(kill, 2000);
    owned.timer.unref?.();
    try {
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify({
        contract: "coven.pi-extension.v1",
        event: "session_start",
        session_id: sessionId,
      }));
    } catch {
      kill();
    }
  });

  pi.on("session_shutdown", () => {
    stopped = true;
    if (active) {
      clearTimeout(active.timer);
      try { active.child.kill("SIGKILL"); } catch {}
    }
  });
}
