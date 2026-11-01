//! Read-only composition for Python's optional-workspace checkpoint command.
//! The three ledger observations intentionally are not a consistent snapshot.
//!
//! TOML input is capped at 1 MiB. The cached parser restricts integers to signed
//! i64 (-9223372036854775808..=9223372036854775807) and refuses positive numeric
//! f64 overflow during deserialization. Those refusals use an explicit scalar
//! profile error. Parsed negative overflow, NaN and inf remain allowed in ignored
//! fields, as in Python; runtime fields still require nonblank strings.
use crate::{
    Error, Result, checkpoints,
    error::require,
    ledger, platform, read_fs,
    records::{self, TaskRecords, TeamRecord},
    values::{self, Profile, Value},
};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

// Deliberately mirror coven.py's small control-plane inventories, not tasks'
// authority fences or any recursive artifact inventory.
const DIRECTORIES: &[&str] = &[
    "agents",
    "prompts",
    "state",
    "logs",
    "checkpoints",
    "work",
    "output",
    "tmux",
    "state/task-ledger",
    "state/task-recovery",
    "state/actions",
];
const FILES: &[&str] = &[
    "coven.json",
    "coven.toml",
    "goal.md",
    "spec.md",
    "dashboard.md",
    "state/agents.json",
    "state/tasks.json",
    "state/lead.json",
    "state/.workspace.lock",
    "state/task-authority.json",
    "logs/events.jsonl",
    "logs/messages.jsonl",
    "tmux/coven.toml",
    "work/README.md",
    "output/README.md",
    "state/runtime-launch-control.json",
    "state/.runtime-transport.lock",
];
const MAX_TEXT: usize = 1_048_576;
const MAX_ENTRIES: usize = 4096;
const INPUT_PROFILE: &str = "unsupported_checkpoint_workspace_input_profile";

fn optional_bytes(
    root: &Path,
    relative: &Path,
    limit: usize,
    profile: &'static str,
) -> Result<Option<Vec<u8>>> {
    let path = read_fs::managed(root, relative, false)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let mut input = platform::open_readonly(&path)?;
    require(input.metadata()?.is_file(), "unsafe_archive_path")?;
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    require(bytes.len() <= limit, profile)?;
    Ok(Some(bytes))
}

fn optional_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    optional_bytes(root, Path::new(relative), values::MAX_INPUT, INPUT_PROFILE)?
        .map(|raw| values::parse(&raw, Profile::LegacySnapshot))
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn text_check(root: &Path, relative: &Path) -> Result<()> {
    if let Some(raw) = optional_bytes(root, relative, MAX_TEXT, INPUT_PROFILE)? {
        std::str::from_utf8(&raw)?;
    }
    Ok(())
}

fn checkpoint_scan(root: &Path) -> Result<()> {
    let directory = read_fs::managed(root, Path::new("checkpoints"), true)?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut names = Vec::new();
    // Bound every entry before collecting/sorting, including unrecognized names.
    for (index, entry) in entries.enumerate() {
        require(index < MAX_ENTRIES, "unsupported_checkpoint_scan_profile")?;
        names.push(entry?.file_name());
    }
    let mut names = names
        .into_iter()
        .map(|name| {
            name.into_string()
                .map_err(|_| Error("non_unicode_path_unsupported"))
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        let path = Path::new(&name);
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("md"))
        {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or(Error("invalid_checkpoint_id"))?;
            require(
                !stem.is_empty()
                    && stem
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-')),
                "invalid_checkpoint_id",
            )?;
            text_check(root, &Path::new("checkpoints").join(name))?;
        }
    }
    Ok(())
}

fn check_paths(root: &Path) -> Result<()> {
    for relative in DIRECTORIES {
        read_fs::managed(root, Path::new(relative), true)?;
    }
    read_fs::managed(root, Path::new(ledger::MARKER), false)?;
    let active = ledger::read(root)?.is_some();
    for relative in FILES {
        if active && *relative == "state/tasks.json" {
            continue;
        }
        read_fs::managed(root, Path::new(relative), false)?;
    }
    for relative in [
        "checkpoints/plan-approval.md",
        "checkpoints/pre-delivery-review.md",
    ] {
        text_check(root, Path::new(relative))?;
    }
    checkpoint_scan(root)
}

