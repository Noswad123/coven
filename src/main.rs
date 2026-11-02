use clap::builder::TypedValueParser;
use clap::{ArgGroup, CommandFactory, FromArgMatches, Parser, Subcommand};
use coven::{
    Result, actions, archive, checkpoint_workspace, collection, execution, refresh, state_commands,
    storage::Delivery,
    task_mutation::{self, TaskOutput},
    tasks,
};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "coven",
    version,
    about = "Native migration preview: local task commands, evidence capture and authority readers; Python coven remains the default"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Regenerate only generated files in an existing workspace; never starts a runtime.
    Refresh {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    Checkpoint {
        #[command(subcommand)]
        command: Checkpoint,
    },
    Action {
        #[command(subcommand)]
        command: Action,
    },
    Execution {
        #[command(subcommand)]
        command: Execution,
    },
    State {
        #[command(subcommand)]
        command: State,
    },
    Task {
        #[command(subcommand)]
        command: Task,
    },
    Handoff {
        #[command(subcommand)]
        command: Handoff,
    },
}

#[derive(Subcommand)]
enum Checkpoint {
    /// Inspect a checkpoint after ordered workspace validation; no approvals or writes.
    #[command(infer_long_args = true)]
    Inspect {
        #[arg(value_name = "WORKSPACE_OR_CHECKPOINT", num_args = 0.., value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, required = true)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Action {
    /// Inspect captured launch authority and its checkpoint without runtime effects.
    Inspect {
        #[arg(value_name = "WORKSPACE_OR_ACTION_ID", value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace_or_action: PathBuf,
        #[arg(value_name = "ACTION_ID")]
        action_id: Option<String>,
        #[arg(long, required = true)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Execution {
    /// Replay retained v1/v2 preparation history, not current-source or durability assessment.
    History {
        #[arg(default_value = ".", value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, required = true)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum State {
    #[command(infer_long_args = true, args_override_self = true)]
    Inspect {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    ImportTasks {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, allow_negative_numbers = true)]
        expect_revision: String,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true,
        group(ArgGroup::new("recovery_mode").required(true).multiple(false)
            .args(["repair_projection", "resume_import", "abort_import"])))]
    RecoverTasks {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        repair_projection: bool,
        #[arg(long)]
        resume_import: bool,
        #[arg(long)]
        abort_import: bool,
        #[arg(long, allow_negative_numbers = true)]
        expect_revision: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Task {
    #[command(infer_long_args = true)]
    Add {
        #[arg(value_name = "WORKSPACE_OR_TITLE", required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        owner: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        notes: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        expect_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        request_id: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    #[command(infer_long_args = true)]
    Assign {
        #[arg(value_name = "WORKSPACE_OR_SELECTOR", required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        expect_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 1, allow_negative_numbers = true)]
        request_id: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    List {
        #[arg(default_value = ".", value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, required = true)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Handoff {
    Collect {
        workspace: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect_sha256: String,
        #[arg(long)]
        expect_tasks_revision: String,
        #[arg(long)]
        json: bool,
    },
    Validate {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Inspect {
        workspace: PathBuf,
        collection_id: String,
        #[arg(long)]
        json: bool,
    },
    List {
        workspace: PathBuf,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

fn run(cli: Cli) -> Result<TaskOutput> {
    let report = match cli.command {
        Command::Refresh { workspace } => return refresh::arguments(&workspace),
        Command::State {
            command:
                State::ImportTasks {
                    workspace,
                    expect_revision,
                    json,
                },
        } => {
            return state_commands::import_arguments(&workspace, &expect_revision, json);
        }
        Command::State {
            command:
                State::RecoverTasks {
                    workspace,
                    repair_projection,
                    resume_import,
                    abort_import: _,
                    expect_revision,
                    json,
                },
        } => {
            let mode = if repair_projection {
                state_commands::RecoveryMode::RepairProjection
            } else if resume_import {
                state_commands::RecoveryMode::ResumeImport
            } else {
                state_commands::RecoveryMode::AbortImport
            };
            return state_commands::recover_arguments(
                &workspace,
                mode,
                expect_revision.as_deref(),
                json,
            );
        }
        Command::Task {
            command:
                Task::Add {
                    items,
                    owner,
                    notes,
                    expect_revision,
                    request_id,
                    json,
                },
        } => {
            let result = task_mutation::add_arguments(
                &items,
                owner.last().map(String::as_str),
                notes.last().map(String::as_str).unwrap_or(""),
                expect_revision.last().map(String::as_str),
                request_id.last().map(String::as_str),
            )?;
            return task_mutation::render_result(&result, !json.is_empty(), "add");
        }
        Command::Task {
            command:
                Task::Assign {
                    items,
                    expect_revision,
                    request_id,
                    json,
                },
        } => {
            let result = task_mutation::assign_arguments(
                &items,
                expect_revision.last().map(String::as_str),
                request_id.last().map(String::as_str),
            )?;
            return task_mutation::render_result(&result, !json.is_empty(), "assign");
        }
        Command::Checkpoint {
            command: Checkpoint::Inspect { items, json: _ },
        } => checkpoint_workspace::inspect_json_arguments(&items),
        Command::Action {
            command:
                Action::Inspect {
                    workspace_or_action,
                    action_id,
                    json: _,
                },
        } => {
            let (workspace, action_id) = action_arguments(workspace_or_action, action_id)?;
            actions::inspect_json_argument(&workspace, &action_id)
        }
        Command::Execution {
            command: Execution::History { workspace, json: _ },
        } => execution::history_json_argument(&workspace),
        Command::Handoff {
            command:
                Handoff::Collect {
                    workspace,
                    file,
                    expect_sha256,
                    expect_tasks_revision,
                    json,
                },
        } => {
            let report =
                collection::collect(&workspace, &file, &expect_sha256, &expect_tasks_revision)?;
            let code = collection_exit(report.delivery());
            return Ok(TaskOutput {
                stdout: archive::output(&report, json)?,
                warnings: Vec::new(),
                exit_code: code,
            });
        }
        Command::State {
            command: State::Inspect { workspace, json },
        } => return state_commands::inspect_arguments(&workspace, json),
        Command::Task {
            command: Task::List { workspace, json: _ },
        } => tasks::list_json_argument(&workspace),
        Command::Handoff {
            command: Handoff::Validate { file, json },
        } => archive::output(&archive::validate_file(&file)?, json),
        Command::Handoff {
            command:
                Handoff::Inspect {
                    workspace,
                    collection_id,
                    json,
                },
        } => archive::output(&archive::inspect(&workspace, &collection_id)?, json),
        Command::Handoff {
            command:
                Handoff::List {
                    workspace,
                    task,
                    json,
                },
        } => archive::output(&archive::list(&workspace, task.as_deref())?, json),
    };
    Ok(TaskOutput {
        stdout: report?,
        warnings: Vec::new(),
        exit_code: 0,
    })
}

fn action_arguments(first: PathBuf, action_id: Option<String>) -> Result<(PathBuf, String)> {
    match action_id {
        Some(id) => Ok((first, id)),
        None => Ok((
            PathBuf::from("."),
            std::str::from_utf8(first.as_os_str().as_encoded_bytes())?.to_owned(),
        )),
    }
}

fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args_os().collect();
    let has_terminator = arguments.iter().any(|argument| argument == "--");
    let matches = Cli::command().get_matches_from(&arguments);
    validate_checkpoint_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    validate_task_option_order(&matches, &arguments).unwrap_or_else(|error| error.exit());
    if (matches.subcommand_matches("state").is_some()
        || matches.subcommand_matches("refresh").is_some())
        && non_operand_number(&arguments)
    {
        Cli::command()
            .error(
                clap::error::ErrorKind::UnknownArgument,
                "option-like literal values require -- or option=value syntax",
            )
            .exit();
    }
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match run(cli) {
        Ok(output) => {
            if !output.stdout.is_empty()
                && writeln!(std::io::stdout().lock(), "{}", output.stdout).is_err()
            {
                return std::process::ExitCode::FAILURE;
            }
            for warning in output.warnings {
                if writeln!(std::io::stderr().lock(), "{warning}").is_err() {
                    return std::process::ExitCode::FAILURE;
                }
            }
            std::process::ExitCode::from(output.exit_code)
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "[ERROR] {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

// Python's nargs="*" wrapper accepts the flag before or after the operand group,
// but not between operands or before a trailing terminator after operands. Clap
// drops terminators, so retain their presence; parsed indices supply all ordering.
// Semantic operand count and workspace selection still happen after preflight.
fn validate_checkpoint_option_order(
    matches: &clap::ArgMatches,
    has_terminator: bool,
) -> std::result::Result<(), clap::Error> {
    let Some(inspect) = matches
        .subcommand_matches("checkpoint")
        .and_then(|command| command.subcommand_matches("inspect"))
    else {
        return Ok(());
    };
    if let (Some(flag), Some(items)) = (inspect.index_of("json"), inspect.indices_of("items")) {
        let (before, after) = items.fold((false, false), |(before, after), index| {
            (before || index < flag, after || index > flag)
        });
        if before && (after || has_terminator) {
            return Err(Cli::command().error(
                clap::error::ErrorKind::UnknownArgument,
                "checkpoint inspection accepts --json before or after operands, not between them or before a trailing terminator",
            ));
        }
    }
    Ok(())
}

fn non_operand_number(arguments: &[std::ffi::OsString]) -> bool {
    arguments
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| {
            arg.to_str().is_some_and(|value| {
                if !value.starts_with('-')
                    || value.starts_with("--")
                    || value.parse::<f64>().is_err()
                {
                    return false;
                }
                let tail = &value[1..];
                let tail = tail.strip_prefix('.').unwrap_or(tail);
                !tail.as_bytes().first().is_some_and(u8::is_ascii_digit)
            })
        })
}

fn validate_task_option_order(
    matches: &clap::ArgMatches,
    arguments: &[std::ffi::OsString],
) -> std::result::Result<(), clap::Error> {
    let Some(task) = matches.subcommand_matches("task") else {
        return Ok(());
    };
    let (command, flags): (&clap::ArgMatches, &[&str]) =
        if let Some(add) = task.subcommand_matches("add") {
            (
                add,
                &["owner", "notes", "expect_revision", "request_id", "json"],
            )
        } else if let Some(assign) = task.subcommand_matches("assign") {
            (assign, &["expect_revision", "request_id", "json"])
        } else {
            return Ok(());
        };
    let Some(items) = command.indices_of("items") else {
        return Ok(());
    };
    let items: Vec<_> = items.collect();
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        return Ok(());
    };
    let has_terminator = arguments.iter().any(|argument| argument == "--");
    let interrupted = flags.iter().any(|flag| {
        command.indices_of(flag).is_some_and(|indices| {
            indices
                .into_iter()
                .any(|index| index > *first && (index < *last || has_terminator))
        })
    });
    // The supported Python oracle accepts negative digit prefixes, including
    // exponent and trailing-dot forms, but does not treat -inf/-nan as operands.
    if interrupted || non_operand_number(arguments) {
        Err(Cli::command().error(clap::error::ErrorKind::UnknownArgument,
            "task options must precede or follow the operand group; use -- for option-like literals"))
    } else {
        Ok(())
    }
}

fn collection_exit(delivery: Delivery) -> u8 {
    if delivery == Delivery::DurabilityUncertain {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_capture_has_exit_two_not_success_or_rejection() {
        assert_eq!(collection_exit(Delivery::DurabilityUncertain), 2);
        assert_eq!(collection_exit(Delivery::Committed), 0);
    }

    #[test]
    fn action_inspection_operands_do_not_require_a_workspace_marker() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for (operands, workspace) in [
            (vec![id], "."),
            (vec!["", id], ""),
            (vec!["/workspace with spaces", id], "/workspace with spaces"),
        ] {
            let args = [vec!["coven", "action", "inspect"], operands, vec!["--json"]].concat();
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Action {
                command:
                    Action::Inspect {
                        workspace_or_action,
                        action_id,
                        ..
                    },
            } = cli.command
            else {
                panic!("wrong command");
            };
            let (actual_workspace, actual_id) =
                action_arguments(workspace_or_action, action_id).unwrap();
            assert_eq!(actual_workspace, PathBuf::from(workspace));
            assert_eq!(actual_id, id);
        }
    }

    #[test]
    fn checkpoint_parser_defers_arity_but_preserves_option_grouping() {
        for args in [
            vec!["--json"],
            vec!["--json", "workspace", "review"],
            vec!["workspace", "review", "--json"],
            vec!["workspace", "review", "extra", "--json"],
            vec!["--json", "--", "workspace", "-"],
            vec!["--json", "workspace", "--"],
            vec!["--j"],
            vec!["--js", "workspace", "review"],
            vec!["workspace", "--jso"],
            vec!["", "review", "--json"],
        ] {
            let argv = [vec!["coven", "checkpoint", "inspect"], args].concat();
            let has_terminator = argv.contains(&"--");
            let matches = Cli::command().try_get_matches_from(argv).unwrap();
            validate_checkpoint_option_order(&matches, has_terminator).unwrap();
        }
        for args in [
            vec!["workspace", "--json", "review"],
            vec!["workspace", "--json", "--"],
            vec!["workspace", "--j", "--"],
        ] {
            let argv = [vec!["coven", "checkpoint", "inspect"], args].concat();
            let has_terminator = argv.contains(&"--");
            let matches = Cli::command().try_get_matches_from(argv).unwrap();
            assert_eq!(
                validate_checkpoint_option_order(&matches, has_terminator)
                    .unwrap_err()
                    .kind(),
                clap::error::ErrorKind::UnknownArgument
            );
        }
    }

    #[test]
    fn task_parser_retains_repeated_options_and_checks_original_grouping() {
        for operands in [
            vec![
                "workspace",
                "title",
                "--owner",
                "missing",
                "--owner",
                "worker",
                "--json",
                "--json",
            ],
            vec!["--json", "workspace", "-1e3"],
            vec!["workspace", "-1.", "--json"],
            vec!["workspace", "title", "--notes=-inf", "--json"],
            vec!["--json", "--", "workspace", "-inf"],
        ] {
            let argv: Vec<std::ffi::OsString> = [vec!["coven", "task", "add"], operands]
                .concat()
                .into_iter()
                .map(Into::into)
                .collect();
            let matches = Cli::command().try_get_matches_from(&argv).unwrap();
            validate_task_option_order(&matches, &argv).unwrap();
        }
        for operands in [
            vec![
                "workspace",
                "--owner",
                "worker",
                "title",
                "--owner",
                "other",
                "--json",
            ],
            vec!["workspace", "title", "--json", "extra", "--json"],
            vec!["workspace", "-inf", "--json"],
            vec!["workspace", "-nan", "--json"],
        ] {
            let argv: Vec<std::ffi::OsString> = [vec!["coven", "task", "add"], operands]
                .concat()
                .into_iter()
                .map(Into::into)
                .collect();
            if let Ok(matches) = Cli::command().try_get_matches_from(&argv) {
                assert!(validate_task_option_order(&matches, &argv).is_err());
            }
        }
        assert!(Cli::try_parse_from(["coven", "task", "add"]).is_err());
        assert!(Cli::try_parse_from(["coven", "task", "assign"]).is_err());
    }

    #[test]
    fn recovery_parser_requires_one_mode_but_defers_pin_policy_to_service() {
        for args in [
            vec!["coven", "state", "recover-tasks"],
            vec![
                "coven",
                "state",
                "recover-tasks",
                "--repair-projection",
                "--abort-import",
            ],
            vec![
                "coven",
                "state",
                "recover-tasks",
                "--resume-import",
                "--repair-projection",
            ],
            vec!["coven", "state", "import-tasks"],
            vec!["coven", "refresh", "--json"],
            vec!["coven", "init"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        for args in [
            vec!["coven", "state", "recover-tasks", "--resume-import"],
            vec![
                "coven",
                "state",
                "recover-tasks",
                "--abort-import",
                "--expect-revision",
                "bad",
            ],
            vec![
                "coven",
                "state",
                "recover-tasks",
                "--repair-projection",
                "--repair-projection",
            ],
            vec!["coven", "state", "inspect"],
            vec!["coven", "state", "inspect", "", "--json", "--json"],
            vec![
                "coven",
                "state",
                "import-tasks",
                "--expect-revision",
                "tasks-missing:v1",
            ],
        ] {
            Cli::try_parse_from(args).unwrap();
        }
    }
}
