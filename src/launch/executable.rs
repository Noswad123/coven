//! execvpe-style selection from the frozen environment and child working directory.
use super::{BTreeMap, Path, TransportResult};
use crate::runtime_command::{Output, Request, Runner, Status};
use std::time::{Duration, Instant};

pub(crate) type Candidates<'a> = Box<dyn Iterator<Item = TransportResult<String>> + 'a>;
pub(crate) type Selector<'a> =
    dyn for<'b> Fn(&'b str, &'b Path, &'b BTreeMap<String, String>, Instant) -> Candidates<'b> + 'a;

#[cfg(all(test, unix))]
#[path = "executable/tests.rs"]
mod tests;

pub(crate) fn candidates<'a>(
    name: &'a str,
    cwd: &'a Path,
    env: &'a BTreeMap<String, String>,
    deadline: Instant,
) -> Candidates<'a> {
    candidates_with_deadline(name, cwd, env, Some(deadline))
}

/// Foreground attach has no captured-command deadline. Selection policy remains
/// identical, but no synthetic far-future timeout limits an interactive session.
pub(crate) fn candidates_unbounded<'a>(
    name: &'a str,
    cwd: &'a Path,
    env: &'a BTreeMap<String, String>,
) -> Candidates<'a> {
    candidates_with_deadline(name, cwd, env, None)
}

fn candidates_with_deadline<'a>(
    name: &'a str,
    cwd: &'a Path,
    env: &'a BTreeMap<String, String>,
    deadline: Option<Instant>,
) -> Candidates<'a> {
    // A present empty PATH is one empty component (cwd), not an absent PATH.
    let path = env
        .get("PATH")
        .map(String::as_str)
        .unwrap_or("/bin:/usr/bin");
    // Keep this lazy: a long cwd repeated across a 32 KiB PATH must not expand
    // into an unbounded retained list. Do not deduplicate aliases or dot paths.
    Box::new(
        path.split(':')
            .take_while(move |_| deadline.is_none_or(|limit| Instant::now() < limit))
            .filter_map(move |directory| {
                let candidate = cwd.join(directory).join(name);
                // Follow aliases only for eligibility checks. Execute the original spelling:
                // resolving or collapsing `alias/..` in argv would change kernel traversal.
                if candidate
                    .canonicalize()
                    .ok()
                    .filter(|_| deadline.is_none_or(|limit| Instant::now() < limit))
                    .is_some_and(|resolved| crate::platform::executable_metadata(&resolved).is_ok())
                {
                    return Some(candidate.into_os_string().into_string().map_err(|_| {
                        (
                            "ValueError",
                            "selected executable is not valid UTF-8".into(),
                        )
                    }));
                }
                None
            }),
    )
}

pub(crate) fn run(
    argv: Vec<String>,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    runner: &dyn Runner,
    select: &Selector<'_>,
    timeout: Duration,
) -> TransportResult<Output> {
    let deadline = Instant::now() + timeout;
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                (
                    "TimeoutExpired",
                    "transport deadline expired before backend image execution".into(),
                )
            })
    };
    let mut candidates = select(&argv[0], cwd, env, deadline);
    loop {
        remaining()?;
        let Some(candidate) = candidates.next() else {
            break;
        };
        let candidate = candidate?;
        // Lookup and prior spawn attempts share this deadline. Blocking OS calls
        // cannot be interrupted here, but never receive a fresh command budget.
        let remaining = remaining()?;
        let mut attempted_argv = argv.clone();
        attempted_argv[0] = candidate;
        let output = runner.run(&Request {
            argv: attempted_argv,
            cwd: cwd.to_owned(),
            env: env.clone(),
            timeout: remaining,
            max_output_bytes: super::MAX_OUTPUT,
        });
        // Only the runtime's proven pre-image failure permits PATH fallback.
        // Generic setup/capture errors can follow effects and must never retry.
        if output.status != Status::ExecFailed {
            return Ok(output);
        }
    }
    remaining()?;
    // No candidate executed a backend image; exhaustion has known no effects.
    Err((
        "OSError",
        "transport failed before backend image execution; frozen PATH exhausted".into(),
    ))
}
