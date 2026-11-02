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
    let (start, field, route): (_, _, &[&str]) =
        match arguments.get(1).and_then(|value| value.to_str()) {
            Some("checkpoint") if arguments.get(2).is_some_and(|value| value == "inspect") => {
                (3, "items", &["checkpoint", "inspect"])
            }
            Some("next") => (2, "workspace", &["next"]),
            Some("execution") if arguments.get(2).is_some_and(|value| value == "inspect") => {
                (3, "workspace", &["execution", "inspect"])
            }
            Some("task") => match (
                arguments.get(2).and_then(|value| value.to_str()),
                arguments.get(3).and_then(|value| value.to_str()),
            ) {
                (Some("attempt"), Some("list")) => (4, "workspace", &["task", "attempt", "list"]),
                (Some("attempt"), Some("inspect")) => (4, "items", &["task", "attempt", "inspect"]),
                (Some("graph"), Some("inspect")) => (4, "items", &["task", "graph", "inspect"]),
                (Some("graph"), Some("export")) => (4, "items", &["task", "graph", "export"]),
                _ => return (command, parsed),
            },
            _ => return (command, parsed),
        };
    let mut occupied = arguments.iter().cloned().collect::<HashSet<_>>();
    if route == ["task", "attempt", "list"] {
        // Attached option values also reach the task parser as standalone strings.
        occupied.extend(arguments.iter().filter_map(|argument| {
            argument
                .to_str()?
                .split_once('=')
                .map(|(_, value)| OsString::from(value))
        }));
    }
    let mut originals = HashMap::new();
    let mut serial = 0usize;
    for argument in parsed.iter_mut().skip(start) {
        if argument == "--" {
            break;
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
    let task_originals = originals.clone();
    let task_parser = clap::builder::StringValueParser::new().map(move |value| {
        task_originals
            .get(&OsString::from(&value))
            .and_then(|original| original.to_str())
            .map(str::to_owned)
            .unwrap_or(value)
    });
    let parser = clap::builder::OsStringValueParser::new()
        .map(move |value| PathBuf::from(originals.get(&value).cloned().unwrap_or(value)));
    let install = |command: clap::Command| {
        let command = command.mut_arg(field, |argument| argument.value_parser(parser));
        if route == ["task", "attempt", "list"] {
            command.mut_arg("task", |argument| argument.value_parser(task_parser))
        } else {
            command
        }
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