fn parse_toml(text: &str) -> Result<toml::Value> {
    // toml 0.8 / toml_edit 0.22 deserialize integers only in i64::MIN..=i64::MAX.
    // Their f64 parser also refuses positive numeric overflow (not inf literals).
    // Keep those Python-valid scalars explicit profile refusals, not corruption.
    let config = text.parse::<toml::Value>().map_err(|error| {
        let overflow = match error.message() {
            "number too large to fit in target type" | "number too small to fit in target type" => {
                true
            }
            "invalid floating-point number" => {
                // The same diagnostic covers missing/malformed exponents. Only
                // classify a complete numeric token that actually overflows.
                error
                    .span()
                    .and_then(|span| text.get(span.start..))
                    .and_then(|tail| {
                        tail.split(|c: char| {
                            c.is_ascii_whitespace() || matches!(c, ',' | ']' | '}' | '#')
                        })
                        .next()
                    })
                    .is_some_and(|token| {
                        token
                            .replace('_', "")
                            .parse::<f64>()
                            .is_ok_and(|number| number.is_infinite())
                    })
            }
            _ => false,
        };
        Error(if overflow {
            "unsupported_checkpoint_toml_scalar_profile"
        } else {
            "invalid_workspace_toml"
        })
    })?;
    validate_toml_datetimes(&config)?;
    Ok(config)
}

