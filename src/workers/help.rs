//! Strict advertised Clap declarations, never command discovery from prose.
use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result, error::require};

#[derive(Debug)]
pub(super) struct Help {
    pub commands: Vec<String>,
    pub flags: Vec<String>,
}

/// Strip only the CSI/OSC forms accepted by the legacy adapter. Unknown escapes
/// remain in the text so the help/version validators can reject them.
pub(super) fn strip_ansi(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != 0x1b {
            at += 1;
            continue;
        }
        let mut end = at + 2;
        let matched = match bytes.get(at + 1) {
            Some(b'[') => {
                while bytes.get(end).is_some_and(|c| (0x30..=0x3f).contains(c)) {
                    end += 1;
                }
                while bytes.get(end).is_some_and(|c| (0x20..=0x2f).contains(c)) {
                    end += 1;
                }
                if bytes.get(end).is_some_and(|c| (0x40..=0x7e).contains(c)) {
                    end += 1;
                    true
                } else {
                    false
                }
            }
            Some(b']') => {
                while bytes.get(end).is_some_and(|c| *c != 7 && *c != 0x1b) {
                    end += 1;
                }
                if bytes.get(end) == Some(&7) {
                    end += 1;
                    true
                } else if bytes.get(end..end + 2) == Some(b"\x1b\\") {
                    end += 2;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if matched {
            output.push_str(&text[copied..at]);
            copied = end;
            at = end;
        } else {
            at += 1;
        }
    }
    output.push_str(&text[copied..]);
    output
}

fn section(line: &str) -> Option<&str> {
    let name = line.strip_suffix(':')?;
    (name.as_bytes().first()?.is_ascii_alphabetic()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphabetic() || b" /-".contains(&c)))
    .then_some(name)
}

fn command(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("  ")?;
    let end = rest.find(' ').unwrap_or(rest.len());
    let name = &rest[..end];
    (name.as_bytes().first()?.is_ascii_lowercase()
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && (end == rest.len() || rest[end..].starts_with("  ")))
    .then_some(name)
}

fn flag(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("  ")?;
    let rest = rest.strip_prefix("    ").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let rest = if bytes.len() >= 4
        && bytes[0] == b'-'
        && (bytes[1].is_ascii_alphanumeric() || bytes[1] == b'?')
        && &bytes[2..4] == b", "
    {
        &rest[4..]
    } else {
        rest
    };
    let name = rest.strip_prefix("--")?;
    if !name.as_bytes().first()?.is_ascii_alphabetic() {
        return None;
    }
    let end = name
        .bytes()
        .position(|c| !c.is_ascii_alphanumeric() && c != b'-')
        .unwrap_or(name.len());
    (end == name.len() || b" =[".contains(&name.as_bytes()[end])).then_some(&rest[..end + 2])
}

pub(super) fn parse(data: &[u8], route: &[&str]) -> Result<Help> {
    let text = strip_ansi(std::str::from_utf8(data)?);
    require(
        text.ends_with('\n')
            && !text.contains('\x1b')
            && !text.chars().any(|c| c < ' ' && !"\n\r\t".contains(c)),
        "invalid_worker_help",
    )?;
    let mut usage = Vec::new();
    let mut sections: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut current = None;
    // Python splitlines also recognizes these separators after the control gate.
    for line in text.split(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) {
        if let Some(rest) = line
            .strip_prefix("Usage:")
            .filter(|rest| rest.chars().next().is_some_and(char::is_whitespace))
        {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                usage.push(rest);
                current = None;
                continue;
            }
        }
        if let Some(name) = section(line) {
            require(!sections.contains_key(name), "invalid_worker_help")?;
            sections.insert(name, Vec::new());
            current = Some(name);
        } else if let Some(name) = current {
            sections
                .get_mut(name)
                .ok_or(Error("invalid_worker_help"))?
                .push(line);
        }
    }
    require(
        usage.len() == 1 && (sections.contains_key("Commands") || sections.contains_key("Options")),
        "invalid_worker_help",
    )?;
    let tokens: Vec<_> = usage[0].split_whitespace().collect();
    require(
        tokens.len() > route.len()
            && tokens[0]
                .chars()
                .all(|c| c.is_alphanumeric() || "_./-".contains(c))
            && tokens[1..1 + route.len()] == *route
            && tokens[1 + route.len()..]
                .iter()
                .all(|token| token.starts_with(['[', '<']) || matches!(*token, "..." | "|")),
        "invalid_worker_help",
    )?;
    let mut commands = BTreeSet::new();
    let mut flags = BTreeSet::new();
    for (name, entries) in &sections {
        if !matches!(*name, "Commands" | "Options") {
            continue;
        }
        for line in entries {
            if line.trim().is_empty() {
                continue;
            }
            require(line.starts_with("  "), "invalid_worker_help")?;
            let declaration = if *name == "Commands" {
                command(line)
            } else {
                flag(line)
            };
            if let Some(declaration) = declaration {
                let declarations = if *name == "Commands" {
                    &mut commands
                } else {
                    &mut flags
                };
                // Every accepted surface must fit either report renderer. Cap
                // distinct declarations before growing the set or owning text.
                require(
                    declarations.len() < crate::values::MAX_ITEMS
                        || declarations.contains(declaration),
                    "invalid_worker_help",
                )?;
                declarations.insert(declaration);
            } else {
                require(line.starts_with("    "), "invalid_worker_help")?;
            }
        }
        require(
            if *name == "Commands" {
                !commands.is_empty()
            } else {
                !flags.is_empty()
            },
            "invalid_worker_help",
        )?;
    }
    Ok(Help {
        commands: commands.into_iter().map(str::to_owned).collect(),
        flags: flags.into_iter().map(str::to_owned).collect(),
    })
}

pub(super) fn version(data: &[u8]) -> Result<String> {
    let text = strip_ansi(std::str::from_utf8(data)?);
    let version = text.trim();
    require(version.chars().count() <= 256, "invalid_worker_version")?;
    let mut number = version;
    if let Some(rest) = number
        .strip_prefix("djinn")
        .filter(|rest| rest.chars().next().is_some_and(char::is_whitespace))
    {
        number = rest.trim_start();
    }
    number = number.strip_prefix('v').unwrap_or(number);
    let (number, build) = number
        .split_once('+')
        .map_or((number, None), |(a, b)| (a, Some(b)));
    let (number, prerelease) = number
        .split_once('-')
        .map_or((number, None), |(a, b)| (a, Some(b)));
    let digits: Vec<_> = number.split('.').collect();
    require(
        digits.len() == 3
            && digits
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
            && [build, prerelease].into_iter().flatten().all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
            }),
        "invalid_worker_version",
    )?;
    Ok(version.to_owned())
}
