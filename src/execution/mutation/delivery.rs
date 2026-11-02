use super::*;

pub(super) const PREFIX: &str =
    "Known execution receipt retained; current authority verification/durability unconfirmed: ";
pub(super) const SLOT: &str = "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

pub(super) struct Prepared {
    committed: TaskOutput,
    uncertain: TaskOutput,
    slot: usize,
}

fn render(value: &Value, json: bool, exit_code: u8) -> Result<TaskOutput> {
    // Admit both CLI spellings, even when only one was requested.
    let pretty = value.storage()?;
    let compact = value.compact()?;
    let ascii = |bytes: &[u8]| -> Result<String> {
        let text = std::str::from_utf8(bytes)?;
        let text = crate::output::ascii_json(text.strip_suffix('\n').unwrap_or(text))?;
        require(
            text.len() <= values::MAX_OUTPUT - 8192,
            "unsupported_json_output_profile",
        )?;
        Ok(text)
    };
    let pretty = ascii(&pretty)?;
    let compact = ascii(&compact)?;
    Ok(TaskOutput {
        stdout: if json { compact } else { pretty },
        warnings: Vec::new(),
        exit_code,
    })
}

impl Prepared {
    pub(super) fn new(
        view: &Validated,
        result: &Value,
        replayed: bool,
        json: bool,
    ) -> Result<Self> {
        let attempt = view.replayed.state.attempts.iter().find(|a| {
            get(result, "attempt_id").ok().and_then(Value::as_str) == Some(a.id.as_str())
        });
        let mut delivery = object(vec![
            ("outcome", string("committed")),
            ("replayed", Value::Bool(replayed)),
            ("current_revision", string(&view.replayed.revision)),
            ("warnings", Value::Array(vec![])),
        ])?;
        if let Some(attempt) = attempt {
            delivery.set_field("attempt_state", string(attempt.state()))?;
        }
        let mut envelope = object(vec![
            ("result", result.clone()),
            ("delivery", delivery.clone()),
            ("authorization", string("none")),
            ("dispatchable", Value::Bool(false)),
            (
                "blockers",
                Value::Array(
                    [
                        "PREPARATION_ONLY",
                        "NATIVE_ADMISSION_NOT_IMPLEMENTED",
                        "TASK_EXECUTION_AUTHORIZATION_NOT_IMPLEMENTED",
                    ]
                    .into_iter()
                    .map(string)
                    .collect(),
                ),
            ),
        ])?;
        let committed = render(&envelope, json, 0)?;
        delivery.set_field("outcome", string("durability_uncertain"))?;
        delivery.set_field("known_control_revision", string(&view.replayed.revision))?;
        delivery.set_field("current_revision", Value::Null)?;
        if attempt.is_some() {
            delivery.set_field("attempt_state", Value::Null)?;
        }
        delivery.set_field(
            "warnings",
            Value::Array(vec![string(format!("{PREFIX}{SLOT}"))]),
        )?;
        envelope.set_field("delivery", delivery)?;
        let uncertain = render(&envelope, json, 2)?;
        // Locate the delivery warning, not a coincidentally identical reason.
        let marker = format!("{PREFIX}{SLOT}");
        let slot = uncertain
            .stdout
            .rfind(&marker)
            .ok_or(Error("invalid_execution_delivery"))?
            + PREFIX.len();
        Ok(Self {
            committed,
            uncertain,
            slot,
        })
    }
    pub(super) fn confirm(self, result: Result<()>) -> TaskOutput {
        match result {
            Ok(()) => self.committed,
            Err(error) => self.uncertain(error),
        }
    }
    pub(super) fn uncertain(mut self, error: Error) -> TaskOutput {
        // Only bounded JSON-safe native diagnostic codes can enter the admitted slot.
        let mut code = String::with_capacity(128);
        for byte in error.code().bytes().take(128) {
            code.push(
                if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':') {
                    char::from(byte)
                } else {
                    '_'
                },
            );
        }
        self.uncertain
            .stdout
            .replace_range(self.slot..self.slot + SLOT.len(), &code);
        self.uncertain
    }
}
