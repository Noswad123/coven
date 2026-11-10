use super::Cli;
use clap::{CommandFactory, builder::TypedValueParser};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;

// Python 3.14 argparse uses the prefix regex r'-\.?\d', not a float parser.
// Substitute only while lexing: typed parsing restores exact paths and retains
// argument indices. allow_hyphen_values would swallow a trailing --json instead.
pub(super) fn checkpoint_compat(arguments: &[OsString]) -> (clap::Command, Vec<OsString>) {
    let command = Cli::command();
    let mut parsed = arguments.to_vec();
    let (start, field, route): (_, _, &[&str]) = match arguments
        .get(1)
        .and_then(|value| value.to_str())
    {
        Some("approve") => (2, "items", &["approve"]),
        Some("init") => (2, "workspace", &["init"]),
        Some("suggest") => (2, "items", &["suggest"]),
        Some("review") => (2, "items", &["review"]),
        Some("send") => (2, "items", &["send"]),
        Some("lead") => (2, "items", &["lead"]),
        Some("standup") => (2, "workspace", &["standup"]),
        Some("console") => (2, "workspace", &["console"]),
        Some("bootstrap") => (2, "workspace", &["bootstrap"]),
        Some("plan") => (2, "workspace", &["plan"]),
        Some("up" | "start") => (2, "workspace", &["up"]),
        Some("restart") => (2, "workspace", &["restart"]),
        Some("down" | "stop") => (2, "workspace", &["down"]),
        Some("agent") if arguments.get(2).is_some_and(|value| value == "send") => {
            (3, "items", &["agent", "send"])
        }
        Some("agent") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("status") => (3, "items", &["agent", "status"]),
            Some("start") => (3, "items", &["agent", "start"]),
            Some("stop") => (3, "items", &["agent", "stop"]),
            Some("restart") => (3, "items", &["agent", "restart"]),
            _ => return (command, parsed),
        },
        Some("action") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("enable-runtime-launch") => (3, "workspace", &["action", "enable-runtime-launch"]),
            Some("prepare-runtime-launch") => {
                (3, "workspace", &["action", "prepare-runtime-launch"])
            }
            Some("inspect") => (3, "workspace_or_action", &["action", "inspect"]),
            Some("submit") => (3, "workspace_or_action", &["action", "submit"]),
            Some("acknowledge-unknown") => {
                (3, "workspace_or_action", &["action", "acknowledge-unknown"])
            }
            Some("stop") => (3, "workspace_or_action", &["action", "stop"]),
            _ => return (command, parsed),
        },
        Some("checkpoint") if arguments.get(2).is_some_and(|value| value == "inspect") => {
            (3, "items", &["checkpoint", "inspect"])
        }
        Some("next") => (2, "workspace", &["next"]),
        Some("repository") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("inspect") => (3, "workspace", &["repository", "inspect"]),
            Some("register") => (3, "workspace", &["repository", "register"]),
            _ => return (command, parsed),
        },
        Some("worker") if arguments.get(2).is_some_and(|value| value == "readiness") => {
            (3, "workspace", &["worker", "readiness"])
        }
        Some("observe") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("djinn") => (3, "", &["observe", "djinn"]),
            Some("kitsune") => (3, "", &["observe", "kitsune"]),
            Some("djinn-live") => (3, "", &["observe", "djinn-live"]),
            _ => return (command, parsed),
        },
        Some("group") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("create") => (3, "workspace", &["group", "create"]),
            Some("add") => (3, "workspace", &["group", "add"]),
            Some("remove") => (3, "workspace", &["group", "remove"]),
            Some("list") => (3, "workspace", &["group", "list"]),
            Some("attach-live") => (3, "workspace", &["group", "attach-live"]),
            Some("detach-live") => (3, "workspace", &["group", "detach-live"]),
            Some("status") => (3, "workspace", &["group", "status"]),
            Some("publish") => (3, "workspace", &["group", "publish"]),
            Some("present") => (3, "workspace", &["group", "present"]),
            _ => return (command, parsed),
        },
        Some("session") => match arguments.get(2).and_then(|value| value.to_str()) {
            Some("list") => (3, "workspace", &["session", "list"]),
            Some("inspect") => (3, "workspace", &["session", "inspect"]),
            Some("watch") => (3, "workspace", &["session", "watch"]),
            Some("start") => (3, "workspace", &["session", "start"]),
            Some("resume") => (3, "workspace", &["session", "resume"]),
            Some("plan-resume") => (3, "", &["session", "plan-resume"]),
            Some("prompt") => (3, "workspace", &["session", "prompt"]),
            Some("prompt-capabilities") => (3, "", &["session", "prompt-capabilities"]),
            _ => return (command, parsed),
        },
        Some("execution") if arguments.get(2).is_some_and(|value| value == "inspect") => {
            (3, "workspace", &["execution", "inspect"])
        }
        Some("execution") if arguments.get(2).is_some_and(|value| value == "recover") => {
            (3, "workspace", &["execution", "recover"])
        }
        Some("execution") if arguments.get(2).is_some_and(|value| value == "enable") => {
            (3, "workspace", &["execution", "enable"])
        }
        Some("execution") if arguments.get(2).is_some_and(|value| value == "enroll") => {
            (3, "items", &["execution", "enroll"])
        }
        Some("execution") if arguments.get(2).is_some_and(|value| value == "upgrade") => {
            (3, "workspace", &["execution", "upgrade"])
        }
        Some("task") => match (
            arguments.get(2).and_then(|value| value.to_str()),
            arguments.get(3).and_then(|value| value.to_str()),
        ) {
            (Some("attempt"), Some("list")) => (4, "workspace", &["task", "attempt", "list"]),
            (Some("attempt"), Some("inspect")) => (4, "items", &["task", "attempt", "inspect"]),
            (Some("attempt"), Some("cancel")) => (4, "items", &["task", "attempt", "cancel"]),
            (Some("attempt"), Some("prepare")) => (4, "items", &["task", "attempt", "prepare"]),
            (Some("graph"), Some("propose")) => (4, "workspace", &["task", "graph", "propose"]),
            (Some("graph"), Some("decide")) => (4, "items", &["task", "graph", "decide"]),
            (Some("graph"), Some("inspect")) => (4, "items", &["task", "graph", "inspect"]),
            (Some("graph"), Some("export")) => (4, "items", &["task", "graph", "export"]),
            _ => return (command, parsed),
        },
        _ => return (command, parsed),
    };
    let string_fields: &[&str] = match route {
        ["session", "plan-resume"] => &["runtime", "source", "session", "executable"],
        ["session", "list"] => &["socket", "group", "member"],
        ["session", "inspect" | "watch"] => {
            &["socket", "pane", "expected_terminal", "group", "member"]
        }
        ["session", "start"] => &["executable", "socket", "pane", "expected_terminal"],
        ["session", "resume"] => &[
            "executable",
            "socket",
            "pane",
            "expected_terminal",
            "session",
        ],
        ["session", "prompt"] => &[
            "native_socket",
            "expected_runtime",
            "session",
            "request_id",
            "prompt_file",
        ],
        ["session", "prompt-capabilities"] => &["native_socket"],
        ["init"] => &["name", "agents", "opencode_command", "example"],
        ["approve"] => &["agent", "message", "expect_revision"],
        ["suggest"] => &["agent", "expect_revision"],
        ["review"] => &["agent", "message"],
        ["lead"] => &["message"],
        ["action", "submit"] => &[
            "action_id",
            "expect_manifest",
            "expect_checkpoint",
            "expect_decision",
        ],
        ["action", "acknowledge-unknown"] => &["action_id", "expect_claim"],
        ["action", "inspect" | "stop"] => &["action_id"],
        ["repository", "register"] => &["base", "delivery_target", "expect_repository_revision"],
        ["observe", "djinn"] => &["executable", "folder"],
        ["observe", "kitsune"] => &["socket", "process_pane"],
        ["observe", "djinn-live"] => &["file", "runtime", "instance", "session"],
        ["group", "create"] => &["group", "name", "expect_revision"],
        ["group", "add"] => &[
            "group",
            "member",
            "role",
            "socket",
            "pane",
            "expect_revision",
        ],
        ["group", "remove" | "detach-live"] => &["group", "member", "expect_revision"],
        ["group", "attach-live"] => &[
            "group",
            "member",
            "expect_revision",
            "file",
            "runtime",
            "instance",
            "session",
        ],
        ["group", "status"] => &["group", "member"],
        ["group", "publish" | "present"] => &["group", "socket", "source"],
        ["task", "attempt", "list"] => &["task"],
        ["task", "attempt", "cancel"] => &["reason", "expect_execution_revision", "request_id"],
        ["execution", "recover"] => &["expect_execution_revision"],
        ["execution", "enable" | "upgrade"] => &["expect_execution_revision", "request_id"],
        ["execution", "enroll"] => &[
            "expect_execution_revision",
            "expect_tasks_revision",
            "request_id",
        ],
        ["task", "attempt", "prepare"] => &[
            "graph_revision",
            "expect_execution_revision",
            "expect_tasks_revision",
            "request_id",
        ],
        ["task", "graph", "propose"] => &[
            "expect_execution_revision",
            "expect_tasks_revision",
            "request_id",
        ],
        ["task", "graph", "decide"] => &[
            "reason",
            "expect_graph_digest",
            "expect_execution_revision",
            "expect_tasks_revision",
            "request_id",
        ],
        _ => &[],
    };
    let path_fields: &[&str] = match route {
        ["repository", "register"] => &["path"],
        ["worker", "readiness"] => &["executable"],
        ["task", "attempt", "prepare"] => &["intent"],
        ["task", "graph", "propose"] => &["document"],
        _ => &[],
    };
    let mut occupied = arguments.iter().cloned().collect::<HashSet<_>>();
    if !string_fields.is_empty() || !path_fields.is_empty() {
        // Attached values reach custom typed parsers as standalone values too.
        occupied.extend(arguments.iter().filter_map(|argument| {
            argument
                .to_str()?
                .split_once('=')
                .map(|(_, value)| OsString::from(value))
        }));
    }
    let mut originals = HashMap::new();
    let mut serial = 0usize;
    let numeric_names: &[&str] = match route {
        ["session", "plan-resume"] => &["session-kind"],
        ["session", "list" | "inspect"] => &["runtime", "timeout", "max-response-bytes"],
        ["session", "watch"] => &[
            "runtime",
            "timeout",
            "max-response-bytes",
            "duration",
            "interval",
        ],
        ["session", "start" | "resume"] => &["runtime", "timeout"],
        ["session", "prompt" | "prompt-capabilities"] => &["timeout"],
        ["init"] => &["multiplexer"],
        ["worker", "readiness"] | ["observe", "djinn"] => &["timeout", "max-output-bytes"],
        ["observe", "kitsune"] => &["timeout", "max-response-bytes"],
        ["observe", "djinn-live"] | ["group", "attach-live" | "status"] => &["timeout"],
        ["group", "publish" | "present"] => &["duration"],
        _ => &[],
    };
    let mut numeric_value = false;
    for argument in parsed.iter_mut().skip(start) {
        if argument == "--" {
            break;
        }
        // Keep numeric and choice values transparent to Clap's validators,
        // including invalid raw values in their diagnostics.
        if numeric_value {
            numeric_value = false;
            continue;
        }
        if !numeric_names.is_empty()
            && argument.to_str().is_some_and(|value| {
                value.strip_prefix("--").is_some_and(|name| {
                    !name.is_empty()
                        && numeric_names.iter().any(|option| {
                            // The new pure planner deliberately does not infer flags:
                            // --session must not be mistaken for --session-kind.
                            if route == ["session", "plan-resume"] {
                                *option == name
                            } else {
                                option.starts_with(name)
                            }
                        })
                })
            })
        {
            numeric_value = true;
            continue;
        }
        if !argument.to_str().is_some_and(|value| {
            let Some(tail) = value.strip_prefix('-') else {
                return false;
            };
            let tail = tail.strip_prefix('.').unwrap_or(tail);
            tail.chars().next().is_some_and(|first| {
                UNICODE_16_ND
                    .iter()
                    .any(|&(start, end)| (start..=end).contains(&(first as u32)))
            })
        }) {
            continue;
        }
        let token = loop {
            let token = OsString::from(format!("__coven_checkpoint_operand_{serial}"));
            serial += 1;
            if !occupied.contains(&token) {
                break token;
            }
        };
        originals.insert(token.clone(), argument.clone());
        *argument = token;
    }
    let string_originals = originals.clone();
    let string_parser = clap::builder::StringValueParser::new().map(move |value| {
        string_originals
            .get(&OsString::from(&value))
            .and_then(|original| original.to_str())
            .map(str::to_owned)
            .unwrap_or(value)
    });
    let parser = clap::builder::OsStringValueParser::new()
        .map(move |value| PathBuf::from(originals.get(&value).cloned().unwrap_or(value)));
    let install = |command: clap::Command| {
        let mut command = if field.is_empty() {
            command
        } else {
            command.mut_arg(field, |argument| argument.value_parser(parser.clone()))
        };
        for field in path_fields {
            command = command.mut_arg(*field, |argument| argument.value_parser(parser.clone()));
        }
        for field in string_fields {
            command = command.mut_arg(*field, |argument| {
                argument.value_parser(string_parser.clone())
            });
        }
        command
    };
    let command = command.mut_subcommand(route[0], |command| match route.len() {
        1 => install(command),
        2 => command.mut_subcommand(route[1], install),
        _ => command.mut_subcommand(route[1], |command| {
            command.mut_subcommand(route[2], install)
        }),
    });
    (command, parsed)
}

