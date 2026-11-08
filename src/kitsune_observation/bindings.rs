use super::*;

pub(crate) struct ProcessRequest<'a> {
    pub socket: &'a str,
    pub pane: &'a str,
    pub deadline: Instant,
    pub frozen_endpoint: Option<&'a Value>,
    pub expected_terminal: Option<&'a str>,
}

pub(crate) fn read_publication_snapshot(
    socket: &str,
    deadline: Instant,
    budget: &mut ReadBudget,
    process_only: bool,
    cancel: &dyn Cancel,
) -> WireResult<Value> {
    publication_with(
        socket,
        deadline,
        budget,
        process_only,
        cancel,
        &SystemClock,
        &SocketTransport,
    )
}
// Keep injected clock/transport beside the exact caller-owned budget and deadline.
#[allow(clippy::too_many_arguments)]
pub(super) fn publication_with(
    socket: &str,
    deadline: Instant,
    budget: &mut ReadBudget,
    process_only: bool,
    cancel: &dyn Cancel,
    clock: &dyn Clock,
    transport: &dyn Transport,
) -> WireResult<Value> {
    let r = transport.read(
        socket,
        ReadMethod::Snapshot,
        &json!({}),
        deadline,
        budget,
        cancel,
    )?;
    let s = schema::snapshot(&r, process_only)?;
    let (mut epoch, mut sources, mut valid_until) = (Value::Null, Value::Null, false);
    if let Some(a) = &s.annotations {
        let a = schema::publication_annotations(a)?;
        epoch = a["epoch"].clone();
        sources = a["sources"].clone();
        valid_until = a["valid_until_unix_ms"] == Value::Bool(true);
    }
    let agents = if process_only {
        vec![]
    } else {
        s["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let mut out = json!({"agent":a["agent"],"agent_session":a["agent_session"]});
                for k in schema::LOCATORS {
                    out[k] = a[k].clone();
                }
                out
            })
            .collect()
    };
    let panes: Vec<Value> = s["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let mut out = json!({});
            for k in schema::LOCATORS {
                out[k] = p[k].clone();
            }
            out
        })
        .collect();
    let out = json!({"endpoint":identity(&r.endpoint)?,"epoch":epoch,"sources":sources,"valid_until_unix_ms":valid_until,
        "observed_at":clock.timestamp().map_err(|e|Failure::new(e.code()))?,"agents":agents,"panes":panes});
    guard(clock, deadline, cancel)?;
    Ok(out)
}

pub(crate) fn binding_from_snapshot(snapshot: &Value, pane: &str) -> WireResult<Value> {
    schema::require(schema::opaque(pane))?;
    let agents = snapshot["agents"]
        .as_array()
        .ok_or_else(|| Failure::new("schema_error"))?;
    let matches: Vec<_> = agents
        .iter()
        .filter(|a| a["pane_id"].as_str() == Some(pane))
        .collect();
    if matches.len() != 1 {
        return Err(Failure::new("agent_not_found"));
    }
    let a = matches[0];
    let native = &a["agent_session"];
    Ok(
        json!({"presentation":{"adapter":"kitsune","endpoint":snapshot["endpoint"],"terminal_id":a["terminal_id"],"pane_id_at_binding":pane},
        "evidence":{"provenance":"kitsune.session.snapshot","observed_at":snapshot["observed_at"],"agent_kind_hint":a["agent"],
        "native_reference":native,"native_binding":if native.is_null(){"unestablished"}else{"observed_reference"}}}),
    )
}

pub(crate) fn binding_from_process(
    request: ProcessRequest<'_>,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
) -> WireResult<Value> {
    process_binding_with(request, budget, cancel, &SystemClock, &SocketTransport)
}
pub(super) fn process_binding_with(
    request: ProcessRequest<'_>,
    budget: &mut ReadBudget,
    cancel: &dyn Cancel,
    clock: &dyn Clock,
    transport: &dyn Transport,
) -> WireResult<Value> {
    observation_socket::validate_path(request.socket).map_err(|e| Failure::new(e.code()))?;
    schema::require(schema::opaque(request.pane))?;
    guard(clock, request.deadline, cancel)?;
    let endpoint = transport.select(request.socket)?;
    let id = identity(&endpoint)?;
    if request.frozen_endpoint.is_some_and(|f| *f != id) {
        return Err(Failure::new("endpoint_replaced"));
    }
    let item = evidence(&request, &endpoint, budget, cancel, clock, transport)?;
    if item["recognition"] != "djinn_executable_family" {
        return Err(Failure::new("process_not_recognized"));
    }
    let mut matches = item["matches"]
        .as_array()
        .cloned()
        .ok_or_else(|| Failure::new("schema_error"))?;
    matches.sort_by_key(|m| {
        (
            m["pid"].as_u64().unwrap_or(0),
            m["executable_family"].as_str().unwrap_or("").to_owned(),
            m["rule"].as_str().unwrap_or("").to_owned(),
        )
    });
    // Group mutation validates this exact producer envelope again before storage.
    // Enforce its byte-oriented locator limits here, not just wire character caps.
    let terminal = item["locators"]["terminal_id"]
        .as_str()
        .ok_or_else(|| Failure::new("schema_error"))?;
    schema::require(
        terminal.len() <= 4096 && request.pane.len() <= 4096 && endpoint.resolved.len() <= 8192,
    )?;
    let out = json!({"presentation":{"adapter":"kitsune","endpoint":id,"terminal_id":terminal,"pane_id_at_binding":request.pane},
        "evidence":{"provenance":"kitsune.pane.process_info","observed_at":item["sample_received_at"],"agent_kind_hint":"djinn",
        "native_reference":null,"native_binding":"unestablished","process_evidence":{"interpreter":PROCESS_CONTRACT,"matches":matches}}});
    guard(clock, request.deadline, cancel)?;
    Ok(out)
}
