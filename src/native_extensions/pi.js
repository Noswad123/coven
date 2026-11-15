// Pi 6e11a00: context activity, not producer-checked root or completion proof.
export default function CovenNativeEvents(pi) {
  const out = emitter();
  let current, suspended = true, stopped = false;
  function context(ctx) {
    if (ctx.hasUI !== true || !["tui", "rpc"].includes(ctx.mode)) return;
    const id = ctx.sessionManager.getSessionId();
    return identifier(id) ? id : undefined;
  }
  pi.on("session_start", (_event, ctx) => {
    try {
      const at = performance.now();
      if (stopped) return;
      const id = context(ctx);
      if (id !== current) out.clear();
      current = id;
      suspended = !id;
      if (id) out.send(id, "session.updated", {}, at);
    } catch { current = undefined; suspended = true; out.clear(); }
  });
  for (const name of ["agent_start", "agent_settled"]) {
    pi.on(name, (_event, ctx) => {
      try {
        const at = performance.now();
        if (stopped || suspended) return;
        const id = context(ctx);
        if (!id || id !== current) { current = undefined; suspended = true; out.clear(); return; }
        if (name === "agent_settled" && ctx.isIdle() !== true) return;
        out.send(id, "session.status", { status: name === "agent_start" ? "busy" : "idle" }, at);
      } catch {}
    });
  }
  pi.on("session_shutdown", (event) => {
    try {
      suspended = true;
      current = undefined;
      if (event.reason === "quit" || event.reason === "reload") {
        stopped = true;
        return out.dispose();
      }
      return out.clear();
    } catch {}
  });
}
