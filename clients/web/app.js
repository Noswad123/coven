export const CONTRACT = 'coven.fleet-observer.v1';
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
  const outputGate = createOutputGate();
  const dialog = $('output-dialog');
  const cardNodes = new Map();

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
    const response = await fetch(path, { method: 'GET', cache: 'no-store', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', headers: { Accept: 'application/json' }, signal });
    if (!response.ok) {
      // Error codes are never treated as upstream display text.
      const body = await response.json().catch(() => null);
      throw new Error(responseError(response.status, body));
    }
    return response.json();
  }

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
    setText($('detail-title'), card.agent.title || 'Untitled agent');
    setText($('output-status'), unavailableReason(card));
    if (!dialog.open) dialog.showModal();
    updateDetail();
    if (card.outputEnabled) void readOutput();
  }

  function render() {
    const now = Date.now();
    const view = projectFleet(state, host, now);
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
    const keys = new Set(view.cards.map(card => card.key));
    for (const [key, node] of cardNodes) if (!keys.has(key)) { node.remove(); cardNodes.delete(key); }
    let position = 0;
    for (const card of view.cards) {
      let node = cardNodes.get(card.key);
      if (!node) {
        node = element('button', 'agent-card');
        node.type = 'button';
        node.setAttribute('aria-haspopup', 'dialog');
        node.addEventListener('click', () => openDetail(card.key));
        const top = element('span', 'card-top');
        top.append(element('span', 'host-badge'), element('span', 'activity'));
        const body = element('span');
        body.append(element('span', 'card-title'), element('span', 'card-project'), element('span', 'card-project card-evidence'));
        const bottom = element('span', 'card-bottom');
        bottom.append(element('span', 'card-age'), element('span', 'card-action'));
        node.append(top, body, bottom);
        cardNodes.set(card.key, node);
      }
      node.dataset.stale = String(!card.fresh);
      setText(node.querySelector('.host-badge'), card.endpoint.label || card.endpoint.id);
      setText(node.querySelector('.activity'), card.fresh ? card.activityLabel : `Stale / ${card.activityLabel}`);
      setText(node.querySelector('.card-title'), card.agent.title || 'Untitled agent');
      setText(node.querySelector('.card-project'), `${card.providerLabel} / ${card.agent.project || 'No project'}`);
      setText(node.querySelector('.card-evidence'), `${card.evidenceLabel}${card.familyHints ? ` / ${card.familyHints}` : ''}`);
      setText(node.querySelector('.card-age'), card.age);
      setText(node.querySelector('.card-action'), card.outputEnabled ? 'Read output +' : 'Details / output unavailable');
      const current = $('agent-grid').children[position++];
      if (current !== node) $('agent-grid').insertBefore(node, current ?? null);
    }
    setText($('agent-count'), snapshot ? `${view.cards.length} agent${view.cards.length === 1 ? '' : 's'}${stale ? ' / last known' : ''}` : 'No snapshot');
    const chips = view.endpoints.map(endpoint => {
      const endpointStale = state.failed || snapshotOld || endpoint.observed_at_ms === null || now - endpoint.observed_at_ms >= snapshot.stale_after_ms || endpoint.observed_at_ms > now + 5000;
      const status = endpoint.connection === 'reachable' && endpointStale ? 'stale' : endpoint.connection;
      const chip = element('span', 'host-chip', `${endpoint.label || endpoint.id} / ${status} / ${endpointCoverage(endpoint)}`);
      chip.dataset.state = status;
      return chip;
    });
    $('host-health').replaceChildren(...chips);
    $('empty-state').hidden = view.cards.length !== 0;
    setText($('empty-title'), view.emptyTitle);
    setText($('empty-description'), view.emptyDescription);
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

  $('host-filter').addEventListener('change', event => { host = event.target.value; render(); });
  $('refresh').addEventListener('click', () => void poll());
  $('read-output').addEventListener('click', () => void readOutput());
  $('close-detail').addEventListener('click', () => { clearOutput(); selectedKey = null; dialog.close(); });
  // Escape fires cancel before close; invalidate immediately, not on a later task.
  dialog.addEventListener('cancel', () => { clearOutput(); selectedKey = null; });
  dialog.addEventListener('close', () => { if (!dialog.open) { clearOutput(); selectedKey = null; } });
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
      refreshOnResume = false;
      fleetRequest?.abort();
      if (outputRequest) { clearOutput(); setText($('output-status'), 'Read paused while hidden. Read again when ready.'); }
    } else if (fleetRequest) refreshOnResume = true;
    else void poll();
    render();
  });
  setInterval(() => { if (!document.hidden) render(); }, 1000);
  void poll();
}
