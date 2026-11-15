// Bounded conservative validation of pinned SessionInfo, not a partial DTO.
const string = (v) => typeof v === "string";
const finite = (v) => typeof v === "number" && Number.isFinite(v);
const integer = (v) => Number.isSafeInteger(v) && v >= 0;
const optional = (v, check) => v === undefined || check(v);
const array = (v, check) => {
  if (!Array.isArray(v) || v.length > 256) return false;
  for (let i = 0; i < v.length; i++) if (!check(v[i])) return false;
  return true;
};
const prefixed = (v, prefix) => string(v) && v.startsWith(prefix);
const sessionID = (v) => identifier(v) && prefixed(v, "ses");
const diff = (v) => object(v) && finite(v.additions) && finite(v.deletions) &&
  optional(v.file, string) && optional(v.patch, string) &&
  optional(v.status, (s) => ["added", "deleted", "modified"].includes(s));
const summary = (v) => object(v) && finite(v.additions) && finite(v.deletions) &&
  finite(v.files) && optional(v.diffs, (v) => array(v, diff));
const tokens = (v) => object(v) && finite(v.input) && finite(v.output) && finite(v.reasoning) &&
  object(v.cache) && finite(v.cache.read) && finite(v.cache.write);
const share = (v) => object(v) && string(v.url);
const model = (v) => object(v) && string(v.id) && string(v.providerID) && optional(v.variant, string);
const rule = (v) => object(v) && string(v.permission) && string(v.pattern) && ["allow", "deny", "ask"].includes(v.action);
const revert = (v) => object(v) && prefixed(v.messageID, "msg") &&
  optional(v.partID, (v) => prefixed(v, "prt")) && optional(v.snapshot, string) && optional(v.diff, string);
function root(info, id) {
  return object(info) && sessionID(id) && info.id === id && string(info.slug) &&
    string(info.projectID) && string(info.directory) && string(info.title) && string(info.version) &&
    object(info.time) && integer(info.time.created) && integer(info.time.updated) &&
    optional(info.time.compacting, integer) && optional(info.time.archived, finite) &&
    optional(info.workspaceID, (v) => prefixed(v, "wrk")) && optional(info.path, string) &&
    info.parentID === undefined && optional(info.summary, summary) && optional(info.cost, finite) &&
    optional(info.tokens, tokens) && optional(info.share, share) && optional(info.agent, string) &&
    optional(info.model, model) && optional(info.metadata, object) &&
    optional(info.permission, (v) => array(v, rule)) && optional(info.revert, revert);
}
