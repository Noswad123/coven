use clap::builder::TypedValueParser;
use clap::{ArgGroup, CommandFactory, FromArgMatches, Parser, Subcommand};
use coven::{
    Result, actions, archive, checkpoint_guidance, checkpoint_workspace, collection, execution,
    init, refresh, state_commands,
    storage::Delivery,
    task_mutation::{self, TaskOutput},
    tasks, workspace_commands,
};
use std::io::Write;
use std::path::PathBuf;
mod cli_operands;

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
    /// Suggest the next local checkpoint/status action; never approves or notifies.
    Next {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Show or replace the local goal; empty text shows rather than clears.
    Goal {
        #[arg(num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    /// Append a local message and event; does not notify or invoke a runtime.
    Message {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    /// Print observed message history; literal text is not terminal-sanitized.
    #[command(infer_long_args = true, args_override_self = true)]
    Messages {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        pretty: bool,
        #[arg(long)]
        raw: bool,
    },
    /// Print observed event history; literal text is not terminal-sanitized.
    #[command(infer_long_args = true, args_override_self = true)]
    Events {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        pretty: bool,
        #[arg(long)]
        raw: bool,
    },
    /// Regenerate and print the local dashboard; this command writes dashboard.md.
    Status {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Initialize a new snapshot workspace; never overwrite or start a runtime.
    #[command(infer_long_args = true, args_override_self = true)]
    Init {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, allow_negative_numbers = true)]
        name: Option<String>,
        #[arg(
            long,
            default_value = "Clippy,Srikanth,Jamal,Kris,Eric",
            allow_negative_numbers = true
        )]
        agents: String,
        #[arg(long, default_value = "oc", allow_negative_numbers = true)]
        opencode_command: String,
        #[arg(long, default_value = "herdr", value_parser = ["herdr", "tmux"])]
        multiplexer: String,
        #[arg(
            long,
            default_value = "interactive-demo",
            allow_negative_numbers = true
        )]
        example: String,
        #[arg(long)]
        blank: bool,
        /// Compatibility no-op; never authorizes a reset or overwrite.
        #[arg(long)]
        force: bool,
    },
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
        #[arg(value_name = "WORKSPACE_OR_CHECKPOINT", num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
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
    /// Inspect retained preparation and assess current source under an existing lock.
    #[command(infer_long_args = true)]
    Inspect {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
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
    Attempt {
        #[command(subcommand)]
        command: Attempt,
    },
    Graph {
        #[command(subcommand)]
        command: Graph,
    },
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
enum Attempt {
    /// List frozen preparations with source diagnostics, never execution permission.
    #[command(infer_long_args = true)]
    List {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        task: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    /// Inspect one preparation; optional workspace precedes the required attempt ID.
    #[command(infer_long_args = true)]
    Inspect {
        #[arg(value_name = "WORKSPACE_OR_ATTEMPT_ID", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
}

#[derive(Subcommand)]
enum Graph {
    /// Inspect a retained graph and current-source assessment; never schedules work.
    #[command(infer_long_args = true)]
    Inspect {
        #[arg(value_name = "WORKSPACE_OR_GRAPH_REVISION", num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    /// Export a fully validated frozen graph without loading current source or locks.
    #[command(infer_long_args = true)]
    Export {
        #[arg(value_name = "WORKSPACE_OR_GRAPH_REVISION", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
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
        Command::Execution {
            command: Execution::Inspect { workspace, json },
        } => {
            return execution::inspect_arguments(&workspace, !json.is_empty());
        }
        Command::Task {
            command: Task::Attempt { command },
        } => {
            return match command {
                Attempt::List {
                    workspace,
                    task,
                    json,
                } => execution::attempt_list_arguments(
                    &workspace,
                    task.last().map(String::as_str),
                    !json.is_empty(),
                ),
                Attempt::Inspect { items, json } => {
                    execution::attempt_inspect_arguments(&items, !json.is_empty())
                }
            };
        }
        Command::Task {
            command: Task::Graph { command },
        } => {
            return match command {
                Graph::Inspect { items, json } => {
                    execution::graph_inspect_arguments(&items, !json.is_empty())
                }
                Graph::Export { items, json } => {
                    execution::graph_export_arguments(&items, !json.is_empty())
                }
            };
        }
        Command::Next { workspace } => return checkpoint_guidance::next_arguments(&workspace),
        Command::Goal { items } => return workspace_commands::goal_arguments(&items),
        Command::Message { items } => return workspace_commands::message_arguments(&items),
        Command::Messages {
            workspace,
            raw,
            pretty: _,
        } => {
            return workspace_commands::history_arguments(
                &workspace,
                workspace_commands::HistoryKind::Messages,
                raw,
            );
        }
        Command::Events {
            workspace,
            raw,
            pretty: _,
        } => {
            return workspace_commands::history_arguments(
                &workspace,
                workspace_commands::HistoryKind::Events,
                raw,
            );
        }
        Command::Status { workspace } => return workspace_commands::status_arguments(&workspace),
        Command::Init {
            workspace,
            name,
            agents,
            opencode_command,
            multiplexer,
            example,
            blank,
            force,
        } => {
            return init::arguments(
                &workspace,
                &init::Options {
                    name,
                    agents,
                    opencode_command,
                    multiplexer,
                    example,
                    blank,
                    force,
                },
            );
        }
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
            command: Checkpoint::Inspect { items, json },
        } => checkpoint_workspace::inspect_text_arguments(&items, !json.is_empty()),
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
    let (command, parse_arguments) = cli_operands::checkpoint_compat(&arguments);
    let matches = command.get_matches_from(&parse_arguments);
    validate_checkpoint_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    validate_execution_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    validate_task_option_order(&matches, &arguments).unwrap_or_else(|error| error.exit());
    if (matches.subcommand_matches("init").is_some()
        || matches.subcommand_matches("state").is_some()
        || matches.subcommand_matches("refresh").is_some()
        || matches.subcommand_matches("goal").is_some()
        || matches.subcommand_matches("message").is_some()
        || matches.subcommand_matches("messages").is_some()
        || matches.subcommand_matches("events").is_some()
        || matches.subcommand_matches("status").is_some()
        || matches.subcommand_matches("next").is_some()
        || matches.subcommand_matches("checkpoint").is_some())
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
    // File/history commands supply complete literal output, including its LF
    // policy. Existing report commands retain their single added terminal LF.
    let literal = matches!(
        &cli.command,
        Command::Goal { .. }
            | Command::Message { .. }
            | Command::Messages { .. }
            | Command::Events { .. }
            | Command::Status { .. }
            | Command::Next { .. }
    );
    match run(cli) {
        Ok(output) => {
            if write_stdout(&mut std::io::stdout().lock(), &output.stdout, literal).is_err() {
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

fn write_stdout(writer: &mut impl Write, text: &str, literal: bool) -> std::io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    writer.write_all(text.as_bytes())?;
    if !literal {
        writer.write_all(b"\n")?;
    }
    // Literal output may have no LF, so a buffered stdout error can otherwise
    // appear only during process cleanup and incorrectly leave a zero exit code.
    writer.flush()
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
    validate_json_operand_group(inspect, has_terminator)
}

fn validate_execution_option_order(
    matches: &clap::ArgMatches,
    has_terminator: bool,
) -> std::result::Result<(), clap::Error> {
    let Some(task) = matches.subcommand_matches("task") else {
        return Ok(());
    };
    let command = task
        .subcommand_matches("attempt")
        .and_then(|attempt| attempt.subcommand_matches("inspect"))
        .or_else(|| {
            task.subcommand_matches("graph").and_then(|graph| {
                graph
                    .subcommand_matches("inspect")
                    .or_else(|| graph.subcommand_matches("export"))
            })
        });
    match command {
        Some(command) => validate_json_operand_group(command, has_terminator),
        None => Ok(()),
    }
}

fn validate_json_operand_group(
    inspect: &clap::ArgMatches,
    has_terminator: bool,
) -> std::result::Result<(), clap::Error> {
    if let (Some(flags), Some(items)) = (inspect.indices_of("json"), inspect.indices_of("items")) {
        let items = items.collect::<Vec<_>>();
        for flag in flags {
            let before = items.iter().any(|index| *index < flag);
            let after = items.iter().any(|index| *index > flag);
            if before && (after || has_terminator) {
                return Err(Cli::command().error(
                    clap::error::ErrorKind::UnknownArgument,
                    "inspection accepts --json before or after operands, not between them or before a trailing terminator",
                ));
            }
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
    #[test]
    fn workspace_parser_keeps_history_flags_and_defers_message_arity() {
        let Cli {
            command: Command::Goal { items },
        } = Cli::try_parse_from(["coven", "goal"]).unwrap()
        else {
            panic!("expected goal");
        };
        assert!(items.is_empty());
        assert!(Cli::try_parse_from(["coven", "message"]).is_err());
        let Cli {
            command: Command::Message { items },
        } = Cli::try_parse_from(["coven", "message", "one"]).unwrap()
        else {
            panic!("expected message");
        };
        assert_eq!(items, [PathBuf::from("one")]);
        let Cli {
            command:
                Command::Messages {
                    workspace,
                    pretty,
                    raw,
                },
        } = Cli::try_parse_from(["coven", "messages", "--ra", "", "--pretty", "--raw"]).unwrap()
        else {
            panic!("expected messages");
        };
        assert_eq!(workspace, PathBuf::from(""));
        assert!(pretty && raw);
        for args in [
            vec!["coven", "goal", "--json"],
            vec!["coven", "message", "--json", "a", "b", "c"],
            vec!["coven", "events", "one", "two"],
            vec!["coven", "status", "--raw"],
            vec!["coven", "lead"],
            vec!["coven", "plan"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn stdout_writer_preserves_literal_empty_and_newline_contracts() {
        for (text, literal, expected) in [
            ("", true, ""),
            ("", false, ""),
            ("tail", true, "tail"),
            ("tail\n", true, "tail\n"),
            ("\n\n", true, "\n\n"),
            ("caf\u{e9}\u{1b}[0m", true, "caf\u{e9}\u{1b}[0m"),
            ("report", false, "report\n"),
        ] {
            let mut bytes = Vec::new();
            write_stdout(&mut bytes, text, literal).unwrap();
            assert_eq!(bytes, expected.as_bytes());
        }
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(write_stdout(&mut Broken, "published", true).is_err());
        assert!(write_stdout(&mut Broken, "", true).is_ok());
        struct DeferredFailure(Vec<u8>);
        impl Write for DeferredFailure {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        let mut deferred = DeferredFailure(Vec::new());
        assert!(write_stdout(&mut deferred, "short tail", true).is_err());
        assert_eq!(deferred.0, b"short tail");
    }

    #[test]
    fn init_parser_retains_defaults_and_every_compatibility_flag() {
        let Cli {
            command:
                Command::Init {
                    workspace,
                    name,
                    agents,
                    opencode_command,
                    multiplexer,
                    example,
                    blank,
                    force,
                },
        } = Cli::try_parse_from(["coven", "init"]).unwrap()
        else {
            panic!("expected init");
        };
        let defaults = init::Options::default();
        assert_eq!(workspace, PathBuf::from("."));
        assert_eq!(name, defaults.name);
        assert_eq!(agents, defaults.agents);
        assert_eq!(opencode_command, defaults.opencode_command);
        assert_eq!(multiplexer, defaults.multiplexer);
        assert_eq!(example, defaults.example);
        assert!(!blank && !force);

        let Cli {
            command:
                Command::Init {
                    workspace,
                    name,
                    agents,
                    opencode_command,
                    multiplexer,
                    example,
                    blank,
                    force,
                },
        } = Cli::try_parse_from([
            "coven",
            "init",
            "--name",
            "old",
            "",
            "--name",
            "new",
            "--ag",
            "Alice,Bob",
            "--opencode-c",
            "oc --model demo",
            "--multi",
            "tmux",
            "--example",
            "unsupported",
            "--blank",
            "--blank",
            "--force",
        ])
        .unwrap()
        else {
            panic!("expected init");
        };
        assert_eq!(workspace, PathBuf::from(""));
        assert_eq!(name.as_deref(), Some("new"));
        assert_eq!(agents, "Alice,Bob");
        assert_eq!(opencode_command, "oc --model demo");
        assert_eq!(multiplexer, "tmux");
        assert_eq!(example, "unsupported");
        assert!(blank && force);
        for args in [
            vec!["coven", "init", "--json"],
            vec!["coven", "init", "--multiplexer", "TMUX"],
            vec!["coven", "init", "one", "two"],
            vec!["coven", "init", "--name"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

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
            vec!["coven", "init", "--json"],
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
