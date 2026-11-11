import { spawn } from "node:child_process";

// anomalyco/opencode v1.19.5, 810c0eba1a7348036f8bdf61e6632d0c36c6876c:
// packages/plugin/src/index.ts, packages/schema/src/v1/session.ts,
// packages/opencode/src/plugin/index.ts. The dev 611cee9 API is unsupported.
// Native hooks cover an exact directory instance, not a selected session or
// entire project. A full DTO without parentID classifies a root, not authority.
// Conservative bounded subset: larger native schema arrays are not reported.
const MAX_SCHEMA_ITEMS = 256;
const object = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const string = (value) => typeof value === "string";
const finite = (value) => typeof value === "number" && Number.isFinite(value);
const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const optional = (value, validate) => value === undefined || validate(value);
const array = (value, validate) => {
  if (!Array.isArray(value)) return false;
  const length = value.length;
  if (!integer(length) || length > MAX_SCHEMA_ITEMS) return false;
  for (let index = 0; index < length; index++) if (!validate(value[index])) return false;
  return true;
};
const prefixed = (value, prefix) => string(value) && value.startsWith(prefix);
const sessionID = (value) => prefixed(value, "ses") && value.length <= 512 &&
  Buffer.byteLength(value, "utf8") <= 512 &&
  !/[\u0000-\u001f\u007f-\u009f\ud800-\udfff]/u.test(value) &&
  !/^[\p{White_Space}]*$/u.test(value) && !/^[\p{White_Space}]*-/u.test(value);
const diff = (value) => object(value) && finite(value.additions) && finite(value.deletions) &&
  optional(value.file, string) && optional(value.patch, string) &&
  optional(value.status, (status) => ["added", "deleted", "modified"].includes(status));
const summary = (value) => object(value) && finite(value.additions) && finite(value.deletions) &&
  finite(value.files) && optional(value.diffs, (items) => array(items, diff));
const tokens = (value) => object(value) && finite(value.input) && finite(value.output) &&
  finite(value.reasoning) && object(value.cache) && finite(value.cache.read) && finite(value.cache.write);
const share = (value) => object(value) && string(value.url);
const model = (value) => object(value) && string(value.id) && string(value.providerID) &&
  optional(value.variant, string);
const rule = (value) => object(value) && string(value.permission) && string(value.pattern) &&
  ["allow", "deny", "ask"].includes(value.action);
const revert = (value) => object(value) && prefixed(value.messageID, "msg") &&
  optional(value.partID, (id) => prefixed(id, "prt")) &&
  optional(value.snapshot, string) && optional(value.diff, string);

function root(info, id) {
  // Never enumerate, serialize, or inspect opaque metadata values or extras.
  return object(info) && sessionID(id) && string(info.slug) && string(info.projectID) &&
    string(info.directory) && string(info.title) && string(info.version) &&
    object(info.time) && integer(info.time.created) && integer(info.time.updated) &&
    optional(info.time.compacting, integer) && optional(info.time.archived, finite) &&
    optional(info.workspaceID, (id) => prefixed(id, "wrk")) && optional(info.path, string) &&
    info.parentID === undefined && optional(info.summary, summary) && optional(info.cost, finite) &&
    optional(info.tokens, tokens) && optional(info.share, share) && optional(info.agent, string) &&
    optional(info.model, model) && optional(info.metadata, object) &&
    optional(info.permission, (items) => array(items, rule)) && optional(info.revert, revert);
}

export async function CovenAgentReferencePlugin() {
  const queue = [];
  let active;
  let stopped = false;
  let draining = false;

  function kill(owned) {
    if (owned.killed) return;
    owned.killed = true;
    try { owned.child.kill("SIGKILL"); } catch {}
  }

  function drain() {
    if (draining) return;
    draining = true;
    try {
      // Synchronous spawn failures consume at most the 32 queued snapshots,
      // without recursive draining or retrying any failed report.
      while (!stopped && !active && queue.length) {
        const snapshot = queue.shift();
        let child;
        try {
          child = spawn(EXECUTABLE, [
            "agent", "report-extension", "--provider", "opencode", "--socket", SOCKET,
            "--event", snapshot.event,
          ], { shell: false, stdio: ["pipe", "ignore", "ignore"] });
        } catch {
          continue;
        }
        const owned = { child, timer: undefined, killed: false };
        active = owned;
        child.on("error", () => {});
        child.on("close", () => {
          clearTimeout(owned.timer);
          if (active !== owned) return;
          active = undefined;
          drain();
        });
        owned.timer = setTimeout(() => kill(owned), 2000);
        owned.timer.unref?.();
        try {
          child.stdin.on("error", () => {});
          child.stdin.end(JSON.stringify({
            contract: "coven.opencode-extension.v1",
            event: snapshot.event,
            session_id: snapshot.id,
          }));
        } catch {
          kill(owned);
        }
      }
    } finally {
      draining = false;
    }
  }

  return {
    async event(input) {
      // OpenCode does not await native event hook promises. Contain failures
      // here, and copy only identity/type before any child lifecycle work.
      try {
        if (stopped || queue.length >= 32) return;
        const event = input.event;
        const type = event.type;
        if (type !== "session.created" && type !== "session.updated") return;
        const properties = event.properties;
        const info = properties.info;
        const id = info.id;
        if (properties.sessionID !== id) return;
        if (!root(info, id)) return;
        queue.push({ id, event: type });
        drain();
      } catch {}
    },
    async dispose() {
      stopped = true;
      queue.length = 0;
      if (active) {
        clearTimeout(active.timer);
        kill(active);
      }
      // Resolves after requesting termination, not a guarantee of close.
    },
  };
}