fn validate_toml_datetimes(value: &toml::Value) -> Result<()> {
    match value {
        toml::Value::Datetime(datetime) => {
            if let Some(date) = datetime.date {
                let leap = date.year % 4 == 0 && (date.year % 100 != 0 || date.year % 400 == 0);
                let days = match date.month {
                    2 if leap => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    _ => 0,
                };
                require(
                    (1..=9999).contains(&date.year) && (1..=days).contains(&date.day),
                    "invalid_workspace_toml",
                )?;
            }
            if let Some(time) = datetime.time {
                // Python datetime rejects second 60, including local times.
                // TOML fractions are valid and may be truncated by the parser;
                // this is deliberately not execution's narrower ISO profile.
                require(
                    time.hour < 24
                        && time.minute < 60
                        && time.second < 60
                        && time.nanosecond < 1_000_000_000,
                    "invalid_workspace_toml",
                )?;
            }
            if let Some(toml::value::Offset::Custom { minutes }) = datetime.offset {
                require((-1439..=1439).contains(&minutes), "invalid_workspace_toml")?;
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                validate_toml_datetimes(value)?;
            }
        }
        toml::Value::Table(table) => {
            for value in table.values() {
                validate_toml_datetimes(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn read_team(root: &Path) -> Result<TeamRecord> {
    let raw = optional_bytes(
        root,
        Path::new("coven.json"),
        values::MAX_INPUT,
        INPUT_PROFILE,
    )?
    .ok_or(Error("invalid_checkpoint_workspace"))?;
    let team = TeamRecord::validate(values::parse(&raw, Profile::LegacySnapshot)?)?;
    let mut effective = team.value().clone();
    if let Some(raw) = optional_bytes(root, Path::new("coven.toml"), MAX_TEXT, INPUT_PROFILE)? {
        let config = parse_toml(std::str::from_utf8(&raw)?)?;
        if let Some(runtime) = config.get("runtime") {
            let runtime = runtime
                .as_table()
                .ok_or(Error("invalid_runtime_configuration"))?;
            for key in ["multiplexer", "opencode_command"] {
                if let Some(value) = runtime.get(key) {
                    let value = value
                        .as_str()
                        .ok_or(Error("invalid_runtime_configuration"))?;
                    let value = records::trimmed(value);
                    require(!value.is_empty(), "invalid_runtime_configuration")?;
                    effective.set_field(key, Value::String(value.to_owned()))?;
                }
            }
        }
    }
    let effective = TeamRecord::validate(effective)?;
    let environment = match std::env::var("COVEN_MULTIPLEXER") {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => None,
        Err(_) => return Err(Error("unsupported_multiplexer")),
    };
    records::multiplexer(
        environment
            .as_deref()
            .or(effective.configured_multiplexer())
            .unwrap_or("herdr"),
    )?;
    crate::actions::validate_workspace_control(root)?;
    Ok(effective)
}

fn inspect_root(
    root: &Path,
    remaining: &[PathBuf],
    #[cfg(test)] observed: &mut dyn FnMut(usize),
) -> Result<Value> {
    check_paths(root)?;
    #[cfg(test)]
    observed(1);
    let team = read_team(root)?;
    check_paths(root)?;
    for agent in team.agent_ids()? {
        for relative in [
            format!("agents/{agent}.md"),
            format!("prompts/{agent}.md"),
            format!("prompts/{agent}.start.txt"),
        ] {
            read_fs::managed(root, Path::new(&relative), false)?;
        }
    }
    #[cfg(test)]
    observed(2);
    let active = ledger::read(root)?.is_some();
    #[cfg(test)]
    observed(3);
    if !active {
        TaskRecords::validate(optional_json(
            root,
            "state/tasks.json",
            Value::Array(Vec::new()),
        )?)?;
    }
    records::agents(&optional_json(
        root,
        "state/agents.json",
        Value::Array(Vec::new()),
    )?)?;
    records::lead(&optional_json(root, "state/lead.json", Value::Null)?)?;
    require(remaining.len() <= 1, "invalid_checkpoint_operands")?;
    let id = remaining
        .first()
        .map(|item| item.to_str().ok_or(Error("non_unicode_path_unsupported")))
        .transpose()?
        .unwrap_or("plan-approval");
    checkpoints::inspect_bytes(id, None)?;
    let raw = optional_bytes(
        root,
        &Path::new("checkpoints").join(format!("{id}.md")),
        MAX_TEXT,
        "unsupported_checkpoint_input_profile",
    )?;
    checkpoints::inspect_bytes(id, raw.as_deref())
}

/// Raw variadic CLI operands: only a marked first candidate selects a workspace.
/// No ancestor search, arity inference, or expansion of the checkpoint selector.
pub fn inspect_arguments(items: &[PathBuf]) -> Result<Value> {
    platform::ensure_supported()?;
    let mut remaining = items;
    let mut workspace = None;
    if let Some(first) = items.first() {
        let candidate = platform::workspace_argument(first)?;
        let marker = candidate.join("coven.json");
        if marker.exists() || marker.is_symlink() {
            workspace = Some(candidate);
            remaining = &items[1..];
        }
    }
    let workspace = match workspace {
        Some(workspace) => workspace,
        None => std::env::current_dir().map_err(|_| Error("invalid_checkpoint_workspace"))?,
    };
    let root = workspace
        .canonicalize()
        .map_err(|_| Error("invalid_checkpoint_workspace"))?;
    require(root.is_dir(), "invalid_checkpoint_workspace")?;
    let value = inspect_root(
        &root,
        remaining,
        #[cfg(test)]
        &mut |_| {},
    )?;
    // Both public APIs enforce the native compact-output profile.
    value.compact()?;
    Ok(value)
}

/// Complete compact and ASCII serialization before the caller writes stdout.
pub fn inspect_json_arguments(items: &[PathBuf]) -> Result<String> {
    let compact = inspect_arguments(items)?.compact()?;
    crate::output::ascii_json(std::str::from_utf8(&compact)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("coven.json"),
            br#"{"name":"test","agents":["operator"]}"#,
        )
        .unwrap();
        root
    }
    fn inspect(root: &Path, items: &[PathBuf]) -> Result<Value> {
        inspect_root(root, items, &mut |_| {})
    }

    #[test]
    fn toml_scalar_corpus_matches_independent_python_oracle_recursively() {
        use std::io::Write;
        let valid = [
            "0",
            "-0",
            "+0",
            "1_000",
            "9223372036854775807",
            "-9223372036854775808",
            "0x7fffffffffffffff",
            "0o777777777777777777777",
            "0b111_000",
            "0x0000",
            "0o00",
            "0b00",
            "1.0",
            "-0.0",
            "1e+9",
            "1e-999",
            "1.7976931348623157e308",
            "-1.7976931348623157e308",
            "-1e999",
            "1_2.3_4e5_6",
            "nan",
            "+nan",
            "-nan",
            "inf",
            "+inf",
            "-inf",
            "0001-01-01",
            "9999-12-31",
            "2000-02-29",
            "2024-02-29",
            "1900-02-28",
            "2026-04-30",
            "00:00:00",
            "23:59:59",
            "12:34:56.1",
            "12:34:56.123456789123456789",
            "2026-10-31T23:59:59Z",
            "2026-10-31t23:59:59z",
            "2026-10-31 23:59:59.123456789123",
            "2026-10-31T23:59:59.123456789Z",
            "2026-10-31T23:59:59+23:59",
            "2026-10-31T23:59:59-23:59",
            "0001-01-01T00:00:00+23:59",
            "9999-12-31T23:59:59-23:59",
            "2026-10-31T23:59:59-00:00",
        ];
        let invalid = [
            "00",
            "01",
            "-01",
            "+01",
            "0_0",
            "0Xff",
            "0O77",
            "0B11",
            "+0xff",
            "-0xff",
            "0x",
            "0xg",
            "0x1g",
            "0o8",
            "0b2",
            "1_",
            "1__0",
            "1_e2",
            "1e",
            "1e+",
            "1e_2",
            "1e+_2",
            "1e++2",
            "1e--2",
            "1.",
            ".1",
            "1._0",
            "1.0_",
            "1e999_",
            "1e999junk",
            "1e999.0",
            "NaN",
            "Infinity",
            "INF",
            "True",
            "null",
            "0000-01-01",
            "0000-02-29",
            "0000-01-01T00:00:00Z",
            "10000-01-01",
            "2026-00-01",
            "2026-13-01",
            "2026-01-00",
            "2026-01-32",
            "2026-04-31",
            "1900-02-29",
            "2026-02-29",
            "2024-02-30",
            "2026-1-01",
            "2026-01-1",
            "24:00:00",
            "23:60:00",
            "23:59:60",
            "23:59:61",
            "1:00:00",
            "12:34",
            "12:34:56.",
            "12:34:56.1_2",
            "12:34:56Z",
            "12:34:56+00:00",
            "2026-10-31T23:59:60Z",
            "2026-10-31T23:59:60.123+00:00",
            "2026-10-31T24:00:00Z",
            "2026-10-31T23:60:00Z",
            "2026-10-31T23:59:59+24:00",
            "2026-10-31T23:59:59-24:00",
            "2026-10-31T23:59:59+00:60",
            "2026-10-31T23:59:59+0:00",
            "2026-10-31T23:59:59+0000",
            "2026-10-31T23:59:59+00:00:00",
            "2026-10-31Z",
            "2026-10-31TT23:59:59Z",
            "2026-10-31\t23:59:59Z",
            "2026-10-31T23:59:59ZZ",
        ];
        let overflow = [
            "9223372036854775808",
            "+9223372036854775808",
            "-9223372036854775809",
            "9_223_372_036_854_775_808",
            "9999999999999999999999999999999999999999",
            "0x8000000000000000",
            "0xffffffffffffffff",
            "0o1000000000000000000000",
            "0b1000000000000000000000000000000000000000000000000000000000000000",
            "1e999",
            "+1e999",
            "1.7976931348623159e308",
            "9_9.9_9e9_9_9",
        ];
        let mut documents = Vec::new();
        let mut expected = Vec::new();
        for (scalars, code) in [
            (valid.as_slice(), None),
            (invalid.as_slice(), Some("invalid_workspace_toml")),
            (
                overflow.as_slice(),
                Some("unsupported_checkpoint_toml_scalar_profile"),
            ),
        ] {
            for scalar in scalars {
                for document in [
                    format!("x={scalar}\n"),
                    format!("x=[{scalar}]\n"),
                    format!("x={{child={scalar}}}\n"),
                    format!("[[x]]\nchild=[{{value={scalar}}}]\n"),
                ] {
                    let result = parse_toml(&document);
                    assert_eq!(
                        result.as_ref().err().map(|error| error.code()),
                        code,
                        "{document}: {result:?}"
                    );
                    documents.push(document);
                    expected.push(code != Some("invalid_workspace_toml"));
                }
            }
        }
        let mut child = std::process::Command::new("python3")
            .args(["-c", "import json,sys,tomllib\ndef accepts(text):\n try: tomllib.loads(text); return True\n except (ValueError,OverflowError): return False\njson.dump([accepts(text) for text in json.load(sys.stdin)],sys.stdout)"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
            .spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&documents).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let actual: Vec<bool> = serde_json::from_slice(&output.stdout).unwrap();
        for ((document, actual), expected) in documents.iter().zip(actual).zip(expected) {
            assert_eq!(actual, expected, "Python TOML oracle: {document}");
        }
    }

    #[test]
    fn toml_runtime_fields_use_strings_and_python_nonblank_semantics_after_scalar_validation() {
        let root = minimal();
        let cases = [
            ("[runtime]\nopencode_command=' oc '", None),
            ("[runtime]\nmultiplexer=' TMUX '", None),
            (
                "[runtime]\nopencode_command='\u{a0}'",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nopencode_command='  '",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nmultiplexer='  '",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nopencode_command=12:34:56.123456789",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nmultiplexer=nan",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nopencode_command=inf",
                Some("invalid_runtime_configuration"),
            ),
            (
                "[runtime]\nopencode_command=9223372036854775808",
                Some("unsupported_checkpoint_toml_scalar_profile"),
            ),
            (
                "[runtime]\nopencode_command=23:59:60",
                Some("invalid_workspace_toml"),
            ),
            (
                "[runtime]\nmultiplexer='wrong'",
                Some("unsupported_multiplexer"),
            ),
            ("runtime=[]", Some("invalid_runtime_configuration")),
            (
                "runtime={opencode_command=false}",
                Some("invalid_runtime_configuration"),
            ),
            (
                "runtime={opencode_command='oc',ignored=[{date=0000-01-01}]}",
                Some("invalid_workspace_toml"),
            ),
        ];
        for (text, code) in cases {
            fs::write(root.path().join("coven.toml"), text).unwrap();
            assert_eq!(
                read_team(root.path()).err().map(|error| error.code()),
                code,
                "{text}"
            );
            let output = std::process::Command::new("python3")
                .args(["-c", "import contextlib,io,sys;from pathlib import Path;from runner import coven\ntry:\n with contextlib.redirect_stderr(io.StringIO()): coven.read_team_config(Path(sys.argv[1]))\nexcept (SystemExit,ValueError,OverflowError): sys.exit(1)"])
                .arg(root.path()).env("PYTHONDONTWRITEBYTECODE", "1").env("COVEN_MULTIPLEXER", "")
                .current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
            assert_eq!(
                output.status.success(),
                code.is_none(),
                "Python runtime oracle: {text}"
            );
        }
    }

    #[test]
    fn inventories_match_python_constants_exactly() {
        let output = std::process::Command::new("python3")
            .args(["-c", "import json;from runner import coven;print(json.dumps([coven.MANAGED_DIRECTORIES,coven.MANAGED_FILES]))"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Vec<Vec<String>> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(expected[0], DIRECTORIES);
        assert_eq!(expected[1], FILES);
    }

    #[test]
    fn scan_caps_all_entries_before_validation_and_sorts_actual_names() {
        let root = minimal();
        let directory = root.path().join("checkpoints");
        fs::create_dir(&directory).unwrap();
        for index in 0..MAX_ENTRIES {
            fs::write(directory.join(format!("{index}.ignored")), []).unwrap();
        }
        checkpoint_scan(root.path()).unwrap();
        fs::write(directory.join("bad.name.MD"), [0xff]).unwrap();
        assert_eq!(
            checkpoint_scan(root.path()).unwrap_err().code(),
            "unsupported_checkpoint_scan_profile"
        );
        fs::remove_file(directory.join("0.ignored")).unwrap();
        assert_eq!(
            checkpoint_scan(root.path()).unwrap_err().code(),
            "invalid_checkpoint_id"
        );
        fs::remove_file(directory.join("bad.name.MD")).unwrap();
        fs::write(directory.join("a.MD"), [0xff]).unwrap();
        fs::write(directory.join("z.bad.md"), []).unwrap();
        // Remove another unrecognized name to remain exactly at the bound.
        fs::remove_file(directory.join("1.ignored")).unwrap();
        assert_eq!(
            checkpoint_scan(root.path()).unwrap_err().code(),
            "invalid_utf8"
        );
    }

    #[test]
    fn configuration_and_records_precede_operand_and_selector_errors() {
        let root = minimal();
        let excess = [PathBuf::from("../bad"), PathBuf::from("extra")];
        fs::write(
            root.path().join("coven.toml"),
            "[runtime]\nopencode_command = ' '",
        )
        .unwrap();
        assert_eq!(
            inspect(root.path(), &excess).unwrap_err().code(),
            "invalid_runtime_configuration"
        );
        fs::remove_file(root.path().join("coven.toml")).unwrap();
        fs::create_dir(root.path().join("state")).unwrap();
        fs::write(root.path().join("state/agents.json"), "{}").unwrap();
        assert_eq!(
            inspect(root.path(), &excess).unwrap_err().code(),
            "invalid_workspace_record_list"
        );
        fs::remove_file(root.path().join("state/agents.json")).unwrap();
        assert_eq!(
            inspect(root.path(), &excess).unwrap_err().code(),
            "invalid_checkpoint_operands"
        );
        assert_eq!(
            inspect(root.path(), &excess[..1]).unwrap_err().code(),
            "invalid_checkpoint_identifier"
        );
    }

    #[test]
    fn pending_without_marker_uses_snapshot_and_has_no_final_fence() {
        let root = minimal();
        fs::create_dir_all(root.path().join(ledger::DIRECTORY)).unwrap();
        fs::write(root.path().join("state/tasks.json"), "[]").unwrap();
        let mut seen = Vec::new();
        let value = inspect_root(root.path(), &[], &mut |pass| {
            seen.push(pass);
            if pass == 3 {
                fs::write(
                    root.path().join(ledger::MARKER),
                    "published after last observation",
                )
                .unwrap();
            }
        })
        .unwrap();
        assert_eq!(seen, [1, 2, 3]);
        assert_eq!(value.as_object().unwrap().entries().len(), 10);
        fs::remove_file(root.path().join(ledger::MARKER)).unwrap();
        fs::write(root.path().join("state/tasks.json"), "{}").unwrap();
        assert_eq!(
            inspect(root.path(), &[]).unwrap_err().code(),
            "invalid_workspace_record_list"
        );
    }

    #[test]
    fn preflight_is_text_only_and_missing_is_not_empty() {
        let root = minimal();
        let missing = inspect(root.path(), &[]).unwrap();
        assert!(matches!(
            missing.as_object().unwrap().get("content"),
            Some(Value::Null)
        ));
        fs::create_dir(root.path().join("checkpoints")).unwrap();
        fs::write(root.path().join("checkpoints/plan-approval.md"), []).unwrap();
        // This is invalid decision grammar, but is not the selected checkpoint.
        fs::write(
            root.path().join("checkpoints/other.MD"),
            "<!-- coven-checkpoint-bad -->",
        )
        .unwrap();
        let empty = inspect(root.path(), &[]).unwrap();
        assert_eq!(
            empty.as_object().unwrap().get("content").unwrap().as_str(),
            Some("")
        );
        assert_ne!(missing.compact().unwrap(), empty.compact().unwrap());
        fs::write(
            root.path().join("checkpoints/other.MD"),
            vec![b'x'; MAX_TEXT + 1],
        )
        .unwrap();
        assert_eq!(inspect(root.path(), &[]).unwrap_err().code(), INPUT_PROFILE);
        assert_eq!(
            optional_bytes(
                root.path(),
                Path::new("checkpoints/other.MD"),
                MAX_TEXT,
                "unsupported_checkpoint_input_profile"
            )
            .unwrap_err()
            .code(),
            "unsupported_checkpoint_input_profile"
        );
    }

    #[test]
    #[cfg(unix)]
    fn non_unicode_selector_is_not_decoded_until_after_preflight() {
        use std::os::unix::ffi::OsStringExt;
        let root = minimal();
        let items = [PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]))];
        fs::write(root.path().join("coven.json"), "null").unwrap();
        assert_eq!(
            inspect(root.path(), &items).unwrap_err().code(),
            "invalid_team_record"
        );
        fs::write(
            root.path().join("coven.json"),
            br#"{"name":"test","agents":["operator"]}"#,
        )
        .unwrap();
        assert_eq!(
            inspect(root.path(), &items).unwrap_err().code(),
            "non_unicode_path_unsupported"
        );
    }

    #[cfg(unix)]
    fn imported() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let output = std::process::Command::new("python3")
            .args(["-c", "from runner import coven;import sys;from pathlib import Path;w=sys.argv[1];coven.main(['init',w,'--blank','--agents','operator']);r=coven.read_task_snapshot(Path(w))[0];coven.main(['state','import-tasks',w,'--expect-revision',r,'--json'])"])
            .arg(&root).env("PYTHONDONTWRITEBYTECODE", "1").env("COVEN_MULTIPLEXER", "")
            .current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (temp, root)
    }

    #[test]
    #[cfg(unix)]
    fn ledger_observations_select_sources_independently_and_never_read_projection_health() {
        use std::os::unix::ffi::OsStrExt;
        let (_temp, root) = imported();
        let marker = fs::read(root.join(ledger::MARKER)).unwrap();
        let projection = root.join("state/tasks.json");
        fs::remove_file(&projection).unwrap();
        let name = std::ffi::CString::new(projection.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        inspect(&root, &[]).unwrap();
        fs::remove_file(&projection).unwrap();
        fs::write(&projection, "{}").unwrap();
        // First active, second inactive, third active: the invalid snapshot must
        // not be parsed, nor compared with an earlier authority observation.
        inspect_root(&root, &[], &mut |pass| match pass {
            1 => fs::remove_file(root.join(ledger::MARKER)).unwrap(),
            2 => fs::write(root.join(ledger::MARKER), &marker).unwrap(),
            _ => {}
        })
        .unwrap();
        // The third observation, not the first or second, selects the snapshot.
        assert_eq!(
            inspect_root(&root, &[], &mut |pass| {
                if pass == 2 {
                    fs::remove_file(root.join(ledger::MARKER)).unwrap();
                }
            })
            .unwrap_err()
            .code(),
            "invalid_workspace_record_list"
        );
        // A corrupt newly selected ledger cannot fall back to valid snapshot.
        fs::write(&projection, "[]").unwrap();
        assert!(
            inspect_root(&root, &[], &mut |pass| {
                if pass == 1 {
                    fs::write(root.join(ledger::MARKER), "{}").unwrap();
                }
            })
            .is_err()
        );
    }
}
