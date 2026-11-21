export const CONTRACT = 'coven.fleet-observer.v1';
export const MAX_UNRESOLVED_PROMPTS = 32;
const AGENT_ID = /^[a-f0-9]{64}$/;
const ACTIVITIES = { working: 'Working (heuristic)', idle: 'Idle (not completion)', blocked: 'Blocked hint', unknown: 'Activity unknown' };
const timestamp = value => Number.isSafeInteger(value) && value >= 0;
const positive = value => Number.isSafeInteger(value) && value > 0;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const string = value => typeof value === 'string';

export function validateFleet(value) {
  if (!record(value) || value.contract !== CONTRACT || !timestamp(value.generated_at_ms)
    || !positive(value.refresh_interval_ms) || !positive(value.stale_after_ms)
    || !Array.isArray(value.endpoints) || value.endpoints.length > 8) throw new Error('invalid_snapshot');
  const endpoints = new Set();
  const agents = new Set();
  for (const endpoint of value.endpoints) {
    if (!record(endpoint) || !string(endpoint.id) || !endpoint.id || endpoints.has(endpoint.id)
      || !string(endpoint.label) || !['reachable', 'unavailable', 'unsupported'].includes(endpoint.connection)
      || !timestamp(endpoint.last_attempt_at_ms)
      || !(endpoint.observed_at_ms === null || timestamp(endpoint.observed_at_ms))
      || !(endpoint.error === null || (string(endpoint.error) && /^[a-z0-9_]+$/.test(endpoint.error)))
      || !(endpoint.discovery_status === undefined || ['observed', 'unsupported', 'unavailable'].includes(endpoint.discovery_status))
      || !(endpoint.unclassified_panes === undefined || endpoint.unclassified_panes === null || timestamp(endpoint.unclassified_panes))
      || !Array.isArray(endpoint.agents) || endpoint.agents.length > 128) throw new Error('invalid_snapshot');
    endpoints.add(endpoint.id);
    for (const agent of endpoint.agents) {
      if (!record(agent) || !string(agent.id) || !AGENT_ID.test(agent.id) || agents.has(agent.id)
        || !string(agent.title) || !string(agent.provider) || !string(agent.project)
        || !Object.hasOwn(ACTIVITIES, agent.activity) || !['kitsune_snapshot', 'coven_process_discovery'].includes(agent.evidence)
        || !(agent.provider_candidates === undefined || (Array.isArray(agent.provider_candidates)
          && agent.provider_candidates.length <= 22 && agent.provider_candidates.every(family => string(family) && family.length > 0)))
        || !timestamp(agent.observed_at_ms) || typeof agent.output_available !== 'boolean') throw new Error('invalid_snapshot');
      if (agent.evidence === 'coven_process_discovery'
        && (agent.activity !== 'unknown' || agent.output_available !== false
          || agent.provider_candidates === undefined || endpoint.discovery_status === undefined
          || endpoint.unclassified_panes === undefined)) throw new Error('invalid_snapshot');
      agents.add(agent.id);
    }
  }
  return value;
}

export function initialState() { return { snapshot: null, failed: false, error: null }; }

export function receiveFleet(state, value, now = Date.now()) {
  try {
    const snapshot = validateFleet(value);
    // A delayed or implausibly future snapshot must not renew a reference's lease.
    if (snapshot.generated_at_ms > now + 5000
      || (state.snapshot && snapshot.generated_at_ms < state.snapshot.generated_at_ms)) throw new Error('invalid_snapshot');
    return { snapshot, failed: false, error: null };
  } catch {
    return failFleet(state, 'invalid_snapshot');
  }
}

export function failFleet(state, error = 'network') { return { ...state, failed: true, error }; }

export function isFresh(state, endpoint, agent, now) {
  if (!state.snapshot || state.failed || endpoint.connection !== 'reachable') return false;
  if (agent.evidence === 'coven_process_discovery' && endpoint.discovery_status !== 'observed') return false;
  const times = [state.snapshot.generated_at_ms, endpoint.observed_at_ms, agent.observed_at_ms];
  return times.every(time => timestamp(time) && time <= now + 5000 && now - time < state.snapshot.stale_after_ms);
}

