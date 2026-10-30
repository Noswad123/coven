use clap::{Parser, Subcommand};
use coven::{Result, archive};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "coven-rs",
    version,
    about = "Native migration preview: read-only handoff validation/history only; Python coven remains the default"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Handoff {
        #[command(subcommand)]
        command: Handoff,
    },
}

#[derive(Subcommand)]
enum Handoff {
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

fn run(cli: Cli) -> Result<String> {
    match cli.command {
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
    }
}

fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
        Ok(report) => {
            if writeln!(std::io::stdout().lock(), "{report}").is_ok() {
                std::process::ExitCode::SUCCESS
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
