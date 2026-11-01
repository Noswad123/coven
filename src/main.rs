use clap::builder::TypedValueParser;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use coven::{
    Result, actions, archive, checkpoint_workspace, collection, execution, storage::Delivery, tasks,
};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "coven",
    version,
    about = "Native migration preview: evidence capture and historical authority readers; Python coven remains the default"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    Inspect {
        #[arg(default_value = ".", value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, required = true)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Task {
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

fn run(cli: Cli) -> Result<(String, u8)> {
    let report = match cli.command {
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
            return Ok((archive::output(&report, json)?, code));
        }
        Command::State {
            command: State::Inspect { workspace, json: _ },
        } => tasks::inspect_json_argument(&workspace),
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
    Ok((report?, 0))
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
    let matches = Cli::command().get_matches_from(arguments);
    validate_checkpoint_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match run(cli) {
        Ok((report, code)) => {
            if writeln!(std::io::stdout().lock(), "{report}").is_ok() {
                std::process::ExitCode::from(code)
            } else {
                std::process::ExitCode::FAILURE
            }
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
}
