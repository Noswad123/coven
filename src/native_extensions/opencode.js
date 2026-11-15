// Exact-directory EventV2 bridge at 810c0eba1a7348036f8bdf61e6632d0c36c6876c.
// Legacy SDK metadata lookup is on demand only, never during plugin bootstrap.
const nativeTool = (v) => object(v) && prefixed(v.messageID, "msg") && string(v.callID);
const questionOption = (v) => object(v) && string(v.label) && string(v.description);
const questionInfo = (v) => object(v) && string(v.question) && string(v.header) &&
  array(v.options, questionOption) && optional(v.multiple, (v) => typeof v === "boolean") &&
  optional(v.custom, (v) => typeof v === "boolean");

function liveExtra(type, p) {
  if (type === "session.status") {
    const s = p.status;
    if (!object(s)) return;
    const status = s.type;
    if (!["busy", "retry", "idle"].includes(status)) return;
    if (status === "retry" && !(integer(s.attempt) && string(s.message) && integer(s.next) &&
        optional(s.action, (v) => object(v) && string(v.reason) && string(v.provider) &&
          string(v.title) && string(v.message) && string(v.label) && optional(v.link, string)))) return;
    return { status };
  }
  if (!["permission.asked", "permission.replied", "question.asked", "question.replied", "question.rejected"].includes(type)) return;
  const request_id = type.endsWith(".asked") ? p.id : p.requestID;
  const prefix = type.startsWith("permission.") ? "per" : "que";
  if (!identifier(request_id) || !request_id.startsWith(prefix)) return;
  if (type === "permission.asked" && !(string(p.permission) && array(p.patterns, string) &&
      object(p.metadata) && array(p.always, string) && optional(p.tool, nativeTool))) return;
  if (type === "permission.replied" && !["once", "always", "reject"].includes(p.reply)) return;
  if (type === "question.asked" && !(array(p.questions, questionInfo) && optional(p.tool, nativeTool))) return;
  if (type === "question.replied" && !array(p.answers, (v) => array(v, string))) return;
  return { request_id };
}

export async function CovenNativeEventsPlugin(input) {
  const out = emitter();
  const known = new Map();
  const queue = [];
  let stopped = false, active, draining = false;

  function discard(id, liveOnly = false) {
    for (let i = queue.length - 1; i >= 0; i--)
      if (queue[i].id === id && (!liveOnly || !queue[i].lifecycle)) queue.splice(i, 1);
  }

  function exclude(id) {
    if (known.has(id) || known.size < 512) known.set(id, false);
    discard(id);
    if (active?.id === id) active.cancel();
  }

  function qualify(item) {
    const controller = new AbortController();
    let cancel;
    const canceled = new Promise(resolve => { cancel = () => {
      resolve(undefined);
      controller.abort();
    }; });
    const owned = { id: item.id, cancel, done: undefined };
    active = owned;
    // Race as well as abort: a non-cooperative SDK must not retain the work slot.
    const timer = setTimeout(cancel, Math.max(0, 1900 - (performance.now() - item.at)));
    timer.unref?.();
    owned.done = (async () => {
      try {
        const result = await Promise.race([
          input.client.session.get({ path: { id: item.id }, throwOnError: true, signal: controller.signal }),
          canceled,
        ]);
        if (stopped || performance.now() - item.at >= 1900 || known.get(item.id) === false) return;
        // A later full lifecycle DTO may already have resolved this classification.
        if (known.get(item.id) !== true && !controller.signal.aborted) {
          const info = result?.data;
          const child = object(info) && info.id === item.id && info.parentID !== undefined;
          const valid = !child && root(info, item.id);
          if (!stopped && !controller.signal.aborted && performance.now() - item.at < 1900 && known.get(item.id) !== false) {
            if (child) exclude(item.id);
            else if (valid) known.set(item.id, true);
          }
        }
        if (known.get(item.id) === true) out.send(item.id, item.event, item.extra, item.at);
      } catch {} finally {
        clearTimeout(timer);
        // One failed lookup drops its waiting cohort, not retries of old events.
        if (known.get(item.id) !== true) discard(item.id, true);
        if (active === owned) active = undefined;
        drain();
      }
    })();
  }

  function drain() {
    if (stopped || draining || active) return;
    draining = true;
    try {
      while (!stopped && !active && queue.length) {
        const item = queue.shift();
        if (performance.now() - item.at >= 1900) continue;
        if (item.lifecycle || known.get(item.id) === true) {
          out.send(item.id, item.event, item.extra, item.at);
        } else if (known.get(item.id) !== false && (known.has(item.id) || known.size < 512) &&
            typeof input?.client?.session?.get === "function") {
          known.set(item.id, undefined);
          qualify(item);
        }
      }
    } catch { queue.length = 0; } finally { draining = false; }
  }

  function enqueue(item) {
    if (queue.length >= 32 || performance.now() - item.at >= 1900) return;
    queue.push(item);
    drain();
  }

  return {
    async event(callback) {
      try {
        const at = performance.now();
        if (stopped) return;
        const { type, properties: p } = callback.event;
        if (!object(p) || !sessionID(p.sessionID)) return;
        const id = p.sessionID;
        if (["session.created", "session.updated", "session.deleted"].includes(type)) {
          // Once excluded, a child/tombstone cannot be reclassified by later DTOs.
          if (known.get(id) === false) return;
          if (type === "session.deleted") {
            const report = root(p.info, id);
            exclude(id);
            if (report) enqueue({ id, event: type, extra: {}, at, lifecycle: true });
            return;
          }
          if (object(p.info) && p.info.id === id && p.info.parentID !== undefined) {
            exclude(id);
            return;
          }
          if (!root(p.info, id)) return;
          if (!known.has(id) && known.size >= 512) return;
          known.set(id, true);
          if (active?.id === id) active.cancel();
          enqueue({ id, event: type, extra: {}, at, lifecycle: true });
          return;
        }
        if (known.get(id) === false || (!known.has(id) && known.size >= 512)) return;
        const extra = liveExtra(type, p);
        if (extra) enqueue({ id, event: type, extra, at });
      } catch {}
    },
    async dispose() {
      stopped = true;
      queue.length = 0;
      known.clear();
      const pending = active;
      pending?.cancel();
      await Promise.all([pending?.done, out.dispose()]);
    },
  };
}
