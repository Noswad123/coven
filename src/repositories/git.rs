//! Closed read-only Git grammar. Raw diagnostics and filenames never enter reports.
//! Git is trusted executable code, not authenticated or race-free pinned execution.
//! Shared fingerprints cap executable reads at 1 GiB each, outside the 20-second
//! probe/scan/acceptance budget; total observation wall time can exceed that budget.
use super::{
    record::{self, Filesystem, Frozen, Request},
    *,
};
use crate::{
    Error,
    error::require,
    process::{ProbeRequest, ProbeRunner, ProbeStatus, identity},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 1_048_576;
const GUARDS: &[&str] = &[
    "--no-pager",
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "maintenance.auto=false",
    "-c",
    "gc.auto=0",
    "-c",
    "protocol.allow=never",
];

fn environment() -> BTreeMap<OsString, OsString> {
    [
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/dev/null"),
        ("XDG_CONFIG_HOME", "/dev/null"),
        ("LC_ALL", "C"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_SYSTEM", "/dev/null"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_NO_REPLACE_OBJECTS", "1"),
        ("GIT_NO_LAZY_FETCH", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_ALLOW_PROTOCOL", ""),
        ("GIT_ATTR_NOSYSTEM", "1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
}

// Each variant is a member of the legacy allowlist; no generic argv is accepted.
#[derive(Clone, Copy)]
enum Command<'a> {
    Version,
    Bare,
    Format,
    Head,
    SymbolicHead,
    Config,
    Split,
    Locator(&'a str),
    ObjectType(&'a str),
    Commit(&'a str),
    Tree(&'a str),
    Status,
}
impl Command<'_> {
    fn args(self) -> Result<Vec<OsString>> {
        let args = match self {
            Self::Version => vec!["--version"],
            Self::Bare => vec!["rev-parse", "--is-bare-repository"],
            Self::Format => vec!["rev-parse", "--show-object-format"],
            Self::Head => vec!["rev-parse", "--verify", "--quiet", "HEAD"],
            Self::SymbolicHead => vec!["symbolic-ref", "--quiet", "HEAD"],
            Self::Config => vec![
                "config",
                "--local",
                "--no-includes",
                "--null",
                "--name-only",
                "--list",
            ],
            Self::Split => vec![
                "config",
                "--local",
                "--no-includes",
                "--bool",
                "--get-all",
                "core.splitindex",
            ],
            Self::Locator(flag) => {
                require(
                    matches!(
                        flag,
                        "--show-toplevel" | "--absolute-git-dir" | "--git-common-dir"
                    ),
                    "repository_probe_not_permitted",
                )?;
                vec!["rev-parse", "--path-format=absolute", flag]
            }
            Self::ObjectType(oid) | Self::Commit(oid) | Self::Tree(oid) => {
                require(
                    record::oid(oid, 40) || record::oid(oid, 64),
                    "repository_probe_not_permitted",
                )?;
                vec![
                    "cat-file",
                    match self {
                        Self::ObjectType(_) => "-t",
                        Self::Commit(_) => "commit",
                        _ => "tree",
                    },
                    oid,
                ]
            }
            Self::Status => vec![
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=all",
            ],
        };
        Ok(GUARDS
            .iter()
            .copied()
            .chain(args)
            .map(OsString::from)
            .collect())
    }
}
struct Observation<'a, R> {
    selected: PathBuf,
    executable: PathBuf,
    identity: identity::Identity,
    root: PathBuf,
    deadline: Instant,
    runner: &'a R,
}
impl<R: ProbeRunner> Observation<'_, R> {
    fn new(root: PathBuf, selected: PathBuf, runner: &R) -> Result<Observation<'_, R>> {
        // Preserve symlinks and parent components in caller PATH filesystem order.
        let selected = if selected.is_absolute() {
            selected
        } else {
            std::env::current_dir()?.join(selected)
        };
        let executable = selected.canonicalize()?;
        let identity = identity::fingerprint(&executable)?;
        Ok(Observation {
            selected,
            executable,
            identity,
            root,
            deadline: Instant::now() + Duration::from_secs(20),
            runner,
        })
    }
    fn check_executable(&self) -> Result<()> {
        require(
            identity::alias_matches(&self.selected, &self.executable)
                && identity::metadata(&self.executable)? == self.identity.metadata,
            "repository_git_changed",
        )
    }
    fn confirm_executable(&mut self) -> Result<()> {
        self.remaining()?;
        let started = Instant::now();
        let current = identity::fingerprint(&self.executable)?;
        // Exclude only fingerprint I/O, not final alias/metadata acceptance checks.
        self.deadline = self
            .deadline
            .checked_add(started.elapsed())
            .ok_or(Error("repository_observation_budget"))?;
        require(current == self.identity, "repository_git_changed")?;
        self.check_executable()?;
        self.remaining()?;
        Ok(())
    }
    fn remaining(&self) -> Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        require(!remaining.is_zero(), "repository_observation_budget")?;
        Ok(remaining)
    }
    fn probe(&self, command: Command<'_>) -> Result<Vec<u8>> {
        self.check_executable()?;
        let result = self.runner.run(&ProbeRequest {
            executable: self.executable.clone(),
            arguments: command.args()?,
            cwd: self.root.clone(),
            environment: environment(),
            timeout: self.remaining()?.min(Duration::from_secs(3)),
            max_output_bytes: MAX_OUTPUT,
        });
        self.remaining()?;
        require(
            result.stdout.len().saturating_add(result.stderr.len()) <= MAX_OUTPUT,
            "repository_probe_output_limit",
        )?;
        if matches!(command, Command::Head)
            && result.status == ProbeStatus::Nonzero
            && result.returncode == Some(1)
            && result.stdout.is_empty()
            && result.stderr.is_empty()
        {
            return Ok(Vec::new());
        }
        require(
            result.status == ProbeStatus::Ok && result.returncode == Some(0),
            "repository_git_probe_failed",
        )?;
        Ok(result.stdout)
    }
    fn locator(&self, flag: &str) -> Result<PathBuf> {
        let bytes = self.probe(Command::Locator(flag))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| Error("repository_invalid_locator"))?;
        let text = text
            .strip_suffix('\n')
            .ok_or(Error("repository_invalid_locator"))?;
        require(
            !text.contains('\0') && Path::new(text).is_absolute(),
            "repository_invalid_locator",
        )?;
        Ok(Path::new(text).canonicalize()?)
    }
    fn scan(&self, directory: &Path, packs: bool) -> Result<()> {
        for (index, entry) in std::fs::read_dir(directory)?.enumerate() {
            self.remaining()?;
            require(index < 10_000, "repository_directory_limit")?;
            let entry = entry?;
            // Long s is the only non-ASCII casefold scalar that can occur in
            // these ASCII sentinel words (sharedindex/promisor).
            let name = entry
                .file_name()
                .to_string_lossy()
                .replace('\u{17f}', "s")
                .to_ascii_lowercase();
            if packs {
                require(
                    !name.ends_with(".promisor") && entry.file_type()?.is_file(),
                    "repository_unsupported_pack_storage",
                )?;
            } else {
                require(
                    !name.starts_with("sharedindex."),
                    "repository_unsupported_sharedindex",
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn observe(request: &Request, runner: &impl ProbeRunner) -> Result<(Frozen, Value)> {
    let root = Path::new(&request.path).canonicalize()?;
    let root_identity = record::identity(&root)?;
    let selected = identity::find_executable("git").ok_or(Error("repository_git_missing"))?;
    let mut observation = Observation::new(root.clone(), selected, runner)?;
    let version = observation.probe(Command::Version)?;
    require(
        valid_version(&version),
        "repository_git_version_unsupported",
    )?;
    require(
        observation.probe(Command::Bare)? == b"false\n",
        "repository_bare_unsupported",
    )?;
    let top = observation.locator("--show-toplevel")?;
    require(top == root, "repository_checkout_root_required")?;
    let git_dir = observation.locator("--absolute-git-dir")?;
    let common = observation.locator("--git-common-dir")?;
    let filesystem = Filesystem {
        root: record::identity(&top)?,
        git_dir: record::identity(&git_dir)?,
        common_dir: record::identity(&common)?,
    };
    require(filesystem.root == root_identity, "repository_root_changed")?;
    observation.scan(&git_dir, false)?;
    if common != git_dir {
        observation.scan(&common, false)?;
    }
    let config = observation.probe(Command::Config)?;
    require(
        config.is_empty() || config.ends_with(b"\0"),
        "repository_invalid_config",
    )?;
    for key in config
        .strip_suffix(b"\0")
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|_| !config.is_empty())
    {
        require(
            !key.is_empty() && !key.contains(&b'\n'),
            "repository_invalid_config",
        )?;
        let key = key.to_ascii_lowercase();
        if key == b"core.splitindex" {
            let values = observation.probe(Command::Split)?;
            require(
                !values.is_empty()
                    && values.ends_with(b"\n")
                    && values[..values.len() - 1]
                        .split(|b| *b == b'\n')
                        .all(|v| v == b"false"),
                "repository_unsupported_splitindex",
            )?;
        }
        require(!forbidden_config(&key), "repository_unsupported_config")?;
    }
    let objects = common.join("objects");
    for dir in [&objects, &objects.join("info"), &objects.join("pack")] {
        observation.remaining()?;
        require(
            std::fs::symlink_metadata(dir)?.is_dir(),
            "repository_unsupported_object_directory",
        )?;
    }
    for name in ["alternates", "http-alternates"] {
        match std::fs::symlink_metadata(objects.join("info").join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Error("repository_alternates_unsupported")),
        }
    }
    observation.scan(&objects.join("pack"), true)?;
    let format = observation.probe(Command::Format)?;
    let (fmt, length) = match format.as_slice() {
        b"sha1\n" => ("sha1", 40),
        b"sha256\n" => ("sha256", 64),
        _ => return Err(Error("repository_invalid_object_format")),
    };
    require(
        request.base.len() == length,
        "repository_base_format_mismatch",
    )?;
    require(
        observation.probe(Command::ObjectType(&request.base))? == b"commit\n",
        "repository_base_not_commit",
    )?;
    let commit = observation.probe(Command::Commit(&request.base))?;
    require(
        object_hash("commit", &commit, fmt) == request.base,
        "repository_commit_hash_mismatch",
    )?;
    let first = commit.split(|b| *b == b'\n').next().unwrap_or_default();
    let tree = first
        .strip_prefix(b"tree ")
        .and_then(|s| std::str::from_utf8(s).ok())
        .ok_or(Error("repository_malformed_commit"))?;
    require(record::oid(tree, length), "repository_malformed_commit")?;
    require(
        observation.probe(Command::ObjectType(tree))? == b"tree\n",
        "repository_tree_unavailable",
    )?;
    let raw_tree = observation.probe(Command::Tree(tree))?;
    require(
        object_hash("tree", &raw_tree, fmt) == tree,
        "repository_tree_hash_mismatch",
    )?;
    validate_tree(&raw_tree, length / 2)?;
    let head_oid = observation.probe(Command::Head)?;
    let head = if head_oid.is_empty() {
        let head_ref = observation.probe(Command::SymbolicHead)?;
        let text = std::str::from_utf8(&head_ref)
            .ok()
            .and_then(|s| s.strip_suffix('\n'))
            .ok_or(Error("repository_invalid_head"))?;
        record::validate_target(text)?;
        json!({"status":"unborn","ref":text})
    } else {
        let text = std::str::from_utf8(&head_oid)
            .ok()
            .and_then(|s| s.strip_suffix('\n'))
            .ok_or(Error("repository_invalid_head"))?;
        require(record::oid(text, length), "repository_invalid_head")?;
        require(
            observation.probe(Command::ObjectType(text))? == b"commit\n",
            "repository_head_not_commit",
        )?;
        json!({"status":"commit","commit":text})
    };
    let mut status = status(&observation.probe(Command::Status)?)?;
    require(
        observation.locator("--show-toplevel")? == top
            && observation.locator("--absolute-git-dir")? == git_dir
            && observation.locator("--git-common-dir")? == common
            && Path::new(&request.path).canonicalize()? == root
            && record::identity(&top)? == filesystem.root
            && record::identity(&git_dir)? == filesystem.git_dir
            && record::identity(&common)? == filesystem.common_dir,
        "repository_locators_changed",
    )?;
    observation.confirm_executable()?;
    status["available"] = json!(true);
    status["head"] = head;
    status["base_object_validation"] = json!("commit_and_root_tree_only");
    status["complete_checkout_availability"] = json!("unestablished");
    Ok((
        Frozen {
            canonical_root: crate::read_fs::path_text(&top)?,
            git_dir: crate::read_fs::path_text(&git_dir)?,
            common_dir: crate::read_fs::path_text(&common)?,
            filesystem,
            object_format: fmt.to_owned(),
            base_commit: request.base.clone(),
            base_tree: tree.to_owned(),
            delivery_target: request.delivery_target.clone(),
        },
        status,
    ))
}

fn valid_version(bytes: &[u8]) -> bool {
    let Some(text) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.strip_prefix("git version "))
        .and_then(|s| s.strip_suffix('\n'))
    else {
        return false;
    };
    if text.contains(['\r', '\n']) {
        return false;
    }
    let mut parts = text.splitn(3, '.');
    let (Some(major), Some(minor), Some(tail)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|b| b.is_ascii_digit())
        || !minor.bytes().all(|b| b.is_ascii_digit())
        || tail.is_empty()
    {
        return false;
    }
    matches!((major.parse::<u64>(),minor.parse::<u64>()),(Ok(a),Ok(b)) if (a,b)>=(2,48))
}
fn forbidden_config(key: &[u8]) -> bool {
    key.starts_with(b"include.")
        || key.starts_with(b"includeif.")
        || key.starts_with(b"filter.")
        || key == b"extensions.partialclone"
        || key == b"extensions.worktreeconfig"
        || (key.starts_with(b"extensions.") && key != b"extensions.objectformat")
        || (key.starts_with(b"remote.")
            && (key.ends_with(b".promisor") || key.ends_with(b".partialclonefilter")))
        || key == b"core.alternaterefscommand"
}
fn object_hash(kind: &str, raw: &[u8], format: &str) -> String {
    let header = format!("{kind} {}\0", raw.len());
    if format == "sha1" {
        let mut digest = ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY);
        digest.update(header.as_bytes());
        digest.update(raw);
        digest
            .finish()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    } else {
        let mut digest = Sha256::new();
        digest.update(header.as_bytes());
        digest.update(raw);
        format!("{:x}", digest.finalize())
    }
}
fn validate_tree(data: &[u8], oid_bytes: usize) -> Result<()> {
    let mut offset = 0;
    let mut names = HashSet::new();
    let mut previous: Option<Vec<u8>> = None;
    while offset < data.len() {
        let space = data[offset..]
            .iter()
            .position(|b| *b == b' ')
            .map(|p| p + offset)
            .ok_or(Error("repository_malformed_tree"))?;
        let end = data[space + 1..]
            .iter()
            .position(|b| *b == 0)
            .map(|p| p + space + 1)
            .ok_or(Error("repository_malformed_tree"))?;
        let mode = &data[offset..space];
        let name = &data[space + 1..end];
        let oid = data
            .get(end + 1..end + 1 + oid_bytes)
            .ok_or(Error("repository_malformed_tree"))?;
        let mut order = name.to_vec();
        if mode == b"40000" {
            order.push(b'/');
        }
        require(
            matches!(
                mode,
                b"40000" | b"100644" | b"100755" | b"120000" | b"160000"
            ) && !name.is_empty()
                && !matches!(name, b"." | b"..")
                && !name.contains(&b'/')
                && names.insert(name)
                && oid.iter().any(|b| *b != 0)
                && previous.as_ref().is_none_or(|p| order > *p),
            "repository_malformed_tree",
        )?;
        previous = Some(order);
        offset = end + 1 + oid_bytes;
    }
    Ok(())
}
fn status(data: &[u8]) -> Result<Value> {
    let (mut tracked, mut untracked) = (0_u64, 0_u64);
    if !data.is_empty() {
        require(data.ends_with(b"\0"), "repository_malformed_status")?;
        let mut entries = data[..data.len() - 1].split(|b| *b == 0);
        while let Some(entry) = entries.next() {
            require(
                entry.len() >= 4 && entry[2] == b' ',
                "repository_malformed_status",
            )?;
            let xy = &entry[..2];
            if xy == b"??" {
                untracked += 1;
            } else {
                require(
                    (b"MTARC".contains(&xy[0]) && b" MTD".contains(&xy[1]))
                        || matches!(
                            xy,
                            b" M"
                                | b" T"
                                | b" A"
                                | b" D"
                                | b" R"
                                | b" C"
                                | b"D "
                                | b"DD"
                                | b"AU"
                                | b"UD"
                                | b"UA"
                                | b"DU"
                                | b"AA"
                                | b"UU"
                        ),
                    "repository_malformed_status",
                )?;
                tracked += 1;
                if xy.contains(&b'R') || xy.contains(&b'C') {
                    require(
                        entries.next().is_some_and(|s| !s.is_empty()),
                        "repository_malformed_rename",
                    )?;
                }
            }
        }
    }
    Ok(
        json!({"tracked_changes":tracked,"untracked":untracked,"excluded_from_base":true,"dirty":tracked!=0 || untracked!=0}),
    )
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