export function ageLabel(observedAt, now) {
  if (!timestamp(observedAt)) return 'Not observed';
  const seconds = Math.max(0, Math.floor((now - observedAt) / 1000));
  if (seconds < 1) return 'Observed just now';
  if (seconds < 60) return `Observed ${seconds}s ago`;
  if (seconds < 3600) return `Observed ${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `Observed ${Math.floor(seconds / 3600)}h ago`;
  return `Observed ${Math.floor(seconds / 86400)}d ago`;
}

export function projectFleet(state, host = '', now = Date.now()) {
  const endpoints = (state.snapshot?.endpoints ?? []).filter(endpoint => !host || endpoint.id === host);
  const cards = endpoints.flatMap(endpoint => endpoint.agents.map(agent => {
    const fresh = isFresh(state, endpoint, agent, now);
    const processEvidence = agent.evidence === 'coven_process_discovery';
    const evidenceLabel = processEvidence ? 'Coven process evidence' : 'Kitsune metadata';
    const familyHints = agent.provider_candidates?.length
      ? `Process family hints (not identity): ${agent.provider_candidates.join(', ')}`
      : processEvidence ? 'Process family hints unavailable; identity unconfirmed.' : '';
    return { key: JSON.stringify([endpoint.id, agent.id]), endpoint, agent, fresh,
      outputEnabled: fresh && !processEvidence && agent.output_available,
      evidenceLabel, familyHints,
      providerLabel: processEvidence ? 'Provider identity unconfirmed' : agent.provider || 'Unknown provider',
      activityLabel: ACTIVITIES[agent.activity], age: ageLabel(agent.observed_at_ms, now) };
  }));
  cards.sort((a, b) => Number(b.fresh) - Number(a.fresh)
    || a.endpoint.label.localeCompare(b.endpoint.label)
    || a.agent.title.localeCompare(b.agent.title) || a.key.localeCompare(b.key));
  let emptyTitle = 'Opening the index';
  let emptyDescription = 'No fleet data has been received yet.';
  if (state.failed) {
    emptyTitle = state.snapshot ? 'Last-known index is empty' : 'No observation available';
    emptyDescription = 'The API is unavailable or its response could not be trusted. Coverage is incomplete; this is not a confirmed empty fleet.';
  } else if (state.snapshot) {
    const old = state.snapshot.generated_at_ms > now + 5000 || now - state.snapshot.generated_at_ms >= state.snapshot.stale_after_ms;
    const unhealthy = endpoints.some(endpoint => endpoint.connection !== 'reachable' || endpoint.observed_at_ms === null
      || endpoint.observed_at_ms > now + 5000 || now - endpoint.observed_at_ms >= state.snapshot.stale_after_ms);
    emptyTitle = old || unhealthy ? 'No current agent observations' : 'No recognized agent evidence';
    emptyDescription = endpoints.length
      ? `${old || unhealthy ? 'A host is stale, unavailable or unsupported. ' : 'No recognized Kitsune metadata or Coven process evidence was reported. '}${endpoints.map(endpoint => endpointCoverage(endpoint)).join(' ')} Coverage is incomplete; an empty index does not establish that no agents are running.`
      : 'No hosts are configured in this snapshot. This is not an inventory of all running agents.';
  }
  return { endpoints, cards, empty: state.snapshot !== null && cards.length === 0, emptyTitle, emptyDescription };
}

export function endpointCoverage(endpoint) {
  const count = endpoint.unclassified_panes;
  return `Process discovery: ${endpoint.discovery_status ?? 'not reported'}. ${count == null
    ? 'Unclassified terminal count unknown.'
    : `${count} unclassified terminal${count === 1 ? '' : 's'} (not identified as agents).`} Scan completeness unknown.`;
}

export function validateOutput(value, agentId, now, staleAfter) {
  if (!record(value) || value.contract !== CONTRACT || value.agent_id !== agentId
    || !AGENT_ID.test(agentId) || !timestamp(value.observed_at_ms)
    || value.observed_at_ms > now + 5000 || now - value.observed_at_ms >= staleAfter
    || !string(value.text) || new TextEncoder().encode(value.text).length > 65536
    || typeof value.truncated !== 'boolean'
    || value.attribution !== 'matching_snapshot_brackets_best_effort') throw new Error('invalid_output');
  return value;
}

// Tickets distinguish even two consecutive openings of the exact same card.
export function createOutputGate() {
  let revision = 0;
  let selected = null;
  return {
    begin(key) { selected = key; return { key, revision: ++revision }; },
    invalidate() { selected = null; revision++; },
    accepts(ticket, currentKey) { return ticket.revision === revision && ticket.key === selected && ticket.key === currentKey; }
  };
}

export function errorMessage(code) {
  if (code === 'auth') return 'Access denied. Open this app through your private Tailscale Serve address, sign in to the intended tailnet, and ask the operator to check the allowed identity and origin.';
  if (code === 'invalid_snapshot') return 'The server returned an invalid or out-of-order snapshot. Last-known observations are retained, but output is disabled.';
  return 'The observation API could not be reached. Check your tailnet connection and gateway. Last-known observations are retained, but output is disabled.';
}

export function responseError(status, body) {
  if (status === 403 && ['output_disabled', 'output_unsupported'].includes(body?.error?.code)) return body.error.code;
  if (status === 401 || status === 403) return 'auth';
  return 'request_failed';
}

// All server strings, including terminal text, cross one inert text boundary.
export function setText(node, value) { node.textContent = value; return node; }

// Older Djinn UIs decorate terminal titles, not the native session's actual name.
// Remove only that exact leading decoration for known Djinn observations.
export function displaySessionTitle(agent) {
  const title = agent.title || 'Untitled session';
  return agent.provider === 'djinn' && title.startsWith('OC | ')
    ? title.slice(5) || 'Untitled session' : title;
}

export function validateConversations(value) {
  if (!record(value) || value.contract !== 'coven.conversations.v1'
    || !Array.isArray(value.conversations) || value.conversations.length > 64) throw new Error('invalid_conversations');
  if (value.sources !== undefined && (!Array.isArray(value.sources) || value.sources.length > 8
    || value.sources.some(source => !record(source) || !string(source.id) || !/^[a-zA-Z0-9_.-]{1,128}$/.test(source.id)
      || !string(source.label) || source.label.length > 256 || !string(source.team)
      || !/^[a-zA-Z0-9_.-]{1,128}$/.test(source.team)
      || !['observed', 'busy', 'unavailable'].includes(source.status)))) throw new Error('invalid_conversations');
  if (value.omitted_conversations !== undefined && !timestamp(value.omitted_conversations)) throw new Error('invalid_conversations');
  const ids = new Set();
  for (const target of value.conversations) {
    if (!record(target) || !string(target.id) || !AGENT_ID.test(target.id) || ids.has(target.id)
      || !string(target.label) || !target.label || target.label.length > 256
      || !string(target.team) || !/^[a-zA-Z0-9_.-]{1,128}$/.test(target.team)
      || target.host !== 'Local native'
      || !(target.can_submit === undefined || typeof target.can_submit === 'boolean')
      || !(target.max_prompt_bytes === undefined || target.max_prompt_bytes === 65536)
      || (target.can_submit === true && target.max_prompt_bytes !== 65536)) throw new Error('invalid_conversations');
    if (!(target.association_status === undefined || ['verified', 'unavailable', 'ambiguous'].includes(target.association_status))
      || !(target.card === undefined || target.card === null || (record(target.card)
        && AGENT_ID.test(target.card.fleet_agent_id) && AGENT_ID.test(target.card.binding_id)
        && string(target.card.fleet_endpoint_id) && /^[a-zA-Z0-9_.-]{1,128}$/.test(target.card.fleet_endpoint_id)))) throw new Error('invalid_conversations');
    ids.add(target.id);
  }
  return value.conversations;
}

export function projectSessions(state, targets, host = '', team = '', now = Date.now()) {
  // Aging changes presentation, not keyboard order in the unified grid.
  const fleet = projectFleet(state, '', now).cards.sort((a, b) => a.key.localeCompare(b.key));
  const links = new Map();
  for (const target of targets) {
    if (!target.card) continue;
    const key = JSON.stringify([target.card.fleet_endpoint_id, target.card.fleet_agent_id]);
    const rows = links.get(key) ?? []; rows.push(target); links.set(key, rows);
  }
  const cards = fleet.map(card => {
    const candidates = links.get(card.key) ?? [];
    const verified = card.fresh && candidates.length === 1 && candidates[0].association_status === 'verified' ? candidates[0] : null;
    const reason = !card.fresh ? 'Fleet observation is stale or unavailable.'
      : candidates.length > 1 || candidates.some(target => target.association_status === 'ambiguous') ? 'Multiple conversations claim this terminal.'
      : 'No verified conversation link. Native provenance may be missing or unsupported.';
    return {...card, target: verified ? {...verified, binding: {...verified.card}} : null,
      chatUnavailable: verified ? '' : reason};
  });
  return cards.filter(card => (!host || card.endpoint.id === host)
    && (!team || card.target?.team === team));
}

export function validateConversation(value, target) {
  if (!record(value) || value.contract !== 'coven.conversations.v1' || value.id !== target.id
    || value.team !== target.team || value.source !== 'djinn.conversation-read.v1'
    || !Array.isArray(value.messages) || value.messages.length > 20
    || !['has_older', 'truncated', 'omitted_parts'].every(k => typeof value[k] === 'boolean')
    || !string(value.projection_sha256) || !AGENT_ID.test(value.projection_sha256)) throw new Error('invalid_conversation');
  let bytes = 0;
  for (const message of value.messages) {
    if (!record(message) || !['user', 'assistant'].includes(message.role)
      || !string(message.text) || typeof message.truncated !== 'boolean') throw new Error('invalid_conversation');
    bytes += new TextEncoder().encode(message.text).length;
  }
  if (bytes > 8192) throw new Error('invalid_conversation');
  return value;
}

export function validateAttention(value) {
  if (!record(value) || value.contract !== 'coven.attention.v1'
    || !['observed', 'partial', 'unavailable'].includes(value.state)
    || !Array.isArray(value.items) || value.items.length > 256
    || !(value.omitted_count === null || timestamp(value.omitted_count))
    || !timestamp(value.unavailable_count) || !timestamp(value.unlinked_count)) throw new Error('invalid_attention');
  const ids = new Set(), alerts = new Set();
  for (const item of value.items) {
    if (!record(item) || !['id', 'alert_id', 'conversation_id', 'fleet_agent_id', 'binding_id'].every(k => string(item[k]) && AGENT_ID.test(item[k]))
      || ids.has(item.id) || alerts.has(item.alert_id)
      || !string(item.fleet_endpoint_id) || !/^[a-zA-Z0-9_.-]{1,128}$/.test(item.fleet_endpoint_id)
      || !string(item.team) || !/^[a-zA-Z0-9_.-]{1,128}$/.test(item.team)
      || !['question', 'permission'].includes(item.kind) || typeof item.can_respond !== 'boolean'
      || typeof item.details_available !== 'boolean') throw new Error('invalid_attention');
    ids.add(item.id); alerts.add(item.alert_id);
  }
  return value;
}

export function attentionLabel(value) {
  if (!value) return 'Attention unknown';
  const count = new Set(value.items.map(item => item.alert_id)).size;
  const complete = value.state === 'observed' && value.omitted_count === 0
    && value.unavailable_count === 0 && value.unlinked_count === 0;
  return `${count ? `${count} needs response` : complete ? 'No pending requests observed' : 'Attention unknown'}${!complete && count ? ' / coverage partial' : ''}`;
}

export function validateDecision(value, item) {
  const closed = (v, keys) => record(v) && Object.keys(v).length === keys.length && keys.every(k => Object.hasOwn(v, k));
  const text = (s, max) => string(s) && new TextEncoder().encode(s).length <= max;
  if (!closed(value, ['contract', 'id', 'alert_id', 'kind', 'can_respond', 'decision'])
    || value.contract !== 'coven.decision.v1' || value.id !== item.id || value.alert_id !== item.alert_id
    || value.kind !== item.kind || typeof value.can_respond !== 'boolean'
    || new TextEncoder().encode(JSON.stringify(value.decision)).length > 32768) throw new Error('invalid_decision');
  const d = value.decision;
  if (item.kind === 'question') {
    if (!closed(d, ['kind', 'questions']) || d.kind !== 'question' || !Array.isArray(d.questions)
      || !d.questions.length || d.questions.length > 8) throw new Error('invalid_decision');
    for (const q of d.questions) {
      if (!closed(q, ['question', 'header', 'options', 'multiple', 'custom']) || !text(q.question, 4096)
        || !text(q.header, 256) || typeof q.multiple !== 'boolean' || typeof q.custom !== 'boolean'
        || !Array.isArray(q.options) || q.options.length > 16
        || q.options.some(o => !closed(o, ['label', 'description']) || !text(o.label, 256) || !o.label.trim() || !text(o.description, 1024))
        || new Set(q.options.map(o => o.label)).size !== q.options.length) throw new Error('invalid_decision');
    }
  } else {
    if (!closed(d, ['kind', 'permission', 'patterns', 'always_patterns', 'metadata_json', 'deny_scope'])
      || d.kind !== 'permission' || !text(d.permission, 1024) || !text(d.metadata_json, 4096)
      || d.deny_scope !== 'session_pending' || !['patterns', 'always_patterns'].every(k => Array.isArray(d[k])
        && d[k].length <= 32 && d[k].every(s => text(s, 1024)))) throw new Error('invalid_decision');
  }
  return value;
}

export function questionAnswers(questions, fields) {
  const answers = questions.map((q, i) => {
    const row = fields[i].options.filter(f => f.checked).map(f => f.value);
    if (q.custom && fields[i].custom?.value) row.push(fields[i].custom.value);
    if ((!q.multiple && row.length !== 1) || row.length > q.options.length + 1 || new Set(row).size !== row.length
      || row.some(s => !s.trim() || new TextEncoder().encode(s).length > 4096
        || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(s))) throw new Error('invalid_answers');
    return row;
  });
  if (answers.flat().reduce((n, s) => n + new TextEncoder().encode(s).length, 0) > 8192) throw new Error('invalid_answers');
  return answers;
}

export async function subscriptionID(endpoint) {
  if (!string(endpoint) || endpoint.length > 2048 || new TextEncoder().encode(endpoint).length > 2048) throw new Error('invalid_subscription');
  const url = new URL(endpoint);
  if (url.protocol !== 'https:' || url.username || url.password || url.hash || (url.port && url.port !== '443')) throw new Error('invalid_subscription');
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(endpoint));
  return [...new Uint8Array(bytes)].map(b => b.toString(16).padStart(2, '0')).join('');
}

if (typeof document !== 'undefined') bootstrap();

function bootstrap() {
  const $ = id => document.getElementById(id);
  const element = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) setText(node, text);
    return node;
  };
  let state = initialState();
  let host = '';
  let pollTimer;
  let fleetRequest = null;
  let failures = 0;
  let refreshOnResume = false;
  let selectedKey = null;
  let outputRequest = null;
  let renderedOutput = null;
  let authRevision = 0;
  let attention = null, attentionAt = null, attentionRequest = null;
  let attentionTimer;
  let requestListKey = null;
  let decisionRevision = 0, decisionRequest = null, decisionItem = null, decisionDetail = null;
  let decisionBusy = false;
  const uncertainDecisions = new Map();
  const attentionFresh = () => attentionAt !== null && Date.now() - attentionAt < 15000;
  const decisionNotice = 'Response uncertain. Inspect this exact request before a new deliberate choice. No automatic retry.';
  let alertNavigation = null;
  let alertUnverified = false;
  const alertHash = () => globalThis.location?.hash?.match(/^#attention=([a-f0-9]{64})$/)?.[1] ?? null;
  let alertsBusy = false;
  let alertsChecking = false;
  let localAlerts = 'off';
  let pushOperationRevision = 0;
  const pendingPushRemovals = new Set();
  const unresolvedPushCreations = new Set();
  let unidentifiedPushRemoval = false;
  const outputGate = createOutputGate();
  const dialog = $('output-dialog');
  const cardNodes = new Map();
  const dialogReturnTargets = new Map();
  const sessions = (host = '', team = '') => projectSessions(state,
    conversationMetadataFresh() ? conversationTargets : [], host, team).map(card => ({...card,
    chatUnavailable: !conversationMetadataFresh() ? 'Conversation metadata is unavailable or expired.' : card.chatUnavailable}));

  function focusReference(card, action = 'primary') {
    return {key: card.key, action};
  }

  function restoreCardFocus(reference) {
    if (!reference || document.hidden || dialog.open || conversationDialog.open) return;
    const card = sessions(host, $('conversation-team').value).find(card => card.key === reference.key);
    const node = card && cardNodes.get(card.key);
    const details = node?.querySelector('.card-details');
    const control = reference.action === 'details' && details && !details.hidden
      ? details : node?.querySelector('.agent-card');
    const target = control?.isConnected ? control : $('host-filter');
    if (document.activeElement !== target) target.focus();
  }

  function restoreDialogFocus(closed) {
    if (closed.open) return;
    const reference = dialogReturnTargets.get(closed);
    dialogReturnTargets.delete(closed);
    restoreCardFocus(reference);
  }

  function selectedCard() {
    return projectFleet(state).cards.find(card => card.key === selectedKey);
  }

  function clearOutput() {
    outputRequest?.abort();
    outputRequest = null;
    outputGate.invalidate();
    renderedOutput = null;
    setText($('output-text'), '');
    $('output-text').hidden = true;
    $('truncation').hidden = true;
    $('copy-output').disabled = true;
    setText($('copy-status'), '');
  }

  function unavailableReason(card) {
    if (!card) return 'This reference is no longer in the current fleet. Close this detail and select a current card.';
    if (card.endpoint.connection === 'unsupported') return 'This host does not support the observation protocol. Last-known metadata is shown; output is unavailable.';
    if (card.endpoint.connection === 'unavailable') return 'This host is unavailable. Last-known metadata is shown; output is disabled until the host returns with fresh observations.';
    if (!card.fresh) return 'Observation is stale or the host is not reachable. Output is disabled until a fresh snapshot arrives.';
    if (card.agent.evidence === 'coven_process_discovery') return 'Process evidence does not establish activity or terminal output capability. Output is unsupported for these cards.';
    return 'Output is unavailable. The operator may have disabled best-effort reads, or this agent does not support them.';
  }

  function updateDetail() {
    if (!dialog.open || !selectedKey) return;
    const card = selectedCard();
    const session = sessions().find(card => card.key === selectedKey);
    setText($('detail-chat-status'), session?.target ? 'Chat available. Open chat from the session card.'
      : `Chat unavailable: ${session?.chatUnavailable ?? 'This fleet reference was removed.'}`);
    if (card) {
      setText($('detail-title'), displaySessionTitle(card.agent));
      setText($('detail-meta'), `${card.evidenceLabel} / ${card.providerLabel} / ${card.agent.project || 'No project'} / ${card.endpoint.label} / ${card.age}${card.familyHints ? ` / ${card.familyHints}` : ''} / ${endpointCoverage(card.endpoint)}`);
    }
    if (!card?.outputEnabled) {
      clearOutput();
      setText($('output-status'), unavailableReason(card));
      $('read-output').hidden = true;
    } else {
      if (renderedOutput && Date.now() - renderedOutput.observed_at_ms >= state.snapshot.stale_after_ms) {
        clearOutput();
        setText($('output-status'), 'This output observation has aged out. Read again for a new bounded snapshot.');
      }
      $('read-output').hidden = false;
      $('read-output').disabled = outputRequest !== null;
    }
  }

  async function fetchJSON(path, signal) {
    const revision = authRevision;
    const response = await fetch(path, { method: 'GET', cache: 'no-store', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', headers: { Accept: 'application/json' }, signal });
    if (!response.ok) {
      if (response.status === 401 || (response.status === 403 && !path.startsWith('/api/v1/agents/'))) {
        invalidateAuthentication();
        throw new Error('auth');
      }
      // Error codes are never treated as upstream display text.
      const body = await response.json().catch(() => null);
      const code = responseError(response.status, body);
      if (code === 'auth') invalidateAuthentication();
      throw new Error(code);
    }
    const value = await response.json();
    // Even a previously successful response must not restore data after a known
    // authentication failure on another request (including a deferred body).
    if (revision !== authRevision) throw new Error('auth');
    return value;
  }

  const conversationDialog = $('conversation-dialog');
  const conversationGate = createOutputGate();
  let conversationTarget = null;
  let conversationRequest = null;
  let conversationTargets = [];
  let conversationListRevision = 0;
  let conversationListRequest = null;
  let conversationMetadataAt = null;
  const conversationLease = 15000;
  const conversationKey = target => target ? JSON.stringify([target.id, target.team]) : null;
  const readKey = target => target ? JSON.stringify([target.id, target.team,
    target.binding?.fleet_agent_id ?? null, target.binding?.fleet_endpoint_id ?? null,
    target.binding?.binding_id ?? null]) : null;
  const conversationMetadataFresh = () => conversationMetadataAt !== null
    && Date.now() - conversationMetadataAt < conversationLease;
  const currentConversation = () => sessions().some(card => readKey(card.target) === readKey(conversationTarget));
  let scrollRevision = 0;
  let interactionRevision = 0;
  let scrollIntent = null;
  const nearLatest = () => conversationDialog.scrollHeight - conversationDialog.clientHeight - conversationDialog.scrollTop <= 96;
  const stopFollowing = () => { interactionRevision++; scrollIntent = null; };
  for (const event of ['wheel', 'touchstart', 'pointerdown', 'keydown']) {
    conversationDialog.addEventListener(event, stopFollowing, {passive: true});
  }
  // Scrollbar dragging and assistive scrolling also cancel an outstanding intent.
  conversationDialog.addEventListener('scroll', () => {
    if (scrollIntent?.appliedTop !== undefined && conversationDialog.scrollTop !== scrollIntent.appliedTop) stopFollowing();
  });
  function positionConversation(current = () => true) {
    const intent = scrollIntent;
    if (!intent) return;
    const view = scrollRevision, interaction = intent.interaction;
    const key = readKey(conversationTarget), decision = decisionRevision;
    const apply = () => {
      if (intent !== scrollIntent || view !== scrollRevision || interaction !== interactionRevision
        || decision !== decisionRevision || key !== readKey(conversationTarget) || document.hidden
        || !conversationDialog.open || !conversationMetadataFresh() || !currentConversation() || !current()) return;
      if (intent.kind === 'request') {
        const heading = $('decision-details').firstElementChild;
        const anchor = heading ?? $('decision-status');
        const header = conversationDialog.querySelector('.dialog-heading').getBoundingClientRect();
        conversationDialog.scrollTop += anchor.getBoundingClientRect().top
          - header.bottom - 8;
      } else if (intent.kind === 'latest') conversationDialog.scrollTop = conversationDialog.scrollHeight;
      else conversationDialog.scrollTop = intent.top;
      intent.appliedTop = conversationDialog.scrollTop;
    };
    if (globalThis.requestAnimationFrame) requestAnimationFrame(apply);
  }
  let promptRevision = 0;
  let promptRequest = null;
  let pendingAttempt = null;
  // Metadata only. Inspection or a later send never resolves a lost receipt.
  const unresolvedAttempts = new Map();
  let promptStatus = { key: null, text: '' };
  const uncertainNotice = 'Delivery uncertain. Inspect conversation before deciding to send again. No automatic retry; closing this view does not cancel native work.';
  const capacityNotice = `Unresolved attempt capacity reached (${MAX_UNRESOLVED_PROMPTS}). New sends are disabled; no uncertain records have been discarded. Inspect the original conversations at the source.`;
  const targetAttempts = target => [...unresolvedAttempts.values()].filter(attempt =>
    attempt.id === target?.id && attempt.team === target?.team && attempt.phase === 'uncertain');
  const setPromptStatus = (text, key = conversationKey(conversationTarget)) => {
    promptStatus = { key, text };
    renderComposer();
  };

  function renderComposer() {
    const writable = conversationDialog.open && conversationMetadataFresh() && currentConversation() && conversationTarget?.can_submit === true;
    const busy = pendingAttempt !== null;
    const atCapacity = unresolvedAttempts.size >= MAX_UNRESOLVED_PROMPTS;
    const attempts = targetAttempts(conversationTarget);
    const uncertain = attempts.length > 0;
    const inspected = uncertain && attempts.every(attempt => attempt.inspected);
    const decision = inspected && $('prompt-new-decision').checked;
    const text = $('prompt-text').value;
    const bytes = new TextEncoder().encode(text).length;
    $('prompt-composer').hidden = !writable;
    $('prompt-text').disabled = busy || (uncertain && !decision);
    $('prompt-decision').hidden = !uncertain;
    $('prompt-new-decision').disabled = busy || !inspected;
    setText($('prompt-bytes'), `${bytes} / 65536 UTF-8 bytes`);
    $('send-prompt').disabled = !writable || busy || atCapacity || (uncertain && !decision)
      || !text.trim() || bytes > 65536 || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(text);
    setText($('prompt-status'), atCapacity ? capacityNotice : uncertain ? uncertainNotice
      : promptStatus.key === conversationKey(conversationTarget) ? promptStatus.text : '');
    setText($('prompt-attempt-status'), unresolvedAttempts.size
      ? `${unresolvedAttempts.size} unresolved submission receipt(s). ${uncertainNotice} Original references may expire; new references are not associated with these attempts.${atCapacity ? ` ${capacityNotice}` : ''}`
      : promptStatus.text);
  }

  function fencePrompt() {
    promptRevision++;
    promptRequest?.abort();
    promptRequest = null;
    $('prompt-text').value = '';
    $('prompt-new-decision').checked = false;
    if (pendingAttempt?.sent) {
      pendingAttempt.fenced = true;
      setPromptStatus(uncertainNotice);
    }
    renderComposer();
  }

  async function sendPrompt(event) {
    event.preventDefault();
    renderComposer();
    if ($('send-prompt').disabled || pendingAttempt) return;
    const target = conversationTarget;
    const revision = promptRevision;
    const current = () => revision === promptRevision && conversationDialog.open
      && conversationMetadataFresh() && conversationKey(target) === conversationKey(conversationTarget)
      && currentConversation() && conversationTarget?.can_submit === true;
    const active = { id: target.id, team: target.team, request_id: null,
      phase: 'pending', sent: false, fenced: false, inspected: false };
    pendingAttempt = active;
    const controller = new AbortController();
    promptRequest = controller;
    const timer = setTimeout(() => controller.abort(), 20000);
    renderComposer();
    setPromptStatus('Obtaining a fresh submission token. Not yet sent.');
    try {
      const token = await fetchJSON('/api/v1/csrf', controller.signal);
      if (!current() || controller.signal.aborted) throw new Error('unattempted');
      if (token?.contract !== 'coven.csrf.v1' || !string(token.token) || !/^[0-9]+-[a-f0-9]{64}-[a-f0-9]{64}$/.test(token.token)) throw new Error('unattempted');
      const requestID = crypto.randomUUID();
      if (unresolvedAttempts.has(requestID)) throw new Error('unattempted');
      const body = JSON.stringify({contract: 'coven.conversation-submit.v1', request_id: requestID, execute: true, text: $('prompt-text').value});
      // Once fetch is invoked, even a rejection/abort can hide a native write.
      active.sent = true;
      active.request_id = requestID;
      unresolvedAttempts.set(requestID, active);
      $('prompt-text').value = '';
      setPromptStatus('Submission pending. Native work is not canceled by closing this view.');
      const response = await fetch(`/api/v1/conversations/${target.id}/prompts`, {
        method: 'POST', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store',
        headers: {'Content-Type': 'application/json', Accept: 'application/json', 'X-Coven-CSRF': token.token},
        body, signal: controller.signal,
      });
      if (response.status === 401 || response.status === 403) invalidateAuthentication();
      const report = await response.json();
      if (!response.ok || report?.contract !== 'coven.conversation-submit.v1' || report.id !== target.id
        || report.request_id !== requestID) throw new Error('uncertain');
      if (!current() || active.fenced) {
        active.phase = 'uncertain';
        setPromptStatus('A receipt arrived for a closed or changed view. Inspect conversation before deciding to send again. Native work was not canceled.');
      } else if (report.outcome === 'accepted' && report.delivery === 'accepted') {
        active.phase = 'resolved';
        setPromptStatus('Accepted: persisted/scheduled, not completed. Refresh messages explicitly to inspect progress.');
      } else if (['unsent', 'rejected', 'not_attempted'].includes(report.delivery)) {
        active.phase = 'resolved';
        setPromptStatus('Not admitted. No automatic retry. Check current sharing policy before a new deliberate send.');
      } else throw new Error('uncertain');
    } catch {
      active.phase = active.sent ? 'uncertain' : 'resolved';
      setPromptStatus(active.sent ? uncertainNotice : 'Not sent: token acquisition or target validation failed.');
    } finally {
      clearTimeout(timer);
      if (active.phase === 'resolved') unresolvedAttempts.delete(active.request_id);
      if (pendingAttempt === active) pendingAttempt = null;
      if (promptRequest === controller) promptRequest = null;
      $('prompt-new-decision').checked = false;
      renderComposer();
    }
  }

  function clearConversation() {
    scrollRevision++;
    conversationRequest?.abort();
    conversationRequest = null;
    conversationGate.invalidate();
    $('conversation-messages').replaceChildren();
    setText($('conversation-status'), '');
    $('read-conversation').disabled = false;
    // Clearing private DOM can clamp scrollTop; that is not a user scroll.
    if (scrollIntent) scrollIntent.appliedTop = conversationDialog.scrollTop;
  }

  function closeConversation() {
    scrollIntent = null;
    fenceDecision();
    fencePrompt();
    clearConversation();
    conversationTarget = null;
    renderComposer();
  }

  function invalidateConversations(reason) {
    conversationListRequest?.abort();
    conversationListRequest = null;
    conversationListRevision++;
    conversationMetadataAt = null;
    conversationTargets = [];
    closeConversation();
    conversationDialog.close();
    setText($('conversation-title'), 'Conversation');
    const option = element('option', '', 'All authorized teams');
    option.value = '';
    $('conversation-team').replaceChildren(option);
    $('conversation-team').value = '';
    render();
    setText($('conversation-list-status'), reason);
  }

  function invalidateAuthentication() {
    attentionRequest?.abort(); attentionRequest = null; attention = null; attentionAt = null;
    localAlerts = 'unknown';
    renderAlerts('Background state unverified after access loss.');
    authRevision++;
    fleetRequest?.abort();
    clearOutput();
    selectedKey = null;
    dialog.close();
    setText($('detail-title'), 'Agent output');
    setText($('detail-meta'), '');
    invalidateConversations(errorMessage('auth'));
    state = failFleet(initialState(), 'auth');
    host = '';
    syncHosts();
    render();
  }

  async function readConversation(inspect = false) {
    const top = conversationDialog.scrollTop;
    const follow = nearLatest();
    const interaction = interactionRevision;
    if (inspect) scrollIntent = {kind: follow ? 'latest' : 'restore', top, interaction};
    clearConversation();
    const target = conversationTarget;
    const inspectedAttempts = inspect ? targetAttempts(target) : [];
    if (!conversationDialog.open || !target || !conversationMetadataFresh()
      || !currentConversation() || !conversationTargets.some(t => conversationKey(t) === conversationKey(target))) return;
    const ticket = conversationGate.begin(readKey(target));
    const controller = new AbortController();
    conversationRequest = controller;
    const timeout = setTimeout(() => controller.abort(), 10000);
    const current = () => conversationDialog.open && conversationMetadataFresh()
      && currentConversation() && conversationGate.accepts(ticket, readKey(conversationTarget));
    setText($('conversation-status'), 'Reading bounded native messages...');
    $('read-conversation').disabled = true;
    try {
      const path = `/api/v1/agents/${target.binding.fleet_agent_id}/conversations/${target.id}/bindings/${target.binding.binding_id}/messages`;
      const response = await fetchJSON(path, controller.signal);
      if (!current()) return;
      const value = validateConversation(response, target);
      for (const attempt of inspectedAttempts) {
        if (unresolvedAttempts.get(attempt.request_id) === attempt && attempt.phase === 'uncertain') attempt.inspected = true;
      }
      renderComposer();
      // Native history also contains tool/non-text steps and bounded omissions.
      // Hide only entries with no displayable text; never trim or reinterpret the
      // actual message, and do not invent text for content the reader omitted.
      const displayed = value.messages.filter(message => /[^\p{White_Space}\p{Default_Ignorable_Code_Point}\p{Cc}]/u.test(message.text));
      for (const message of displayed) {
        const article = element('article', 'conversation-message');
        article.append(element('h3', '', `${message.role === 'user' ? 'User' : 'Assistant'}${message.truncated ? ' / truncated' : ''}`), element('pre', '', message.text));
        $('conversation-messages').append(article);
      }
      if (!displayed.length) {
        $('conversation-messages').append(element('p', 'muted small', 'No text messages to display.'));
      }
      const partial = value.truncated || value.omitted_parts || value.messages.some(m => m.truncated);
      setText($('conversation-status'), [value.has_older ? 'Earlier messages not shown.' : '', partial ? 'Some content omitted.' : ''].filter(Boolean).join(' '));
      positionConversation(current);
    } catch (error) {
      if (!current()) return;
      $('conversation-messages').replaceChildren();
      setText($('conversation-status'), error.message === 'auth' ? errorMessage('auth')
        : 'Conversation refused, unavailable or invalid. Native setup and the configured runtime/session must still match. No old text is retained.');
    } finally {
      clearTimeout(timeout);
      if (conversationRequest === controller) { conversationRequest = null; $('read-conversation').disabled = false; }
    }
  }

  function openConversation(card, exactRequest = null) {
    const target = card.target;
    if (!target?.binding || !conversationMetadataFresh()) return false;
    fenceDecision();
    clearConversation();
    fencePrompt();
    conversationTarget = target;
    dialogReturnTargets.set(conversationDialog, focusReference(card));
    setText($('conversation-title'), `${displaySessionTitle(card.agent)} / ${target.team}`);
    scrollIntent = {kind: 'latest', interaction: interactionRevision};
    if (!conversationDialog.open) conversationDialog.showModal();
    renderComposer();
    renderRequests();
    const requests = currentRequests();
    if (exactRequest?.details_available) void openDecision(exactRequest);
    else if (!exactRequest && requests.length === 1 && requests[0].details_available) void openDecision(requests[0]);
    void readConversation();
    return true;
  }

  async function loadConversations() {
    if (document.hidden || conversationListRequest) return;
    const revision = ++conversationListRevision;
    const controller = new AbortController();
    conversationListRequest = controller;
    const timeout = setTimeout(() => controller.abort(), 10000);
    try {
      const value = await fetchJSON('/api/v1/conversations', controller.signal);
      if (revision !== conversationListRevision) return;
      if (!conversationMetadataFresh()) {
        closeConversation();
        conversationDialog.close();
      }
      conversationTargets = validateConversations(value);
      conversationMetadataAt = Date.now();
      const updatedTarget = conversationTargets.find(t => conversationKey(t) === conversationKey(conversationTarget));
      if (conversationTarget && updatedTarget && updatedTarget.can_submit !== conversationTarget.can_submit) {
        fencePrompt();
        clearConversation();
      }
      if (conversationTarget && !conversationTargets.some(t => conversationKey(t) === conversationKey(conversationTarget))) {
        closeConversation();
        conversationDialog.close();
        setText($('conversation-title'), 'Conversation');
      }
      // render() reapplies the verified display binding separately from native identity.
      renderComposer();
      const selectedTeam = $('conversation-team').value;
      const options = [element('option', '', 'All authorized teams')];
      options[0].value = '';
      for (const team of [...new Set(conversationTargets.map(t => t.team))].sort()) {
        const option = element('option', '', team); option.value = team; options.push(option);
      }
      $('conversation-team').replaceChildren(...options);
      $('conversation-team').value = conversationTargets.some(t => t.team === selectedTeam) ? selectedTeam : '';
      render();
      setText($('conversation-list-status'), [value.sources?.some(source => source.status !== 'observed')
        ? 'Some chats unavailable. Check the native source.' : '', value.omitted_conversations
        ? 'Some sessions are not shown. Narrow your selection at the source.' : ''].filter(Boolean).join(' '));
    } catch (error) {
      if (revision !== conversationListRevision) return;
      invalidateConversations(error.message === 'auth' ? errorMessage('auth')
        : 'Authorized conversation list unavailable. Check access and gateway setup.');
    } finally {
      clearTimeout(timeout);
      if (conversationListRequest === controller) conversationListRequest = null;
    }
  }

  function matchesTarget(item, target) {
    return !!target?.binding && item.conversation_id === target.id && item.team === target.team
      && item.binding_id === target.binding.binding_id && item.fleet_agent_id === target.binding.fleet_agent_id
      && item.fleet_endpoint_id === target.binding.fleet_endpoint_id;
  }
  function currentRequests() {
    return attentionFresh() && currentConversation() ? attention.items.filter(item => matchesTarget(item, conversationTarget)) : [];
  }
  function fenceDecision() {
    decisionRevision++;
    decisionRequest?.abort(); decisionRequest = null;
    decisionItem = null; decisionDetail = null;
    $('decision-details').replaceChildren();
    $('decision-requests').replaceChildren();
    requestListKey = null;
    setText($('decision-status'), '');
  }
  function renderAttention() {
    setText($('attention-status'), alertUnverified ? 'Cannot currently verify this alert. No request opened.' : attentionLabel(attentionFresh() ? attention : null));
    setText($('decision-attempt-status'), uncertainDecisions.size
      ? `${uncertainDecisions.size} unresolved response receipt(s). ${decisionNotice}` : '');
    if (decisionItem && !currentRequests().some(item => item.id === decisionItem.id
      && item.alert_id === decisionItem.alert_id && item.can_respond === decisionItem.can_respond
      && item.details_available)) { fenceDecision(); renderRequests(); }
    const hash = alertHash();
    if (hash && hash !== alertNavigation && attentionFresh() && conversationMetadataFresh() && state.snapshot && !state.failed) {
      const item = attention.items.find(item => item.alert_id === hash);
      const card = item && sessions().find(card => matchesTarget(item, card.target));
      if (card && item.details_available && openConversation(card, item)) { alertNavigation = hash; alertUnverified = false; }
      else { alertUnverified = true; setText($('attention-status'), 'Cannot currently verify this alert. No request opened.'); }
    }
  }
  function renderRequests() {
    const items = currentRequests();
    const key = JSON.stringify([conversationDialog.open, readKey(conversationTarget), items]);
    if (key === requestListKey) return;
    requestListKey = key;
    $('decision-requests').replaceChildren();
    if (!conversationDialog.open) return;
    for (const item of items) {
      const button = element('button', 'quiet-button', item.details_available
        ? `Open ${item.kind}${item.can_respond ? '' : ' / read-only'}` : `${item.kind} / use source UI`);
      button.type = 'button'; button.disabled = !item.details_available;
      button.addEventListener('click', () => void openDecision(item));
      $('decision-requests').append(button);
    }
    if (!items.length && !decisionItem) setText($('decision-status'), attentionFresh() ? '' : 'Requests unavailable. Refresh to check.');
  }
  async function loadAttention() {
    if (document.hidden || attentionRequest) return;
    clearTimeout(attentionTimer);
    const controller = new AbortController(); attentionRequest = controller;
    const timer = setTimeout(() => controller.abort(), 10000);
    try {
      const value = validateAttention(await fetchJSON('/api/v1/attention', controller.signal));
      if (controller.signal.aborted || document.hidden || attentionRequest !== controller) return;
      attention = value; attentionAt = Date.now();
    } catch {
      if (attentionRequest !== controller) return;
      attention = null; attentionAt = null; fenceDecision();
    } finally {
      clearTimeout(timer);
      if (attentionRequest === controller) {
        attentionRequest = null; render(); renderRequests();
        if (!document.hidden) attentionTimer = setTimeout(loadAttention, 3000);
      }
    }
  }
  async function openDecision(item) {
    fenceDecision(); decisionItem = item;
    scrollIntent = {kind: 'request', interaction: interactionRevision, appliedTop: conversationDialog.scrollTop};
    const revision = decisionRevision;
    const controller = new AbortController(); decisionRequest = controller;
    const timer = setTimeout(() => controller.abort(), 10000);
    const current = () => revision === decisionRevision && conversationDialog.open && currentRequests().some(row => row.id === item.id
      && row.can_respond === item.can_respond && row.details_available);
    setText($('decision-status'), 'Reading exact request...');
    try {
      const detail = validateDecision(await fetchJSON(`/api/v1/decisions/${item.id}`, controller.signal), item);
      if (!current() || controller.signal.aborted) return;
      decisionDetail = detail;
      renderDecision(item, detail);
      positionConversation(current);
    } catch {
      if (!current()) return;
      $('decision-details').replaceChildren();
      setText($('decision-status'), 'Request changed, unsupported or unavailable. Use source UI.');
    } finally { clearTimeout(timer); if (decisionRequest === controller) decisionRequest = null; }
  }
  function renderDecision(item, detail) {
    const root = $('decision-details'); root.replaceChildren();
    const d = detail.decision;
    const writable = item.can_respond && detail.can_respond;
    setText($('decision-status'), writable ? 'Respond to this exact request.' : 'Read-only request. Respond at source.');
    const fields = [];
    if (d.kind === 'question') {
      d.questions.forEach((q, index) => {
        const group = element('fieldset'); group.append(element('legend', '', q.header), element('pre', '', q.question));
        const row = {options: [], custom: null};
        q.options.forEach((option, n) => {
          const label = element('label', 'decision-option');
          const input = element('input'); input.type = q.multiple ? 'checkbox' : 'radio';
          input.name = `question-${index}`; input.id = `decision-${index}-${n}`;
          input.value = option.label; input.disabled = !writable;
          label.append(input, element('span', '', option.label), element('span', 'muted small', option.description));
          group.append(label); row.options.push(input);
        });
        if (q.custom) {
          const label = element('label', '', q.multiple ? 'Custom answer (optional)' : 'Custom answer (instead of selection)');
          const input = element('textarea'); input.rows = 2; input.disabled = !writable; input.autocomplete = 'off';
          input.addEventListener('input', () => { if (!q.multiple && input.value) row.options.forEach(o => { o.checked = false; }); });
          row.options.forEach(o => o.addEventListener('change', () => { if (!q.multiple && o.checked) input.value = ''; }));
          label.append(input); group.append(label); row.custom = input;
        }
        root.append(group); fields.push(row);
      });
    } else {
      root.append(element('h3', '', d.permission), element('h4', '', 'Patterns'), element('pre', '', d.patterns.join('\n')),
        element('h4', '', 'Native rule patterns (no remembered approval)'), element('pre', '', d.always_patterns.join('\n')),
        element('pre', '', d.metadata_json), element('p', 'warning', 'Deny rejects all currently pending permissions in this session, not future requests. Allow once remembers no rule.'));
    }
    if (!writable) return;
    const prior = [...uncertainDecisions.values()].filter(a => a.alert_id === item.alert_id);
    let deliberate = null;
    if (prior.length) {
      const label = element('label', '', 'I inspected this exact request and separately choose a new response. This is not a retry.');
      deliberate = element('input'); deliberate.type = 'checkbox'; label.append(deliberate); root.append(label);
      setText($('decision-status'), decisionNotice);
    }
    const buttons = [];
    const add = (label, action) => {
      const button = element('button', 'quiet-button', label); button.type = 'button';
      button.disabled = decisionBusy || uncertainDecisions.size >= 32 || !!deliberate;
      button.addEventListener('click', () => {
        if (button.disabled || decisionBusy || (deliberate && !deliberate.checked)) return;
        let value;
        try { value = action(); } catch { setText($('decision-status'), 'Choose valid answers for every question.'); return; }
        void respondDecision(item, detail, value);
      });
      root.append(button); buttons.push(button);
    };
    deliberate?.addEventListener('change', () => buttons.forEach(b => { b.disabled = !deliberate.checked || decisionBusy || uncertainDecisions.size >= 32; }));
    if (d.kind === 'question') {
      add('Answer once', () => ({type: 'question_answer', answers: questionAnswers(d.questions, fields)}));
      add('Reject question', () => ({type: 'question_reject'}));
    } else {
      add('Allow once', () => ({type: 'permission_once'}));
      add('Deny pending session permissions', () => ({type: 'permission_deny', scope: 'session_pending'}));
    }
  }
  async function respondDecision(item, detail, action) {
    const revision = decisionRevision;
    const current = () => revision === decisionRevision && decisionDetail === detail && conversationDialog.open
      && currentRequests().some(row => row.id === item.id && row.can_respond && row.details_available);
    if (!current() || decisionBusy || uncertainDecisions.size >= 32) return;
    const controller = new AbortController(); decisionRequest = controller; decisionBusy = true;
    const timer = setTimeout(() => controller.abort(), 20000);
    let attempt = null;
    // Clear all answer drafts before any asynchronous boundary.
    $('decision-details').replaceChildren();
    setText($('decision-status'), 'Obtaining a fresh response token.');
    try {
      const token = await fetchJSON('/api/v1/csrf', controller.signal);
      if (!current() || controller.signal.aborted || token?.contract !== 'coven.csrf.v1'
        || !string(token.token) || !/^[0-9]+-[a-f0-9]{64}-[a-f0-9]{64}$/.test(token.token)) throw new Error('unsent');
      const id = crypto.randomUUID();
      if (uncertainDecisions.has(id)) throw new Error('unsent');
      attempt = {id: item.id, alert_id: item.alert_id, attempt_id: id, action: action.type};
      uncertainDecisions.set(id, attempt);
      const body = JSON.stringify({contract: 'coven.decision-response.v1', attempt_id: id, execute: true, action});
      action = null;
      const response = await fetch(`/api/v1/decisions/${item.id}/responses`, {
        method: 'POST', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store', signal: controller.signal,
        headers: {'Content-Type': 'application/json', Accept: 'application/json', 'X-Coven-CSRF': token.token}, body,
      });
      if (response.status === 401 || response.status === 403) invalidateAuthentication();
      const report = await response.json();
      if (!current() || !response.ok || report?.contract !== 'coven.decision-response.v1'
        || report.id !== item.id || report.attempt_id !== id || report.action !== attempt.action) throw new Error('uncertain');
      if (report.outcome === 'applied' && report.delivery === 'applied') {
        uncertainDecisions.delete(id);
        setText($('decision-status'), 'Applied: native wait settled, not tool completion.');
      } else if ((report.outcome === 'rejected' && report.delivery === 'rejected')
        || (report.outcome === 'unavailable' && report.delivery === 'unsent')) {
        uncertainDecisions.delete(id); setText($('decision-status'), 'Not applied. Inspect current request before another choice.');
      } else throw new Error('uncertain');
    } catch {
      if (current()) setText($('decision-status'), attempt ? decisionNotice : 'Not sent. Inspect request again.');
    } finally {
      action = null; clearTimeout(timer); decisionBusy = false;
      if (decisionRequest === controller) decisionRequest = null;
      renderAttention(); renderRequests();
      if (revision !== decisionRevision && decisionItem && decisionDetail) renderDecision(decisionItem, decisionDetail);
    }
  }

  function alertWait(promise, signal) {
    return new Promise((resolve, reject) => {
      const abort = () => reject(new Error('alert_timeout'));
      if (signal.aborted) { Promise.resolve(promise).catch(() => {}); abort(); return; }
      signal.addEventListener('abort', abort, {once: true});
      Promise.resolve(promise).then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
    });
  }
  function pushConfig(value) {
    if (value?.contract !== 'coven.web-push.v1' || typeof value.enabled !== 'boolean'
      || !Array.isArray(value.subscriptions) || value.subscriptions.length > 8
      || !value.subscriptions.every(id => string(id) && AGENT_ID.test(id))) throw new Error('config');
    return value;
  }
  function deliveryLabel(config) {
    if (config?.delivery_status === undefined || ['idle', 'sent'].includes(config.delivery_status)) return '';
    if (['unavailable', 'queue_limit', 'history_or_state_limit', 'crypto_failed', 'subscription_expired', 'retrying', 'delivery_failed', 'state_unavailable'].includes(config?.delivery_status)) return 'Alert delivery degraded. Check the gateway.';
    return 'Alert delivery unverified. Check the gateway.';
  }
  function renderAlerts(message = $('alerts-status').textContent) {
    const busy = alertsBusy || alertsChecking;
    setText($('alerts-toggle'), busy ? 'Alerts checking' : `Alerts ${localAlerts}`);
    $('alerts-toggle').setAttribute('aria-pressed', localAlerts === 'unknown' ? 'mixed' : String(localAlerts === 'on'));
    $('alerts-toggle').title = busy ? 'Checking alert settings'
      : localAlerts === 'off' ? 'Enable alerts'
      : localAlerts === 'on' ? 'Disable alerts' : 'Alert state unverified; disable browser alerts';
    $('alerts-toggle').setAttribute('aria-busy', String(busy));
    // Inspection is passive and may be superseded by an explicit user choice.
    $('alerts-toggle').disabled = alertsBusy;
    $('alerts-cleanup').disabled = alertsBusy;
    $('alerts-cleanup').hidden = !pendingPushRemovals.size && !unresolvedPushCreations.size && !unidentifiedPushRemoval;
    setText($('alerts-status'), message);
    $('alerts-error').hidden = !message && $('alerts-cleanup').hidden;
  }
  async function pushMutation(path, data, revision, operation, signal, creation = null) {
    if (signal.aborted || revision !== authRevision || operation !== pushOperationRevision) throw new Error('auth');
    const token = await alertWait(fetchJSON('/api/v1/csrf', signal), signal);
    if (signal.aborted || revision !== authRevision || operation !== pushOperationRevision || token?.contract !== 'coven.csrf.v1' || !string(token.token) || !/^[0-9]+-[a-f0-9]{64}-[a-f0-9]{64}$/.test(token.token)) throw new Error('csrf');
    // A lost create receipt may hide a blocking commit even after a later delete.
    if (creation) unresolvedPushCreations.add(creation);
    const response = await alertWait(fetch(path, {method: 'POST', cache: 'no-store', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', signal,
      headers: {'Content-Type': 'application/json', Accept: 'application/json', 'X-Coven-CSRF': token.token},
      body: JSON.stringify({contract: 'coven.web-push.v1', execute: true, ...data})}), signal);
    if (response.status === 401 || response.status === 403) invalidateAuthentication();
    const value = await alertWait(response.json(), signal);
    if (signal.aborted || revision !== authRevision || operation !== pushOperationRevision || !response.ok || value?.contract !== 'coven.web-push.v1' || !AGENT_ID.test(value.subscription_id)
      || value.subscribed !== (path === '/api/v1/push/subscriptions')) throw new Error('push_unavailable');
    if (creation) {
      if (value.subscription_id !== creation.id) throw new Error('receipt');
      unresolvedPushCreations.delete(creation);
    }
    return value;
  }
  async function toggleAlerts(enable, cleanupOnly = false) {
    if (alertsBusy) return;
    const operation = ++pushOperationRevision;
    if (!globalThis.navigator?.serviceWorker || (enable && (!globalThis.PushManager || !globalThis.Notification))) {
      renderAlerts('Background alerts unsupported. In-page attention still works.'); return;
    }
    const controller = new AbortController(), signal = controller.signal;
    const timer = setTimeout(() => controller.abort(), 10000);
    alertsBusy = true; alertsChecking = false; renderAlerts('');
    const revision = authRevision;
    try {
      if (!enable) {
        let localDisabled = false;
        let unidentifiedSubscription = false;
        if (!cleanupOnly) try {
          const registration = await alertWait(navigator.serviceWorker.getRegistration('/'), signal);
          const sub = registration && await alertWait(registration.pushManager.getSubscription(), signal);
          if (sub) {
            // Local removal must not depend on any server or CSRF request succeeding.
            unidentifiedSubscription = true;
            const id = subscriptionID(sub.endpoint).catch(() => null);
            const removal = Promise.resolve().then(() => sub.unsubscribe()).catch(() => false);
            // Hash completion is independent of a stalled/canceled local removal.
            const value = await alertWait(id, signal);
            if (value) { pendingPushRemovals.add(value); unidentifiedSubscription = false; }
            localDisabled = await alertWait(removal, signal) === true;
          } else localDisabled = true;
        } catch { /* Keep local state unverified; server cleanup is still deliberate. */ }
        unidentifiedPushRemoval ||= unidentifiedSubscription;
        if (!cleanupOnly) localAlerts = localDisabled ? 'off' : 'unknown';
        renderAlerts('');
        let serverVerified = false;
        try {
          if (signal.aborted || revision !== authRevision || operation !== pushOperationRevision) throw new Error('auth');
          const config = pushConfig(await alertWait(fetchJSON('/api/v1/push/config', signal), signal));
          const creations = new Set([...unresolvedPushCreations].map(creation => creation.id));
          for (const id of new Set([...pendingPushRemovals, ...creations])) {
            if (config.subscriptions.includes(id) || creations.has(id)) {
              const result = await pushMutation('/api/v1/push/unsubscribe', {subscription_id: id}, revision, operation, signal);
              if (result.subscription_id !== id) throw new Error('receipt');
            }
            pendingPushRemovals.delete(id);
          }
          serverVerified = revision === authRevision && operation === pushOperationRevision
            && !unidentifiedPushRemoval && unresolvedPushCreations.size === 0;
        } catch { /* No automatic mutation retries, including after lost receipts. */ }
        const message = cleanupOnly ? 'Server removal unverified. Check the gateway or retry cleanup.'
          : localDisabled ? 'Browser alerts disabled; server removal unverified'
          : 'Browser removal unverified. Check browser settings.';
        renderAlerts(serverVerified && (cleanupOnly || localDisabled) ? '' : message);
        return;
      }
      if (pendingPushRemovals.size >= 32 || unresolvedPushCreations.size >= 32) throw new Error('cleanup_capacity');
      // Called only from a deliberate click, before awaiting network I/O.
      if (await alertWait(Notification.requestPermission(), signal) !== 'granted') {
        renderAlerts('Background permission denied. Check browser settings.'); return;
      }
      const config = pushConfig(await alertWait(fetchJSON('/api/v1/push/config', signal), signal));
      if (!config.enabled || !string(config.public_key) || !/^[A-Za-z0-9_-]{87}$/.test(config.public_key)) {
        renderAlerts('Background alerts not configured. Ask the operator to enable push.'); return;
      }
      await alertWait(navigator.serviceWorker.register('/service-worker.js', {scope: '/'}), signal);
      const registration = await alertWait(navigator.serviceWorker.ready, signal);
      if (signal.aborted || revision !== authRevision) throw new Error('auth');
      let sub = await alertWait(registration.pushManager.getSubscription(), signal);
      localAlerts = 'unknown';
      if (sub && !config.subscriptions.includes(await alertWait(subscriptionID(sub.endpoint), signal))) {
        // This explicit re-enable must create a new endpoint, never transfer ownership.
        if (!await alertWait(sub.unsubscribe(), signal)) throw new Error('unsubscribe'); sub = null;
      }
      if (!sub) {
        const key = Uint8Array.from(atob(config.public_key.replace(/-/g, '+').replace(/_/g, '/') + '='), c => c.charCodeAt(0));
        sub = await alertWait(registration.pushManager.subscribe({userVisibleOnly: true, applicationServerKey: key}), signal);
      }
      if (signal.aborted || revision !== authRevision) throw new Error('auth');
      const serialized = sub.toJSON();
      const id = await alertWait(subscriptionID(sub.endpoint), signal);
      pendingPushRemovals.add(id);
      const result = await pushMutation('/api/v1/push/subscriptions', {subscription: {endpoint: sub.endpoint, keys: {p256dh: serialized.keys?.p256dh, auth: serialized.keys?.auth}}}, revision, operation, signal, {id});
      if (revision !== authRevision || result.subscription_id !== id) throw new Error('auth');
      pendingPushRemovals.delete(id);
      localAlerts = 'on';
      renderAlerts([deliveryLabel(config), unresolvedPushCreations.size ? 'Earlier registration unverified. Retry cleanup.' : ''].filter(Boolean).join('; '));
    } catch {
      localAlerts = 'unknown';
      renderAlerts('Background state unverified. Check browser settings; in-page attention still works.');
    } finally { controller.abort(); clearTimeout(timer); alertsBusy = false; renderAlerts(); }
  }
  $('alerts-toggle').addEventListener('click', () => void toggleAlerts(localAlerts === 'off'));
  $('alerts-cleanup').addEventListener('click', () => void toggleAlerts(false, true));
  async function inspectAlerts() {
    if (!globalThis.navigator?.serviceWorker || !globalThis.PushManager || globalThis.Notification?.permission !== 'granted') return;
    const revision = authRevision;
    const operation = pushOperationRevision;
    const controller = new AbortController(), signal = controller.signal;
    const timer = setTimeout(() => controller.abort(), 10000);
    alertsChecking = true; renderAlerts();
    try {
      const registration = await alertWait(navigator.serviceWorker.getRegistration('/'), signal);
      const sub = registration && await alertWait(registration.pushManager.getSubscription(), signal);
      if (alertsBusy || revision !== authRevision || operation !== pushOperationRevision) return;
      if (!sub) { localAlerts = 'off'; renderAlerts(''); return; }
      localAlerts = 'unknown';
      const config = pushConfig(await alertWait(fetchJSON('/api/v1/push/config', signal), signal));
      const id = await alertWait(subscriptionID(sub.endpoint), signal);
      if (alertsBusy || revision !== authRevision || operation !== pushOperationRevision) return;
      if (config?.contract === 'coven.web-push.v1' && config.enabled === true
        && Array.isArray(config.subscriptions) && config.subscriptions.includes(id)) {
        localAlerts = 'on';
        renderAlerts(deliveryLabel(config));
      } else {
        localAlerts = 'off';
        renderAlerts('Browser subscription not verified for this account. Turn alerts on to replace it.');
      }
    } catch { if (!alertsBusy && revision === authRevision && operation === pushOperationRevision) { localAlerts = 'unknown'; renderAlerts('Background state unverified. Check browser settings.'); } }
    finally { controller.abort(); clearTimeout(timer); if (operation === pushOperationRevision) { alertsChecking = false; renderAlerts(); } }
  }
  renderAlerts('');
  void inspectAlerts();
  globalThis.addEventListener?.('hashchange', () => { alertNavigation = null; alertUnverified = false; attentionAt = null; fenceDecision(); void loadAttention(); });

  $('read-conversation').addEventListener('click', () => void readConversation(true));
  $('prompt-composer').addEventListener('submit', event => void sendPrompt(event));
  $('prompt-text').addEventListener('input', renderComposer);
  $('prompt-new-decision').addEventListener('change', renderComposer);
  $('close-conversation').addEventListener('click', () => { closeConversation(); conversationDialog.close(); });
  conversationDialog.addEventListener('cancel', closeConversation);
  conversationDialog.addEventListener('close', () => {
    if (!conversationDialog.open) closeConversation();
    restoreDialogFocus(conversationDialog);
  });
  $('conversation-team').addEventListener('change', () => { closeConversation(); conversationDialog.close(); render(); });

  async function readOutput() {
    clearOutput();
    const card = selectedCard();
    if (!dialog.open || !card?.outputEnabled) { updateDetail(); return; }
    const ticket = outputGate.begin(card.key);
    const controller = new AbortController();
    outputRequest = controller;
    const timeout = setTimeout(() => controller.abort(), 10000);
    setText($('output-status'), 'Reading a bounded text snapshot...');
    $('read-output').disabled = true;
    try {
      const value = await fetchJSON(`/api/v1/agents/${card.agent.id}/output`, controller.signal);
      if (!dialog.open || !outputGate.accepts(ticket, selectedKey) || !selectedCard()?.outputEnabled) return;
      const output = validateOutput(value, card.agent.id, Date.now(), state.snapshot.stale_after_ms);
      renderedOutput = output;
      setText($('output-text'), output.text);
      $('output-text').hidden = false;
      $('truncation').hidden = !output.truncated;
      $('copy-output').disabled = !navigator.clipboard?.writeText;
      setText($('output-status'), output.text.length ? `Text snapshot / ${ageLabel(output.observed_at_ms, Date.now()).toLowerCase()}` : 'The output snapshot is empty.');
    } catch (error) {
      if (!dialog.open || !outputGate.accepts(ticket, selectedKey)) return;
      if (error.message === 'auth') {
        state = failFleet(state, 'auth');
        render();
        setText($('output-status'), errorMessage('auth'));
      } else {
        setText($('output-status'), error.message === 'output_disabled'
          ? 'Best-effort output is disabled by the operator.'
          : error.message === 'output_unsupported' ? 'Output is unsupported for this observation. Process evidence does not establish terminal output capability.'
          : 'Output could not be read or validated. The reference may have changed; wait for a fresh fleet snapshot and try again.');
      }
    } finally {
      clearTimeout(timeout);
      if (outputRequest === controller) { outputRequest = null; updateDetail(); }
    }
  }

  function openDetail(key) {
    clearOutput();
    selectedKey = key;
    const card = selectedCard();
    if (!card) return;
    const session = sessions(host, $('conversation-team').value).find(card => card.key === key);
    if (session) dialogReturnTargets.set(dialog, focusReference(session, 'details'));
    setText($('detail-title'), displaySessionTitle(card.agent));
    setText($('output-status'), unavailableReason(card));
    if (!dialog.open) dialog.showModal();
    updateDetail();
    if (card.outputEnabled) void readOutput();
  }

  function render() {
    const now = Date.now();
    const team = $('conversation-team').value;
    const view = projectFleet(state, host, now);
    const cards = sessions(host, team);
    const focusedNode = [...cardNodes.values()].find(node => node.contains(document.activeElement));
    const focusedReference = focusedNode ? {...focusedNode.focusReference,
      action: focusedNode.querySelector('.card-details') === document.activeElement ? 'details' : 'primary'} : null;
    if (conversationTarget) {
      const updated = cards.find(card => card.key === dialogReturnTargets.get(conversationDialog)?.key
        && conversationKey(card.target) === conversationKey(conversationTarget))?.target;
      if (!updated || readKey(updated) !== readKey(conversationTarget)) {
        fenceDecision();
        fencePrompt();
        clearConversation();
        if (!updated) { closeConversation(); conversationDialog.close(); }
      }
      if (updated) conversationTarget = updated;
    }
    renderComposer();
    renderAttention();
    const snapshot = state.snapshot;
    const snapshotOld = snapshot && (snapshot.generated_at_ms > now + 5000 || now - snapshot.generated_at_ms >= snapshot.stale_after_ms);
    const stale = state.failed || snapshotOld;
    const connectivity = !snapshot ? (state.failed ? 'Observatory unavailable' : 'Connecting to the observatory...')
      : stale ? 'Last-known fleet / stale' : 'Connected to the observatory';
    setText($('connection-status'), document.hidden ? `${connectivity} / polling paused` : connectivity);
    $('connection-dot').dataset.state = snapshot && !stale ? 'fresh' : 'stale';
    setText($('poll-status'), `${snapshot ? `Fleet ${ageLabel(snapshot.generated_at_ms, now).toLowerCase()}. ` : ''}${document.hidden ? 'Polling paused while hidden.' : `Polling every ${Math.max(1, Math.round(pollDelay() / 1000))}s${failures ? ' with failure backoff' : ''}.`}`);
    const emptyHostErrors = view.endpoints.filter(endpoint => endpoint.connection !== 'reachable' && !endpoint.agents.length)
      .map(endpoint => `${endpoint.label || endpoint.id} ${endpoint.connection}.`).join(' ');
    const notice = state.failed ? errorMessage(state.error) : emptyHostErrors;
    setText($('notice'), notice);
    $('notice').hidden = !notice;
    $('refresh').disabled = fleetRequest !== null || document.hidden;
    // Reconcile by opaque identity instead of replacing focused cards every second.
    const keys = new Set(cards.map(card => card.key));
    for (const [key, node] of cardNodes) if (!keys.has(key)) { node.remove(); cardNodes.delete(key); }
    let position = 0;
    for (const card of cards) {
      let node = cardNodes.get(card.key);
      if (!node) {
        node = element('article', 'session-card');
        const primary = element('button', 'agent-card');
        primary.type = 'button';
        primary.setAttribute('aria-haspopup', 'dialog');
        primary.addEventListener('click', () => {
          const current = sessions(host, $('conversation-team').value).find(c => c.key === card.key);
          if (current?.target && !document.hidden) openConversation(current);
        });
        const top = element('span', 'card-top');
        top.append(element('span', 'host-badge'), element('span', 'activity'));
        const body = element('span');
        body.append(element('span', 'card-title'), element('span', 'card-project'), element('span', 'card-project card-evidence'));
        const bottom = element('span', 'card-bottom');
        bottom.append(element('span', 'card-age'), element('span', 'attention-badge', 'Needs response'), element('span', 'card-action'));
        primary.append(top, body, bottom);
        const details = element('button', 'quiet-button card-details', 'Details / terminal output');
        details.type = 'button';
        details.addEventListener('click', () => openDetail(card.key));
        node.append(primary, details);
        cardNodes.set(card.key, node);
      }
      node.focusReference = focusReference(card);
      node.dataset.stale = String(!card.fresh);
      setText(node.querySelector('.host-badge'), card.endpoint.label || card.endpoint.id);
      setText(node.querySelector('.activity'), card.fresh ? card.activityLabel : `Stale / ${card.activityLabel}`);
      setText(node.querySelector('.card-title'), displaySessionTitle(card.agent));
      setText(node.querySelector('.card-project'), `${card.providerLabel} / ${card.agent.project || 'No project'} / ${card.target ? `Chat team: ${card.target.team}` : 'Team unassigned'}`);
      setText(node.querySelector('.card-evidence'), `${card.evidenceLabel}${card.familyHints ? ` / ${card.familyHints}` : ''} / ${card.target
        ? 'Chat available'
        : `Chat unavailable: ${card.chatUnavailable}`}`);
      setText(node.querySelector('.card-age'), card.age);
      const needsResponse = attentionFresh() && attention.items.some(item => matchesTarget(item, card.target));
      node.querySelector('.attention-badge').hidden = !needsResponse;
      setText(node.querySelector('.card-action'), card.target ? 'Open conversation +' : 'Chat unavailable');
      node.querySelector('.agent-card').setAttribute('aria-disabled', String(!card.target));
      node.querySelector('.card-details').hidden = false;
      const current = $('agent-grid').children[position++];
      if (current !== node) $('agent-grid').insertBefore(node, current ?? null);
    }
    if (focusedReference) restoreCardFocus(focusedReference);
    setText($('agent-count'), `${cards.length} session card${cards.length === 1 ? '' : 's'}${stale ? ' / fleet last known' : ''}`);
    $('empty-state').hidden = cards.length !== 0;
    const nativeCoverage = !conversationMetadataFresh()
      ? 'Native chat metadata is unavailable or awaiting revalidation; absence is not confirmed.'
      : !conversationTargets.length ? 'No authorized native chat sessions were reported.'
      : 'Shared sessions without verified fleet links are diagnostic metadata only, not additional monitoring cards.';
    setText($('empty-title'), host || team ? 'No matching sessions'
      : conversationMetadataFresh() && snapshot && !stale ? 'No sessions reported' : 'Session coverage incomplete');
    setText($('empty-description'), host || team
      ? `No session cards match the selected Host/Team filters.${team ? ' Team filters exclude unassigned terminal-only observations; only verified chat team metadata is included.' : ''} ${nativeCoverage} Other hosts or teams may still have sessions. Source health and coverage are reported separately.`
      : `${view.emptyDescription} ${nativeCoverage}`);
    updateDetail();
  }

  function syncHosts() {
    const endpoints = state.snapshot?.endpoints ?? [];
    if (host && !endpoints.some(endpoint => endpoint.id === host)) host = '';
    const options = [element('option', '', 'All hosts')];
    options[0].value = '';
    for (const endpoint of endpoints) {
      const option = element('option', '', `${endpoint.label || endpoint.id} (${endpoint.id})`);
      option.value = endpoint.id;
      options.push(option);
    }
    $('host-filter').replaceChildren(...options);
    $('host-filter').value = host;
  }

  function pollDelay() {
    return Math.min(60000, Math.max(1000, state.snapshot?.refresh_interval_ms ?? 3000) * 2 ** Math.min(failures, 4));
  }

  function schedulePoll() {
    clearTimeout(pollTimer);
    if (!document.hidden) pollTimer = setTimeout(poll, pollDelay());
  }

  async function poll() {
    clearTimeout(pollTimer);
    if (fleetRequest || document.hidden) return;
    const controller = new AbortController();
    fleetRequest = controller;
    void loadConversations();
    void loadAttention();
    const timeout = setTimeout(() => controller.abort(), 10000);
    render();
    try {
      const value = await fetchJSON('/api/v1/fleet', controller.signal);
      if (controller.signal.aborted) return;
      state = receiveFleet(state, value);
      failures = state.failed ? failures + 1 : 0;
      syncHosts();
    } catch (error) {
      if (!(controller.signal.aborted && document.hidden)) {
        state = failFleet(state, error.message === 'auth' ? 'auth' : 'network');
        failures++;
      }
    } finally {
      clearTimeout(timeout);
      fleetRequest = null;
      render();
      if (refreshOnResume && !document.hidden) {
        refreshOnResume = false;
        void poll();
      } else schedulePoll();
    }
  }

  $('host-filter').addEventListener('change', event => { host = event.target.value; closeConversation(); conversationDialog.close(); render(); });
  $('refresh').addEventListener('click', () => void poll());
  $('read-output').addEventListener('click', () => void readOutput());
  $('close-detail').addEventListener('click', () => { clearOutput(); selectedKey = null; dialog.close(); });
  // Escape fires cancel before close; invalidate immediately, not on a later task.
  dialog.addEventListener('cancel', () => { clearOutput(); selectedKey = null; });
  dialog.addEventListener('close', () => {
    if (!dialog.open) { clearOutput(); selectedKey = null; }
    restoreDialogFocus(dialog);
  });
  $('copy-output').addEventListener('click', async () => {
    const output = renderedOutput;
    const key = selectedKey;
    if (!output || !selectedCard()?.outputEnabled || !navigator.clipboard?.writeText) return;
    try {
      await navigator.clipboard.writeText(output.text);
      if (renderedOutput === output && selectedKey === key) setText($('copy-status'), 'Text copied. Treat copied output as potentially sensitive.');
    } catch {
      if (renderedOutput === output && selectedKey === key) setText($('copy-status'), 'Clipboard access was denied. You can select the text directly.');
    }
  });
  document.addEventListener('visibilitychange', () => {
    clearTimeout(pollTimer);
    if (document.hidden) {
      clearTimeout(attentionTimer);
      attentionRequest?.abort(); attentionRequest = null; attention = null; attentionAt = null;
      invalidateConversations('Conversation metadata paused while hidden. Revalidated on return; reopen to read messages.');
      refreshOnResume = false;
      fleetRequest?.abort();
      if (outputRequest) { clearOutput(); setText($('output-status'), 'Read paused while hidden. Read again when ready.'); }
    } else {
      void loadAttention();
      void loadConversations();
      if (fleetRequest) refreshOnResume = true;
      else void poll();
    }
    render();
  });
  setInterval(() => {
    if (!document.hidden) {
      if (conversationMetadataAt !== null && !conversationMetadataFresh()) {
        invalidateConversations('Conversation metadata lease expired. Revalidating access; reopen to read messages.');
        void loadConversations();
      }
      if (attentionAt !== null && !attentionFresh()) { attention = null; attentionAt = null; fenceDecision(); }
      render();
    }
  }, 1000);
  void poll();
}
