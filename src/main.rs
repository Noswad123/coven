use clap::builder::TypedValueParser;
use clap::{Parser, Subcommand};
use coven::{Result, archive, collection, storage::Delivery, tasks};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "coven-rs",
    version,
    about = "Native migration preview: handoff evidence capture and task authority readers; Python coven remains the default"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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

fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
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
}
