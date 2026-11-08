use super::coordinator::Io;
use super::*;

pub(super) fn deliver(root: &Path, selector: &str, report: &mut Report, io: &impl Io) {
    let targets = match io.targets(root, selector) {
        Ok(targets) => targets,
        Err(error) => {
            report.warnings.push(format!(
                "Checkpoint committed; notifications unavailable: {}",
                error.code()
            ));
            return;
        }
    };
    let message = format!(
        "Checkpoint `{}` decision {} recorded as {} for scope {}. Reinspect current checkpoint status before acting. This notice is not execution authorization.",
        report.checkpoint, report.decision_id, report.action, report.scope_revision
    );
    for agent in targets {
        let attempt = (|| -> Result<bool> {
            // Deliberately outside the workspace guard, immediately before EACH
            // recipient. Transport separately rechecks runtime mode under its gate.
            let current = view(root, &report.checkpoint)?;
            let identity = current
                .as_object()
                .and_then(|o| o.get("decision"))
                .and_then(Value::as_object)
                .and_then(|o| o.get("decision_id"))
                .and_then(Value::as_str);
            if identity != Some(report.decision_id.as_str())
                || field(&current, "effective_status")? != report.action
            {
                report.warnings.push(
                    "Checkpoint notice suppressed: decision superseded or no longer effective"
                        .into(),
                );
                return Ok(false);
            }
            if io.send(root, &agent, &message)? {
                report.sent += 1;
                io.notice_history(root, &agent, report)?;
            } else {
                report.missing.push(agent.clone());
            }
            Ok(true)
        })();
        match attempt {
            Ok(false) => break,
            Ok(true) => {}
            Err(error) => report.warnings.push(format!(
                "Checkpoint committed; notice delivery/history failed for {agent}: {}",
                error.code()
            )),
        }
    }
}
