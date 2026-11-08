use clap::builder::TypedValueParser;
use clap::{ArgGroup, CommandFactory, FromArgMatches, Parser, Subcommand};
use coven::{
    Result, actions, archive, checkpoint_decisions, checkpoint_guidance, checkpoint_workspace,
    collection, console, djinn_observation, execution, group_commands, init, lead, refresh,
    repositories, runtime, state_commands,
    storage::Delivery,
    task_mutation::{self, TaskOutput},
    tasks, workers, workspace_commands, workspace_commands as workspace,
};
use std::io::Write;
use std::path::PathBuf;
mod cli_operands;

#[derive(Parser)]
#[command(name = "coven", version, about = "Manage multi-agent coven workspaces")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect existing targets or explicitly submit interactive input.
    Session {
        #[command(subcommand)]
        command: Session,
    },
    /// Explicit participant declarations and annotation-only publication.
    Group {
        #[command(subcommand)]
        command: Group,
    },
    /// Print a Scrum-style coven standup summary.
    Standup {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Open the interactive agent console.
    Console {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Set, show, or clear the coven lead.
    #[command(infer_long_args = true)]
    Lead {
        #[arg(num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        clear: Vec<bool>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        message: Vec<String>,
    },
    /// Paste starter prompts into running agent panes.
    Bootstrap {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Print the configured runtime launch plan.
    Plan {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Start the workspace runtime, optionally replacing it first.
    #[command(alias = "start", infer_long_args = true, args_override_self = true)]
    Up {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        replace: bool,
    },
    /// Restart the workspace runtime.
    Restart {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Record a checkpoint approval and notify the selected agents.
    #[command(infer_long_args = true)]
    Approve {
        #[arg(num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        agent: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        message: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    /// Request checkpoint changes and notify the selected agents.
    #[command(infer_long_args = true)]
    Suggest {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        agent: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
    /// Interactively review checkpoint snapshots.
    #[command(infer_long_args = true)]
    Review {
        #[arg(num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        agent: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        message: Vec<String>,
    },
    /// Send literal text to an agent or all agents.
    Send {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    Agent {
        #[command(subcommand)]
        command: Agent,
    },
    /// Stop the workspace runtime without resetting launch consumption.
    #[command(alias = "stop")]
    Down {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
    },
    /// Inspect or pin repository facts; never authorizes Git writes or execution.
    Repository {
        #[command(subcommand)]
        command: Repository,
    },
    /// Inspect adapter compatibility without admitting worker execution.
    Worker {
        #[command(subcommand)]
        command: Worker,
    },
    /// Capture persisted evidence, not live activity or execution authority.
    Observe {
        #[command(subcommand)]
        command: Observe,
    },
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
enum Agent {
    Status {
        #[arg(num_args = 0.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    Start {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    Stop {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    Restart {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
    Send {
        #[arg(required = true, num_args = 1.., allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
    },
}

#[derive(clap::Args)]
struct SessionOutput {
    #[arg(long, default_value = "3", allow_hyphen_values = true)]
    timeout: f64,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct SessionInspection {
    #[arg(allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
    workspace: PathBuf,
    #[arg(long, allow_negative_numbers = true)]
    socket: String,
    #[arg(long, default_value = "djinn", value_parser = ["djinn"])]
    runtime: String,
    #[arg(long, allow_negative_numbers = true)]
    group: Option<String>,
    #[arg(long, allow_negative_numbers = true)]
    member: Option<String>,
    #[arg(long)]
    recognized_only: bool,
    #[arg(long, default_value = "3", allow_hyphen_values = true)]
    timeout: f64,
    #[arg(long, default_value = "1048576", allow_hyphen_values = true)]
    max_response_bytes: i64,
    #[arg(long)]
    json: bool,
}

impl SessionInspection {
    fn request(
        &self,
        pane: Option<String>,
        expected_terminal: Option<String>,
    ) -> Result<coven::session_inspection::Request> {
        Ok(coven::session_inspection::Request {
            workspace: self.workspace.clone(),
            socket: self.socket.clone(),
            pane,
            expected_terminal,
            runtime: self.runtime.clone(),
            timeout: self.timeout,
            max_response_bytes: self
                .max_response_bytes
                .try_into()
                .map_err(|_| coven::Error::invalid_cli_arguments())?,
            group: self.group.clone(),
            member: self.member.clone(),
            recognized_only: self.recognized_only,
        })
    }
}

#[derive(clap::Args)]
struct SessionInteractive {
    #[arg(allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
    workspace: PathBuf,
    #[arg(long, value_parser = ["djinn"])]
    runtime: String,
    #[arg(long, allow_negative_numbers = true)]
    executable: String,
    #[arg(long, allow_negative_numbers = true)]
    socket: String,
    #[arg(long, allow_negative_numbers = true)]
    pane: String,
    #[arg(long, allow_negative_numbers = true)]
    expected_terminal: String,
    #[arg(long)]
    execute: bool,
    #[command(flatten)]
    output: SessionOutput,
}

impl SessionInteractive {
    fn request(
        &self,
        operation: coven::interactive_sessions::Operation,
        session: Option<String>,
    ) -> coven::interactive_sessions::Request {
        coven::interactive_sessions::Request {
            workspace: self.workspace.clone(),
            operation,
            runtime: self.runtime.clone(),
            executable: self.executable.clone(),
            socket: self.socket.clone(),
            pane: self.pane.clone(),
            expected_terminal: self.expected_terminal.clone(),
            session,
            timeout: self.output.timeout,
            execute: self.execute,
        }
    }
}

#[derive(Subcommand)]
enum Session {
    #[command(infer_long_args = true, args_override_self = true)]
    List {
        #[command(flatten)]
        options: SessionInspection,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Inspect {
        #[command(flatten)]
        options: SessionInspection,
        #[arg(long, allow_negative_numbers = true)]
        pane: String,
        #[arg(long, allow_negative_numbers = true)]
        expected_terminal: Option<String>,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Watch {
        #[command(flatten)]
        options: SessionInspection,
        #[arg(long, allow_negative_numbers = true)]
        pane: Option<String>,
        #[arg(long, allow_negative_numbers = true)]
        expected_terminal: Option<String>,
        #[arg(long, default_value = "60", allow_hyphen_values = true)]
        duration: f64,
        #[arg(long, default_value = "2", allow_hyphen_values = true)]
        interval: f64,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Start {
        #[command(flatten)]
        options: SessionInteractive,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Resume {
        #[command(flatten)]
        options: SessionInteractive,
        #[arg(long, allow_negative_numbers = true)]
        session: String,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    PromptCapabilities {
        #[arg(long, allow_negative_numbers = true)]
        native_socket: String,
        #[command(flatten)]
        output: SessionOutput,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Prompt {
        #[arg(allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, allow_negative_numbers = true)]
        native_socket: String,
        #[arg(long, allow_negative_numbers = true)]
        expected_runtime: String,
        #[arg(long, allow_negative_numbers = true)]
        session: String,
        #[arg(long, allow_negative_numbers = true)]
        request_id: String,
        #[arg(long, allow_negative_numbers = true)]
        prompt_file: String,
        #[arg(long)]
        execute: bool,
        #[command(flatten)]
        output: SessionOutput,
    },
}

#[derive(Subcommand)]
enum Repository {
    /// Read frozen registration; optional verification only observes current Git facts.
    #[command(infer_long_args = true, args_override_self = true)]
    Inspect {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        verify: bool,
        /// Emit JSON instead of the human report; the CLI adds one terminal LF.
        #[arg(long)]
        json: bool,
    },
    /// Pin an existing checkout and full commit; no Git mutation or execution permission.
    #[command(infer_long_args = true, args_override_self = true)]
    Register {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        path: PathBuf,
        #[arg(long, allow_negative_numbers = true)]
        base: String,
        #[arg(long, allow_negative_numbers = true)]
        delivery_target: String,
        #[arg(long, allow_negative_numbers = true)]
        expect_repository_revision: String,
        /// Emit JSON instead of the human report; the CLI adds one terminal LF.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Worker {
    /// Help-only fail-closed inspection; never grants admission or execution permission.
    #[command(infer_long_args = true, args_override_self = true)]
    Readiness {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, default_value = "djinn", value_parser = ["djinn"])]
        adapter: String,
        #[arg(long, default_value = "research-readonly", value_parser = ["research-readonly"])]
        profile: String,
        #[arg(long, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        executable: Option<PathBuf>,
        #[arg(long, default_value = "3", allow_hyphen_values = true)]
        timeout: f64,
        #[arg(long, default_value = "131072", allow_hyphen_values = true)]
        max_output_bytes: i64,
        /// Emit JSON instead of the human report; the CLI adds one terminal LF.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Observe {
    /// Observe an explicitly selected Kitsune API endpoint.
    #[command(infer_long_args = true, args_override_self = true)]
    Kitsune {
        #[arg(long, allow_negative_numbers = true)]
        socket: String,
        #[arg(long, allow_negative_numbers = true)]
        process_pane: Option<String>,
        /// Compare reported built-in agent hints with generic Djinn-family evidence.
        #[arg(long, conflicts_with = "process_pane")]
        compare_agents: bool,
        #[arg(long, default_value = "3", allow_hyphen_values = true)]
        timeout: f64,
        #[arg(long, default_value = "1048576", allow_hyphen_values = true)]
        max_response_bytes: i64,
        #[arg(long)]
        json: bool,
    },
    /// Read one explicitly pinned live artifact.
    #[command(infer_long_args = true, args_override_self = true)]
    DjinnLive {
        #[command(flatten)]
        source: LiveSource,
        #[arg(long)]
        json: bool,
    },
    /// Read persisted folder evidence from an explicit trusted producer, never agent work.
    #[command(infer_long_args = true, args_override_self = true)]
    Djinn {
        /// Explicit UTF-8 executable selector, validated by the observation service.
        #[arg(long, allow_negative_numbers = true)]
        executable: String,
        /// Explicit UTF-8 folder selector, not a UI/native session ID.
        #[arg(long, allow_negative_numbers = true)]
        folder: String,
        #[arg(long, default_value = "3", allow_hyphen_values = true)]
        timeout: f64,
        #[arg(long, default_value = "131072", allow_hyphen_values = true)]
        max_output_bytes: i64,
        /// Emit compact ASCII JSON rather than pretty JSON; CLI adds one terminal LF.
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
struct GroupTarget {
    #[arg(allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
    workspace: PathBuf,
    #[arg(allow_negative_numbers = true)]
    group: String,
}

#[derive(clap::Args)]
struct GroupMember {
    #[command(flatten)]
    target: GroupTarget,
    #[arg(long, allow_negative_numbers = true)]
    member: String,
    #[arg(long, allow_negative_numbers = true)]
    expect_revision: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct LiveSource {
    #[arg(long, allow_negative_numbers = true)]
    file: String,
    #[arg(long, allow_negative_numbers = true)]
    runtime: String,
    #[arg(long, allow_negative_numbers = true)]
    instance: String,
    #[arg(long, allow_negative_numbers = true)]
    session: String,
    #[arg(long, default_value = "1", allow_hyphen_values = true)]
    timeout: f64,
}

#[derive(Subcommand)]
enum Group {
    #[command(infer_long_args = true, args_override_self = true)]
    Create {
        #[command(flatten)]
        target: GroupTarget,
        #[arg(long, allow_negative_numbers = true)]
        name: Option<String>,
        #[arg(long, allow_negative_numbers = true)]
        expect_revision: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Add {
        #[command(flatten)]
        member: GroupMember,
        #[arg(long, allow_negative_numbers = true)]
        role: String,
        #[arg(long, allow_negative_numbers = true)]
        socket: String,
        #[arg(long, allow_negative_numbers = true)]
        pane: String,
        #[arg(long)]
        process_evidence: bool,
        #[arg(long)]
        rebind: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Remove {
        #[command(flatten)]
        member: GroupMember,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    List {
        #[arg(allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    AttachLive {
        #[command(flatten)]
        member: GroupMember,
        #[command(flatten)]
        source: LiveSource,
        #[arg(long)]
        reattach: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    DetachLive {
        #[command(flatten)]
        member: GroupMember,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Status {
        #[command(flatten)]
        target: GroupTarget,
        #[arg(long, allow_negative_numbers = true)]
        member: Option<String>,
        #[arg(long, default_value = "1", allow_hyphen_values = true)]
        timeout: f64,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Publish {
        #[command(flatten)]
        target: GroupTarget,
        #[arg(long, allow_negative_numbers = true)]
        socket: String,
        #[arg(long, allow_negative_numbers = true)]
        source: String,
        #[arg(long, default_value = "60", allow_hyphen_values = true)]
        duration: i64,
        #[arg(long)]
        live: bool,
        #[arg(long)]
        json: bool,
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
    #[command(infer_long_args = true, args_override_self = true)]
    EnableRuntimeLaunch {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    PrepareRuntimeLaunch {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Submit {
        #[command(flatten)]
        selector: ActionSelector,
        #[arg(long, allow_negative_numbers = true)]
        expect_manifest: String,
        #[arg(long, allow_negative_numbers = true)]
        expect_checkpoint: String,
        #[arg(long, allow_negative_numbers = true)]
        expect_decision: String,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    AcknowledgeUnknown {
        #[command(flatten)]
        selector: ActionSelector,
        #[arg(long, allow_negative_numbers = true)]
        expect_claim: String,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Stop {
        #[command(flatten)]
        selector: ActionSelector,
        #[arg(long)]
        json: bool,
    },
    /// Inspect captured launch authority and its checkpoint without runtime effects.
    #[command(infer_long_args = true, args_override_self = true)]
    Inspect {
        #[arg(value_name = "WORKSPACE_OR_ACTION_ID", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace_or_action: PathBuf,
        #[arg(value_name = "ACTION_ID", allow_negative_numbers = true)]
        action_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
struct ActionSelector {
    #[arg(value_name = "WORKSPACE_OR_ACTION_ID", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
    workspace_or_action: PathBuf,
    #[arg(value_name = "ACTION_ID", allow_negative_numbers = true)]
    action_id: Option<String>,
}

#[derive(Subcommand)]
enum Execution {
    /// Enable local preparation authority; never dispatches work or starts a runtime.
    #[command(infer_long_args = true)]
    Enable {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
    /// Enroll a task's source for preparation; never dispatches work.
    #[command(infer_long_args = true)]
    Enroll {
        #[arg(value_name = "WORKSPACE_OR_TASK_ID", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_tasks_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        rebind_source: Vec<bool>,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
    /// Upgrade local preparation authority; never dispatches work or starts a runtime.
    #[command(infer_long_args = true)]
    Upgrade {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
    /// Explicitly confirm authority or preserve a pending activation; no source/runtime access.
    #[command(infer_long_args = true, args_override_self = true,
        group(ArgGroup::new("execution_recovery_mode").required(true).multiple(false)
            .args(["confirm_authority", "archive_pending"])))]
    Recover {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        confirm_authority: bool,
        #[arg(long)]
        archive_pending: bool,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_execution_revision: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
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

#[derive(clap::Args)]
struct ExecutionWriteFlags {
    #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
    expect_execution_revision: Vec<String>,
    #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
    request_id: Vec<String>,
    /// Emit JSON instead of the default human-readable preparation report.
    #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
    json: Vec<bool>,
}

impl ExecutionWriteFlags {
    fn borrowed<'a>(&'a self, tasks: &'a [String]) -> execution::WriteOptions<'a> {
        execution::WriteOptions {
            expected_execution_revision: self
                .expect_execution_revision
                .last()
                .map(String::as_str)
                .unwrap_or(""),
            expected_tasks_revision: tasks.last().map(String::as_str),
            request_id: self.request_id.last().map(String::as_str).unwrap_or(""),
            json: !self.json.is_empty(),
        }
    }
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
    #[command(infer_long_args = true, args_override_self = true)]
    List {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Attempt {
    /// Freeze local preparation from an intent or graph; does NOT dispatch or start a process.
    #[command(infer_long_args = true,
        group(ArgGroup::new("preparation_input").required(true).multiple(false)
            .args(["intent", "graph_revision"])))]
    Prepare {
        #[arg(value_name = "WORKSPACE_OR_TASK_ID", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, value_name = "FILE", action = clap::ArgAction::Append, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        intent: Vec<PathBuf>,
        #[arg(long, value_name = "REV", action = clap::ArgAction::Append, allow_negative_numbers = true)]
        graph_revision: Vec<String>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_tasks_revision: Vec<String>,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
    /// Record preparation cancellation; does not stop a process or grant execution permission.
    #[command(infer_long_args = true)]
    Cancel {
        #[arg(value_name = "WORKSPACE_OR_ATTEMPT_ID", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        reason: Vec<String>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_execution_revision: Vec<String>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        request_id: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, num_args = 0, default_missing_value = "true")]
        json: Vec<bool>,
    },
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
    /// Propose a frozen graph document for preparation review; never schedules work.
    #[command(infer_long_args = true)]
    Propose {
        #[arg(default_value = ".", allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        workspace: PathBuf,
        #[arg(long, required = true, value_name = "FILE", action = clap::ArgAction::Append, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        document: Vec<PathBuf>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_tasks_revision: Vec<String>,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
    /// Record graph review for preparation, not permission to dispatch work.
    #[command(infer_long_args = true)]
    Decide {
        #[arg(value_name = "WORKSPACE_OR_GRAPH_REVISION", required = true, num_args = 1..=2, allow_negative_numbers = true, value_parser=clap::builder::OsStringValueParser::new().map(PathBuf::from))]
        items: Vec<PathBuf>,
        #[arg(long, required = true, action = clap::ArgAction::Append, value_parser = ["approve", "changes-requested"])]
        decision: Vec<String>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        reason: Vec<String>,
        #[arg(long, required = true, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_graph_digest: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append, allow_negative_numbers = true)]
        expect_tasks_revision: Vec<String>,
        #[command(flatten)]
        options: ExecutionWriteFlags,
    },
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
    #[command(infer_long_args = true, args_override_self = true)]
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
    #[command(infer_long_args = true, args_override_self = true)]
    Validate {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
    Inspect {
        workspace: PathBuf,
        collection_id: String,
        #[arg(long)]
        json: bool,
    },
    #[command(infer_long_args = true, args_override_self = true)]
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
        Command::Session { command } => {
            return match command {
                Session::List { options } => coven::session_inspection::arguments(
                    &options.request(None, None)?,
                    options.json,
                    false,
                ),
                Session::Inspect {
                    options,
                    pane,
                    expected_terminal,
                } => coven::session_inspection::arguments(
                    &options.request(Some(pane), expected_terminal)?,
                    options.json,
                    true,
                ),
                Session::Watch {
                    options,
                    pane,
                    expected_terminal,
                    duration,
                    interval,
                } => coven::session_watch::arguments(
                    &options.request(pane, expected_terminal)?,
                    duration,
                    interval,
                    options.json,
                    &mut std::io::stdout().lock(),
                ),
                Session::Start { options } => coven::interactive_sessions::arguments(
                    &options.request(coven::interactive_sessions::Operation::Start, None),
                    options.output.json,
                ),
                Session::Resume { options, session } => coven::interactive_sessions::arguments(
                    &options.request(
                        coven::interactive_sessions::Operation::Resume,
                        Some(session),
                    ),
                    options.output.json,
                ),
                Session::PromptCapabilities {
                    native_socket,
                    output,
                } => coven::session_prompts::capabilities_arguments(
                    &native_socket,
                    output.timeout,
                    output.json,
                ),
                Session::Prompt {
                    workspace,
                    native_socket,
                    expected_runtime,
                    session,
                    request_id,
                    prompt_file,
                    execute,
                    output,
                } => coven::session_prompts::prompt_arguments(
                    &coven::session_prompts::Request {
                        workspace,
                        native_socket,
                        expected_runtime,
                        session,
                        request_id,
                        prompt_file,
                        execute,
                        timeout: output.timeout,
                    },
                    output.json,
                ),
            };
        }
        Command::Group { command } => {
            let result = match command {
                Group::Create {
                    target,
                    name,
                    expect_revision,
                    json,
                } => group_commands::create(
                    &target.workspace,
                    &target.group,
                    name.as_deref(),
                    expect_revision.as_deref(),
                    json,
                ),
                Group::List { workspace, json } => group_commands::list(&workspace, json),
                Group::Remove { member } => group_commands::remove(
                    &member.target.workspace,
                    &member.target.group,
                    &member.member,
                    member.expect_revision.as_deref(),
                    member.json,
                ),
                Group::DetachLive { member } => group_commands::detach(
                    &member.target.workspace,
                    &member.target.group,
                    &member.member,
                    member.expect_revision.as_deref(),
                    member.json,
                ),
                Group::Add {
                    member,
                    role,
                    socket,
                    pane,
                    process_evidence,
                    rebind,
                } => group_commands::add(
                    &member.target.workspace,
                    group_commands::AddRequest {
                        group: &member.target.group,
                        member: &member.member,
                        role: &role,
                        socket: &socket,
                        pane: &pane,
                        process_evidence,
                        rebind,
                        expected_revision: member.expect_revision.as_deref(),
                    },
                    member.json,
                ),
                Group::AttachLive {
                    member,
                    source,
                    reattach,
                } => group_commands::attach(
                    &member.target.workspace,
                    coven::group_live::AttachRequest {
                        group: &member.target.group,
                        member: &member.member,
                        file: &source.file,
                        runtime: &source.runtime,
                        instance: &source.instance,
                        session: &source.session,
                        reattach,
                        expected_revision: member.expect_revision.as_deref(),
                        timeout: source.timeout,
                    },
                    member.json,
                ),
                Group::Status {
                    target,
                    member,
                    timeout,
                    json,
                } => group_commands::status(
                    &target.workspace,
                    &target.group,
                    member.as_deref(),
                    timeout,
                    json,
                ),
                Group::Publish {
                    target,
                    socket,
                    source,
                    duration,
                    live,
                    json,
                } => group_commands::publish(
                    &coven::group_publisher::Request {
                        workspace: target.workspace,
                        group: target.group,
                        socket,
                        source,
                        duration: duration
                            .try_into()
                            .map_err(|_| coven::Error::invalid_cli_arguments())?,
                        live,
                    },
                    json,
                ),
            }?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Observe {
            command:
                Observe::Kitsune {
                    socket,
                    process_pane,
                    compare_agents,
                    timeout,
                    max_response_bytes,
                    json,
                },
        } => {
            let max_response_bytes: usize = max_response_bytes
                .try_into()
                .map_err(|_| coven::Error::invalid_cli_arguments())?;
            if compare_agents {
                return coven::kitsune_comparison::arguments(
                    &coven::kitsune_comparison::Request {
                        socket,
                        timeout,
                        max_response_bytes,
                    },
                    json,
                );
            }
            let result = group_commands::observe_kitsune(
                &coven::kitsune_observation::Request {
                    socket,
                    process_pane,
                    timeout,
                    max_response_bytes,
                },
                json,
            )?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Observe {
            command: Observe::DjinnLive { source, json },
        } => {
            let result = group_commands::observe_live(
                &coven::djinn_live_observation::Request {
                    file: source.file,
                    runtime: source.runtime,
                    instance: source.instance,
                    session: source.session,
                    timeout: source.timeout,
                },
                json,
            )?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Standup { workspace } => {
            let result = runtime::standup(&workspace)?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Bootstrap { workspace } => {
            let result = runtime::bootstrap(&workspace)?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Console { workspace } => {
            return Ok(command_output(String::new(), console::run(&workspace)?));
        }
        Command::Lead {
            items,
            clear,
            message,
        } => {
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            let result = lead::command(
                &workspace,
                lead_operation(items, !clear.is_empty())?,
                message.last().map(String::as_str),
            )?;
            return Ok(TaskOutput {
                stdout: result.body,
                warnings: result.warnings,
                exit_code: result.exit_code,
            });
        }
        command @ (Command::Plan { .. } | Command::Up { .. } | Command::Restart { .. }) => {
            let (workspace, action) = match command {
                Command::Plan { workspace } => (workspace, runtime::RuntimeAction::Plan),
                Command::Up {
                    workspace,
                    replace: false,
                } => (workspace, runtime::RuntimeAction::Up),
                Command::Up {
                    workspace,
                    replace: true,
                }
                | Command::Restart { workspace } => (workspace, runtime::RuntimeAction::Restart),
                _ => unreachable!(),
            };
            let result = runtime::lifecycle(&workspace, action)?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Agent { command } if !matches!(command, Agent::Send { .. }) => {
            let (items, action) = match command {
                Agent::Status { items } => (items, None),
                Agent::Start { items } => (items, Some(runtime::AgentAction::Start)),
                Agent::Stop { items } => (items, Some(runtime::AgentAction::Stop)),
                Agent::Restart { items } => (items, Some(runtime::AgentAction::Restart)),
                Agent::Send { .. } => unreachable!(),
            };
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            if action.is_some() && items.is_empty() {
                return Err(coven::Error::invalid_cli_arguments());
            }
            let ids = operand_strings(items)?;
            let result = match action {
                Some(action) => runtime::agent_change(&workspace, &ids, action),
                None => runtime::agent_status(&workspace, &ids),
            }?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Action { command } if !matches!(command, Action::Inspect { .. }) => {
            let result = match command {
                Action::EnableRuntimeLaunch { workspace, json } => {
                    actions::mutation::enable(&workspace, json)
                }
                Action::PrepareRuntimeLaunch { workspace, json } => {
                    actions::mutation::prepare(&workspace, json)
                }
                Action::Submit {
                    selector,
                    expect_manifest,
                    expect_checkpoint,
                    expect_decision,
                    json,
                } => {
                    let (workspace, id) =
                        action_arguments(selector.workspace_or_action, selector.action_id)?;
                    actions::mutation::submit(
                        &workspace,
                        &id,
                        &expect_manifest,
                        &expect_checkpoint,
                        &expect_decision,
                        json,
                    )
                }
                Action::AcknowledgeUnknown {
                    selector,
                    expect_claim,
                    json,
                } => {
                    let (workspace, id) =
                        action_arguments(selector.workspace_or_action, selector.action_id)?;
                    actions::mutation::acknowledge(&workspace, &id, &expect_claim, json)
                }
                Action::Stop { selector, json } => {
                    let (workspace, id) =
                        action_arguments(selector.workspace_or_action, selector.action_id)?;
                    actions::mutation::stop(&workspace, &id, json)
                }
                Action::Inspect { .. } => unreachable!("inspection is handled separately"),
            }?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Approve {
            items,
            agent,
            message,
            expect_revision,
            json,
        } => {
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            let checkpoint = items
                .first()
                .map(|item| operand_text(item))
                .transpose()?
                .unwrap_or("plan-approval");
            let result = checkpoint_decisions::decide(
                &workspace,
                checkpoint_decisions::DecisionRequest {
                    checkpoint: checkpoint.to_owned(),
                    action: checkpoint_decisions::DecisionAction::Approve,
                    notes: message.last().cloned().unwrap_or_default(),
                    source: "cli".into(),
                    agent: agent.last().cloned().unwrap_or_else(|| "all".into()),
                    expected_revision: expect_revision.last().cloned(),
                },
                !json.is_empty(),
            )?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Suggest {
            items,
            agent,
            expect_revision,
            json,
        } => {
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            if items.len() < 2 {
                return Err(coven::Error::invalid_cli_arguments());
            }
            let result = checkpoint_decisions::decide(
                &workspace,
                checkpoint_decisions::DecisionRequest {
                    checkpoint: operand_text(&items[0])?.to_owned(),
                    action: checkpoint_decisions::DecisionAction::Suggest,
                    notes: operand_strings(&items[1..])?.join(" "),
                    source: "cli".into(),
                    agent: agent.last().cloned().unwrap_or_else(|| "all".into()),
                    expected_revision: expect_revision.last().cloned(),
                },
                !json.is_empty(),
            )?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Review {
            items,
            agent,
            message,
        } => {
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            let code = checkpoint_decisions::review(
                &workspace,
                &operand_strings(items)?,
                agent.last().map(String::as_str).unwrap_or("all"),
                message.last().map(String::as_str).unwrap_or(""),
            )?;
            // Review owns interactive stdout; do not synthesize a report or LF.
            return Ok(command_output(String::new(), code));
        }
        Command::Send { items }
        | Command::Agent {
            command: Agent::Send { items },
        } => {
            let (workspace, items) = workspace::split_optional_workspace(&items)?;
            if items.len() < 2 {
                return Err(coven::Error::invalid_cli_arguments());
            }
            let result = runtime::send(
                &workspace,
                operand_text(&items[0])?,
                &operand_strings(&items[1..])?.join(" "),
            )?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Down { workspace } => {
            let result = runtime::down(&workspace)?;
            return Ok(command_output(result.body, result.exit_code));
        }
        Command::Agent { .. } => unreachable!("agent commands are handled above"),
        Command::Repository { command } => {
            return match command {
                Repository::Inspect {
                    workspace,
                    verify,
                    json,
                } => repositories::inspect_arguments(&workspace, verify, json),
                Repository::Register {
                    workspace,
                    path,
                    base,
                    delivery_target,
                    expect_repository_revision,
                    json,
                } => repositories::register_arguments(
                    &workspace,
                    &path,
                    &base,
                    &delivery_target,
                    &expect_repository_revision,
                    json,
                ),
            };
        }
        Command::Worker {
            command:
                Worker::Readiness {
                    workspace,
                    adapter,
                    profile,
                    executable,
                    timeout,
                    max_output_bytes,
                    json,
                },
        } => {
            return workers::readiness_arguments(
                &workspace,
                workers::ReadinessOptions {
                    adapter: &adapter,
                    profile: &profile,
                    executable: executable.as_deref(),
                    timeout,
                    max_output_bytes,
                    json,
                },
            );
        }
        Command::Observe {
            command:
                Observe::Djinn {
                    executable,
                    folder,
                    timeout,
                    max_output_bytes,
                    json,
                },
        } => {
            return djinn_observation::arguments(
                &executable,
                &folder,
                timeout,
                max_output_bytes,
                json,
            );
        }
        Command::Execution {
            command: Execution::Enable { workspace, options },
        } => return execution::enable_arguments(&workspace, options.borrowed(&[])),
        Command::Execution {
            command: Execution::Upgrade { workspace, options },
        } => return execution::upgrade_arguments(&workspace, options.borrowed(&[])),
        Command::Execution {
            command:
                Execution::Enroll {
                    items,
                    expect_tasks_revision,
                    rebind_source,
                    options,
                },
        } => {
            return execution::enroll_arguments(
                &items,
                !rebind_source.is_empty(),
                options.borrowed(&expect_tasks_revision),
            );
        }
        Command::Execution {
            command:
                Execution::Recover {
                    workspace,
                    confirm_authority,
                    archive_pending: _,
                    expect_execution_revision,
                    json,
                },
        } => {
            return execution::recover_arguments(
                &workspace,
                if confirm_authority {
                    execution::ExecutionRecoveryMode::ConfirmAuthority
                } else {
                    execution::ExecutionRecoveryMode::ArchivePending
                },
                expect_execution_revision.last().map(String::as_str),
                !json.is_empty(),
            );
        }
        Command::Execution {
            command: Execution::Inspect { workspace, json },
        } => {
            return execution::inspect_arguments(&workspace, !json.is_empty());
        }
        Command::Task {
            command: Task::Attempt { command },
        } => {
            return match command {
                Attempt::Prepare {
                    items,
                    intent,
                    graph_revision,
                    expect_tasks_revision,
                    options,
                } => {
                    let input = match intent.last() {
                        Some(path) => execution::PreparationInput::Intent(path),
                        None => execution::PreparationInput::GraphRevision(
                            graph_revision.last().map(String::as_str).unwrap_or(""),
                        ),
                    };
                    execution::prepare_arguments(
                        &items,
                        input,
                        options.borrowed(&expect_tasks_revision),
                    )
                }
                Attempt::Cancel {
                    items,
                    reason,
                    expect_execution_revision,
                    request_id,
                    json,
                } => execution::cancel_arguments(
                    &items,
                    reason.last().map(String::as_str).unwrap_or(""),
                    expect_execution_revision
                        .last()
                        .map(String::as_str)
                        .unwrap_or(""),
                    request_id.last().map(String::as_str).unwrap_or(""),
                    !json.is_empty(),
                ),
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
                Graph::Propose {
                    workspace,
                    document,
                    expect_tasks_revision,
                    options,
                } => {
                    let path = document
                        .last()
                        .map(PathBuf::as_path)
                        .unwrap_or(std::path::Path::new(""));
                    execution::propose_arguments(
                        &workspace,
                        path,
                        options.borrowed(&expect_tasks_revision),
                    )
                }
                Graph::Decide {
                    items,
                    decision,
                    reason,
                    expect_graph_digest,
                    expect_tasks_revision,
                    options,
                } => execution::decide_arguments(
                    &items,
                    decision.last().map(String::as_str).unwrap_or(""),
                    reason.last().map(String::as_str).unwrap_or(""),
                    expect_graph_digest.last().map(String::as_str).unwrap_or(""),
                    options.borrowed(&expect_tasks_revision),
                ),
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
                    json,
                },
        } => {
            let (workspace, action_id) = action_arguments(workspace_or_action, action_id)?;
            if json {
                actions::inspect_json_argument(&workspace, &action_id)
            } else {
                actions::inspect_cli_pretty(&workspace, &action_id)
            }
        }
        Command::Action { .. } => unreachable!("action mutations are handled above"),
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
            command: Task::List { workspace, json },
        } => tasks::list_argument(&workspace, json),
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

fn command_output(stdout: String, exit_code: u8) -> TaskOutput {
    TaskOutput {
        stdout,
        warnings: Vec::new(),
        exit_code,
    }
}

fn lead_operation(items: &[PathBuf], clear: bool) -> Result<lead::Operation> {
    if clear {
        return Ok(lead::Operation::Clear);
    }
    let Some(first) = items.first() else {
        return Ok(lead::Operation::Show);
    };
    Ok(match operand_text(first)? {
        "status" | "show" => lead::Operation::Show,
        "clear" | "remove" | "unset" => lead::Operation::Clear,
        "set" if items.len() > 1 => lead::Operation::Set(operand_text(&items[1])?.to_owned()),
        agent => lead::Operation::Set(agent.to_owned()),
    })
}

fn operand_text(item: &std::path::Path) -> Result<&str> {
    item.to_str().ok_or_else(coven::Error::non_unicode_operand)
}

fn operand_strings(items: &[PathBuf]) -> Result<Vec<String>> {
    items
        .iter()
        .map(|item| operand_text(item).map(str::to_owned))
        .collect()
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
    if let Some(code) = coven::process_supervisor_entry() {
        return std::process::ExitCode::from(code);
    }
    let arguments: Vec<_> = std::env::args_os().collect();
    let has_terminator = arguments.iter().any(|argument| argument == "--");
    let (command, parse_arguments) = cli_operands::checkpoint_compat(&arguments);
    let matches = command.get_matches_from(&parse_arguments);
    validate_checkpoint_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    validate_execution_option_order(&matches, has_terminator).unwrap_or_else(|error| error.exit());
    validate_task_option_order(&matches, &arguments).unwrap_or_else(|error| error.exit());
    validate_runtime_option_order(&matches, &arguments).unwrap_or_else(|error| error.exit());
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

fn validate_runtime_option_order(
    matches: &clap::ArgMatches,
    arguments: &[std::ffi::OsString],
) -> std::result::Result<(), clap::Error> {
    let Some((name, command)) = matches.subcommand() else {
        return Ok(());
    };
    let flags: &[&str] = match name {
        "approve" => &["agent", "message", "expect_revision", "json"],
        "suggest" => &["agent", "expect_revision", "json"],
        "review" => &["agent", "message"],
        "lead" => &["clear", "message"],
        "send" | "agent" | "down" | "action" | "standup" | "console" | "bootstrap" | "plan"
        | "up" | "restart" => &[],
        _ => return Ok(()),
    };
    if non_operand_number(arguments) {
        return Err(Cli::command().error(
            clap::error::ErrorKind::UnknownArgument,
            "option-like literal values require -- or option=value syntax",
        ));
    }
    // argparse's optional workspace cannot resume at a terminator after a flag.
    if name == "up"
        && arguments.iter().any(|arg| arg == "--")
        && command.value_source("workspace") == Some(clap::parser::ValueSource::CommandLine)
        && command.index_of("workspace").is_some_and(|workspace| {
            command.indices_of("replace").is_some_and(|mut flags| {
                *command.get_one::<bool>("replace").unwrap_or(&false)
                    && flags.any(|flag| flag > workspace)
            })
        })
    {
        return Err(Cli::command().error(
            clap::error::ErrorKind::UnknownArgument,
            "options after the workspace cannot precede a trailing terminator",
        ));
    }
    if !flags.is_empty() {
        validate_flag_operand_group(command, arguments.iter().any(|arg| arg == "--"), flags)?;
    }
    Ok(())
}

fn validate_execution_option_order(
    matches: &clap::ArgMatches,
    has_terminator: bool,
) -> std::result::Result<(), clap::Error> {
    if let Some(enroll) = matches
        .subcommand_matches("execution")
        .and_then(|execution| execution.subcommand_matches("enroll"))
    {
        return validate_flag_operand_group(
            enroll,
            has_terminator,
            &[
                "expect_execution_revision",
                "expect_tasks_revision",
                "request_id",
                "json",
                "rebind_source",
            ],
        );
    }
    let Some(task) = matches.subcommand_matches("task") else {
        return Ok(());
    };
    if let Some(prepare) = task
        .subcommand_matches("attempt")
        .and_then(|attempt| attempt.subcommand_matches("prepare"))
    {
        return validate_flag_operand_group(
            prepare,
            has_terminator,
            &[
                "intent",
                "graph_revision",
                "expect_execution_revision",
                "expect_tasks_revision",
                "request_id",
                "json",
            ],
        );
    }
    if let Some(decide) = task
        .subcommand_matches("graph")
        .and_then(|graph| graph.subcommand_matches("decide"))
    {
        return validate_flag_operand_group(
            decide,
            has_terminator,
            &[
                "decision",
                "reason",
                "expect_graph_digest",
                "expect_execution_revision",
                "expect_tasks_revision",
                "request_id",
                "json",
            ],
        );
    }
    if let Some(cancel) = task
        .subcommand_matches("attempt")
        .and_then(|attempt| attempt.subcommand_matches("cancel"))
    {
        return validate_flag_operand_group(
            cancel,
            has_terminator,
            &["reason", "expect_execution_revision", "request_id", "json"],
        );
    }
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
    validate_flag_operand_group(inspect, has_terminator, &["json"])
}

fn validate_flag_operand_group(
    inspect: &clap::ArgMatches,
    has_terminator: bool,
    names: &[&str],
) -> std::result::Result<(), clap::Error> {
    let Some(items) = inspect.indices_of("items") else {
        return Ok(());
    };
    let items = items.collect::<Vec<_>>();
    for name in names {
        if let Some(flags) = inspect.indices_of(name) {
            for flag in flags {
                let before = items.iter().any(|index| *index < flag);
                let after = items.iter().any(|index| *index > flag);
                if before && (after || has_terminator) {
                    return Err(Cli::command().error(
                    clap::error::ErrorKind::UnknownArgument,
                    "options must precede or follow the operand group, not interrupt it or precede a trailing terminator",
                ));
                }
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
    fn lead_operations_only_decode_selected_operands_and_clear_overrides() {
        for (items, expected) in [
            (vec![], "show"),
            (vec!["show", "ignored"], "show"),
            (vec!["status", "ignored"], "show"),
            (vec!["clear", "ignored"], "clear"),
            (vec!["remove", "ignored"], "clear"),
            (vec!["unset", "ignored"], "clear"),
            (vec!["set"], "set"),
            (vec!["set", "Legacy_ID", "ignored"], "Legacy_ID"),
            (vec!["Legacy_ID", "ignored"], "Legacy_ID"),
        ] {
            let items = items.into_iter().map(PathBuf::from).collect::<Vec<_>>();
            let operation = lead_operation(&items, false).unwrap();
            match operation {
                lead::Operation::Show => assert_eq!(expected, "show"),
                lead::Operation::Clear => assert_eq!(expected, "clear"),
                lead::Operation::Set(id) => assert_eq!(expected, id),
            }
            assert!(matches!(
                lead_operation(&items, true).unwrap(),
                lead::Operation::Clear
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn lead_ignored_bad_utf8_operands_are_not_decoded() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        assert!(matches!(
            lead_operation(std::slice::from_ref(&invalid), true).unwrap(),
            lead::Operation::Clear
        ));
        for first in ["show", "status", "clear", "remove", "unset", "Legacy_ID"] {
            assert!(lead_operation(&[PathBuf::from(first), invalid.clone()], false).is_ok());
        }
        assert_eq!(
            lead_operation(&[PathBuf::from("set"), invalid], false)
                .err()
                .unwrap(),
            coven::Error::non_unicode_operand()
        );
    }

    #[test]
    fn runtime_missing_text_is_refused_before_dispatch() {
        for route in [&["suggest"][..], &["send"], &["agent", "send"]] {
            let args = [&["coven"][..], route, &["one"]].concat();
            let error = run(Cli::try_parse_from(args).unwrap()).err().unwrap();
            assert_eq!(error, coven::Error::invalid_cli_arguments());
        }
    }

    #[test]
    fn runtime_results_keep_literal_bodies_exit_status_and_one_terminal_lf() {
        for code in [0, 1, 2] {
            let body = "{\n  \"literal\": \"caf\u{e9}\"\n}";
            let output = command_output(body.to_owned(), code);
            assert_eq!(output.stdout, body);
            assert_eq!(output.exit_code, code);
            assert!(output.warnings.is_empty());
            let mut bytes = Vec::new();
            write_stdout(&mut bytes, &output.stdout, false).unwrap();
            assert_eq!(bytes, format!("{body}\n").as_bytes());
        }
    }

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