// Private CLI characterization, not a general Unicode API. Frozen from CPython
// 3.14's unicodedata 16.0.0: merge consecutive code points whose category is Nd.
// Reproduce/compare every scalar with the python_unicode_16_oracle test.
// UnicodeData.txt: https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt
const UNICODE_16_ND: &[(u32, u32)] = &[
    (0x0030, 0x0039),
    (0x0660, 0x0669),
    (0x06F0, 0x06F9),
    (0x07C0, 0x07C9),
    (0x0966, 0x096F),
    (0x09E6, 0x09EF),
    (0x0A66, 0x0A6F),
    (0x0AE6, 0x0AEF),
    (0x0B66, 0x0B6F),
    (0x0BE6, 0x0BEF),
    (0x0C66, 0x0C6F),
    (0x0CE6, 0x0CEF),
    (0x0D66, 0x0D6F),
    (0x0DE6, 0x0DEF),
    (0x0E50, 0x0E59),
    (0x0ED0, 0x0ED9),
    (0x0F20, 0x0F29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10D30, 0x10D39),
    (0x10D40, 0x10D49),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x116D0, 0x116E3),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11BF0, 0x11BF9),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16130, 0x16139),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x16D70, 0x16D79),
    (0x1CCF0, 0x1CCF9),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E5F1, 0x1E5FA),
    (0x1E950, 0x1E959),
    (0x1FBF0, 0x1FBF9),
];

#[cfg(test)]
#[path = "cli_operands/tests.rs"]
mod tests;
