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
    setText($('detail-chat-status'), session?.target ? 'Verified terminal link. Open chat from the monitoring card.'
      : `Chat unavailable: ${session?.chatUnavailable ?? 'This fleet reference was removed.'}`);
    if (card) {
      setText($('detail-title'), card.agent.title || 'Untitled agent');
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
    conversationRequest?.abort();
    conversationRequest = null;
    conversationGate.invalidate();
    $('conversation-messages').replaceChildren();
    setText($('conversation-status'), '');
    $('read-conversation').disabled = false;
  }

  function closeConversation() {
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
      const hidden = value.messages.length - displayed.length;
      for (const message of displayed) {
        const article = element('article', 'conversation-message');
        article.append(element('h3', '', `${message.role === 'user' ? 'User' : 'Assistant'}${message.truncated ? ' / truncated' : ''}`), element('pre', '', message.text));
        $('conversation-messages').append(article);
      }
      if (!displayed.length) {
        $('conversation-messages').append(element('p', 'muted small', 'No displayable text in this sample. Non-text or truncated content may not be included.'));
      }
      const partial = value.truncated || value.omitted_parts || value.messages.some(m => m.truncated);
      setText($('conversation-status'), `${displayed.length} text messages shown / native projection${hidden ? ` / ${hidden} entries without displayable text hidden` : ''}${partial ? ' / partial' : ''}${value.has_older ? ' / older messages not shown' : ''}${value.truncated ? ' / truncated' : ''}${value.omitted_parts ? ' / non-text or unsupported parts omitted' : ''}. Refresh explicitly for changes.`);
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

  function openConversation(card) {
    const target = card.target;
    if (!target?.binding || !conversationMetadataFresh()) return;
    clearConversation();
    fencePrompt();
    conversationTarget = target;
    dialogReturnTargets.set(conversationDialog, focusReference(card));
    setText($('conversation-title'), `${card.agent.title} / ${target.team}`);
    setText($('conversation-link-status'), 'Verified terminal link');
    if (!conversationDialog.open) conversationDialog.showModal();
    renderComposer();
    void readConversation();
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
      const coverage = (value.sources ?? []).map(source => `${source.label} (${source.id}) / ${source.team}: ${source.status}`).join('; ');
      const awaiting = conversationTargets.filter(target => !sessions().some(card => card.target?.id === target.id)).length;
      setText($('conversation-list-status'), `${conversationTargets.length ? `Authorized chat metadata, not execution readiness.${awaiting ? ` ${awaiting} shared session${awaiting === 1 ? '' : 's'} awaiting association.` : ''} Verified monitoring cards open directly; follow-ups require explicit write sharing.`
        : 'No authorized conversations currently available. The operator must configure a read-only registration or an authorized native sharing source and current selection.'}${coverage ? ` Native source health: ${coverage}.` : ''}${value.omitted_conversations ? ` ${value.omitted_conversations} authorized conversations omitted by the list limit.` : ''}`);
    } catch (error) {
      if (revision !== conversationListRevision) return;
      invalidateConversations(error.message === 'auth' ? errorMessage('auth')
        : 'Authorized conversation list unavailable. Check access and gateway setup.');
    } finally {
      clearTimeout(timeout);
      if (conversationListRequest === controller) conversationListRequest = null;
    }
  }

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
    setText($('detail-title'), card.agent.title || 'Untitled agent');
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
        fencePrompt();
        clearConversation();
        if (!updated) { closeConversation(); conversationDialog.close(); }
      }
      if (updated) conversationTarget = updated;
    }
    renderComposer();
    const snapshot = state.snapshot;
    const snapshotOld = snapshot && (snapshot.generated_at_ms > now + 5000 || now - snapshot.generated_at_ms >= snapshot.stale_after_ms);
    const stale = state.failed || snapshotOld;
    const connectivity = !snapshot ? (state.failed ? 'Observatory unavailable' : 'Connecting to the observatory...')
      : stale ? 'Last-known fleet / stale' : 'Connected to the observatory';
    setText($('connection-status'), document.hidden ? `${connectivity} / polling paused` : connectivity);
    $('connection-dot').dataset.state = snapshot && !stale ? 'fresh' : 'stale';
    setText($('poll-status'), `${snapshot ? `Fleet ${ageLabel(snapshot.generated_at_ms, now).toLowerCase()}. ` : ''}${document.hidden ? 'Polling paused while hidden.' : `Polling every ${Math.max(1, Math.round(pollDelay() / 1000))}s${failures ? ' with failure backoff' : ''}.`}`);
    $('notice').hidden = !state.failed;
    if (state.failed) setText($('notice'), errorMessage(state.error));
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
        bottom.append(element('span', 'card-age'), element('span', 'card-action'));
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
      setText(node.querySelector('.card-title'), card.agent.title || 'Untitled session');
      setText(node.querySelector('.card-project'), `${card.providerLabel} / ${card.agent.project || 'No project'} / ${card.target ? `Chat team: ${card.target.team}` : 'Team unassigned'}`);
      setText(node.querySelector('.card-evidence'), `${card.evidenceLabel}${card.familyHints ? ` / ${card.familyHints}` : ''} / ${card.target
        ? 'Chat capable / Verified terminal link'
        : `Chat unavailable: ${card.chatUnavailable}`}`);
      setText(node.querySelector('.card-age'), card.age);
      setText(node.querySelector('.card-action'), card.target ? 'Open conversation +' : 'Chat unavailable');
      node.querySelector('.agent-card').setAttribute('aria-disabled', String(!card.target));
      node.querySelector('.card-details').hidden = false;
      const current = $('agent-grid').children[position++];
      if (current !== node) $('agent-grid').insertBefore(node, current ?? null);
    }
    if (focusedReference) restoreCardFocus(focusedReference);
    setText($('agent-count'), `${cards.length} session card${cards.length === 1 ? '' : 's'}${stale ? ' / fleet last known' : ''}`);
    const chips = view.endpoints.map(endpoint => {
      const endpointStale = state.failed || snapshotOld || endpoint.observed_at_ms === null || now - endpoint.observed_at_ms >= snapshot.stale_after_ms || endpoint.observed_at_ms > now + 5000;
      const status = endpoint.connection === 'reachable' && endpointStale ? 'stale' : endpoint.connection;
      const chip = element('span', 'host-chip', `${endpoint.label || endpoint.id} / ${status} / ${endpointCoverage(endpoint)}`);
      chip.dataset.state = status;
      return chip;
    });
    $('host-health').replaceChildren(...chips);
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
      invalidateConversations('Conversation metadata paused while hidden. Revalidated on return; reopen to read messages.');
      refreshOnResume = false;
      fleetRequest?.abort();
      if (outputRequest) { clearOutput(); setText($('output-status'), 'Read paused while hidden. Read again when ready.'); }
    } else {
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
      render();
    }
  }, 1000);
  void poll();
}
