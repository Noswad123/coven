"""Immutable local checkout observations, never delivery authorization.

The Git selected from PATH is trusted, not sandboxed. Filesystem checks and the
workspace flock exclude cooperating writers only: editors and unrelated Git
processes can race observations. No checkout contents are copied or frozen.
Split indexes, including stale sharedindex.* artifacts, are unsupported because
Git can refresh their metadata even with optional locks disabled. Object checks
cover only the commit and root tree, not the transitive checkout closure.
"""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import time
import uuid

if __package__:
    from . import storage
else:
    import storage


UNREGISTERED = "repository-unregistered:v1"
MAX_RECORD_BYTES = 65536
PROBE_TIMEOUT = 3.0
OBSERVATION_TIMEOUT = 20.0
MAX_OUTPUT_BYTES = 1048576
_DOMAIN = b"coven.repository-registration:v1\0"


class RepositoryError(storage.StorageError):
    """Registration cannot be safely observed or committed."""


def _fail(message):
    raise RepositoryError(message)


def validate_inputs(path, base, delivery_target, expected_revision):
    """Return the original request, normalizing the selector without resolving it."""
    if (not isinstance(path, str) or not path or len(os.fsencode(path)) > 8192
            or "\0" in path):
        _fail("path must be a nonempty NUL-free local path")
    if ".." in Path(path).parts:
        _fail("path must not contain parent traversal components")
    if not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", base):
        _fail("base must be a full lowercase commit object ID")
    if (not isinstance(delivery_target, str) or len(delivery_target) > 1024
            or not delivery_target.startswith("refs/heads/")
            or any(ord(c) <= 32 or ord(c) == 127 for c in delivery_target)
            or any(c in delivery_target for c in "~^:?*[\\")
            or ".." in delivery_target or "@{" in delivery_target
            or any(not p or p.startswith(".") or p.endswith((".", ".lock"))
                   for p in delivery_target.split("/"))):
        _fail("delivery target must be a valid full refs/heads/... ref")
    if (not isinstance(expected_revision, str) or not re.fullmatch(
            r"repository-unregistered:v1|repository-sha256:[0-9a-f]{64}", expected_revision)):
        _fail("invalid repository expected revision")
    return {"path": os.path.abspath(path), "base": base,
            "delivery_target": delivery_target, "expected_revision": expected_revision}


def _workspace(workspace):
    try:
        root = Path(workspace).resolve(strict=True)
        if not root.is_dir():
            _fail("workspace must be an existing orchestration root")
        storage._check_path(root / "coven.json")
        if not stat.S_ISREG((root / "coven.json").lstat().st_mode):
            _fail("workspace requires a regular coven.json marker")
        if not stat.S_ISDIR((root / "state").lstat().st_mode):
            _fail("workspace requires a nonsymlink state directory")
        storage._check_path(root / "state" / "repository.json")
        return root
    except (OSError, ValueError, RuntimeError, storage.StorageError) as exc:
        raise RepositoryError("invalid repository workspace structure") from exc


def _revision(data):
    return UNREGISTERED if data is None else "repository-sha256:" + hashlib.sha256(_DOMAIN + data).hexdigest()


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            _fail("duplicate registration JSON key")
        result[key] = value
    return result


def _validate_record(record):
    fields = {"schema_version", "kind", "registration_id", "canonical_root", "git_dir",
              "common_dir", "filesystem", "object_format", "base_commit", "base_tree",
              "delivery_target", "registered_at", "original_request"}
    if (not isinstance(record, dict) or set(record) != fields
            or type(record["schema_version"]) is not int or record["schema_version"] != 1
            or record["kind"] != "repository-registration"):
        _fail("unsupported or malformed repository registration")
    try:
        if str(uuid.UUID(record["registration_id"])) != record["registration_id"]:
            raise ValueError()
        stamp = datetime.fromisoformat(record["registered_at"])
        if stamp.tzinfo is None or stamp.utcoffset().total_seconds() != 0:
            raise ValueError()
        original = record["original_request"]
        if not isinstance(original, dict) or set(original) != {"path", "base", "delivery_target", "expected_revision"}:
            raise ValueError()
        if (validate_inputs(**original) != original or original["expected_revision"] != UNREGISTERED
                or original["base"] != record["base_commit"]
                or original["delivery_target"] != record["delivery_target"]):
            raise ValueError()
        length = {"sha1": 40, "sha256": 64}[record["object_format"]]
        for key in ("base_commit", "base_tree"):
            if not isinstance(record[key], str) or not re.fullmatch("[0-9a-f]{%d}" % length, record[key]):
                raise ValueError()
        for key in ("canonical_root", "git_dir", "common_dir"):
            value = record[key]
            if not isinstance(value, str) or "\0" in value or not os.path.isabs(value) or os.path.normpath(value) != value:
                raise ValueError()
        identities = record["filesystem"]
        if not isinstance(identities, dict) or set(identities) != {"root", "git_dir", "common_dir"}:
            raise ValueError()
        for identity in identities.values():
            if (not isinstance(identity, dict) or set(identity) != {"device", "inode"}
                    or any(type(v) is not int or v < 0 for v in identity.values())):
                raise ValueError()
    except (ValueError, TypeError, KeyError, AttributeError, OverflowError) as exc:
        raise RepositoryError("malformed repository registration fields") from exc
    return record


def _read(root):
    path = root / "state" / "repository.json"
    try:
        storage._check_path(path)
        try:
            fd = storage._open_regular(path, os.O_RDONLY)
        except FileNotFoundError:
            return None, None
        with os.fdopen(fd, "rb") as stream:
            data = stream.read(MAX_RECORD_BYTES + 1)
        if len(data) > MAX_RECORD_BYTES:
            _fail("repository registration exceeds size limit")
        record = json.loads(data.decode("utf-8"), object_pairs_hook=_pairs,
                            parse_constant=lambda _: _fail("nonfinite registration JSON"))
        return _validate_record(record), data
    except (OSError, ValueError, RecursionError, storage.StorageError) as exc:
        raise RepositoryError("cannot read repository registration authority") from exc


def read_registration(workspace):
    """Read strict bounded authority, without locking, syncing, or invoking Git."""
    return _read(_workspace(workspace))[0]


# The supervisor retains its process-group identity until killed by the parent,
# including after Git exits. A parent-death pipe and alarm bound orphan lifetime.
_SUPERVISOR = r'''
import os, selectors, signal, subprocess, sys, time
control, parent = int(sys.argv[1]), int(sys.argv[2])
def stop(*unused):
    os.killpg(os.getpgrp(), signal.SIGKILL)
signal.signal(signal.SIGALRM, stop)
signal.setitimer(signal.ITIMER_REAL, float(sys.argv[3]) + 1)
os.set_inheritable(control, False)
os.set_inheritable(parent, False)
try:
    child = subprocess.Popen(sys.argv[4:], stdin=subprocess.DEVNULL, close_fds=True)
except OSError:
    os.write(control, b'error')
    stop()
os.close(1)
os.close(2)
watcher = selectors.DefaultSelector()
watcher.register(parent, selectors.EVENT_READ)
sent = False
while True:
    if watcher.select(.02) and os.read(parent, 1) == b'':
        stop()
    code = child.poll()
    if code is not None and not sent:
        os.write(control, str(code).encode('ascii'))
        os.close(control)
        sent = True
'''


def _run_git(executable, arguments, cwd, env, timeout, max_output=MAX_OUTPUT_BYTES):
    """Private bounded direct-argv runner. Never return raw diagnostics."""
    allowed = {("--version",), ("rev-parse", "--is-bare-repository"),
               ("rev-parse", "--show-object-format"),
               ("rev-parse", "--verify", "--quiet", "HEAD"), ("symbolic-ref", "--quiet", "HEAD"),
               ("config", "--local", "--no-includes", "--null", "--name-only", "--list"),
               ("config", "--local", "--no-includes", "--bool", "--get-all", "core.splitindex"),
               ("status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=all")}
    allowed.update(("rev-parse", "--path-format=absolute", flag) for flag in
                   ("--show-toplevel", "--absolute-git-dir", "--git-common-dir"))
    object_probe = (len(arguments) == 3 and arguments[0] == "cat-file"
                    and arguments[1] in {"-t", "commit", "tree"}
                    and re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", arguments[2]))
    if (arguments not in allowed and not object_probe
            or timeout <= 0 or signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL):
        _fail("Git probe not permitted")
    process = None
    descriptors = []
    selector = selectors.DefaultSelector()
    output = bytearray()
    control = bytearray()
    size = 0
    code = None
    try:
        cr, cw = os.pipe()
        descriptors.extend((cr, cw))
        pr, pw = os.pipe()
        descriptors.extend((pr, pw))
        deadline = time.monotonic() + timeout
        process = subprocess.Popen(
            [sys.executable, "-I", "-S", "-c", _SUPERVISOR, str(cw), str(pr), str(timeout),
             str(executable), "--no-pager", "--no-optional-locks",
             "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false",
             "-c", "core.hooksPath=/dev/null", "-c", "maintenance.auto=false",
             "-c", "gc.auto=0", "-c", "protocol.allow=never", *arguments],
            cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, close_fds=True, start_new_session=True, pass_fds=(cw, pr))
        for fd in (cw, pr):
            os.close(fd)
            descriptors.remove(fd)
        for pipe, name in ((process.stdout, "out"), (process.stderr, "err"), (cr, "control")):
            fd = pipe if isinstance(pipe, int) else pipe.fileno()
            os.set_blocking(fd, False)
            selector.register(fd, selectors.EVENT_READ, name)
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                _fail("Git probe timed out")
            for key, _ in selector.select(min(remaining, .05)):
                try:
                    chunk = os.read(key.fd, min(65536, max_output - size + 1))
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(key.fd)
                    if key.data == "control":
                        if not re.fullmatch(rb"-?[0-9]{1,3}", control):
                            _fail("Git supervisor failed")
                        code = int(control)
                    continue
                if key.data == "control":
                    control.extend(chunk)
                    if len(control) > 16:
                        _fail("Git supervisor failed")
                else:
                    size += len(chunk)
                    if size > max_output:
                        _fail("Git probe output limit exceeded")
                    if key.data == "out":
                        output.extend(chunk)
        if code == 1 and arguments == ("rev-parse", "--verify", "--quiet", "HEAD") and size == 0:
            return b""  # A symbolic HEAD without a resolved commit is unborn.
        if code != 0:
            _fail("Git probe failed; diagnostic output withheld")
        return bytes(output)
    except OSError as exc:
        raise RepositoryError("Git probe could not execute safely") from exc
    finally:
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            for pipe in (process.stdout, process.stderr):
                pipe.close()
            process.wait(timeout=2)
        for fd in descriptors:
            os.close(fd)
        selector.close()


def _environment():
    # No inherited GIT_*, HOME, XDG, loader, askpass, proxy, or shell variables.
    return {"PATH": "/usr/bin:/bin", "HOME": "/dev/null", "XDG_CONFIG_HOME": "/dev/null",
            "LC_ALL": "C", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": "/dev/null",
            "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_OPTIONAL_LOCKS": "0",
            "GIT_NO_REPLACE_OBJECTS": "1", "GIT_NO_LAZY_FETCH": "1",
            "GIT_TERMINAL_PROMPT": "0", "GIT_ALLOW_PROTOCOL": "", "GIT_ATTR_NOSYSTEM": "1"}


def _status(data):
    counts = {"tracked_changes": 0, "untracked": 0, "excluded_from_base": True}
    if not data:
        return counts | {"dirty": False}
    if not data.endswith(b"\0"):
        _fail("malformed Git status")
    entries = iter(data[:-1].split(b"\0"))
    for entry in entries:
        if len(entry) < 4 or entry[2:3] != b" " or not entry[3:]:
            _fail("malformed Git status")
        xy = entry[:2]
        if xy == b"??":
            counts["untracked"] += 1
        elif xy not in ({bytes((x, y)) for x in b"MTARC" for y in b" MTD"}
                        | {b" M", b" T", b" A", b" D", b" R", b" C",
                           b"D ", b"DD", b"AU", b"UD", b"UA", b"DU", b"AA", b"UU"}):
            _fail("malformed Git status")
        else:
            counts["tracked_changes"] += 1
            if b"R" in xy or b"C" in xy:
                if not next(entries, b""):
                    _fail("malformed Git rename status")
    return counts | {"dirty": bool(counts["tracked_changes"] or counts["untracked"])}


def _identity(path):
    info = path.stat()
    if not stat.S_ISDIR(info.st_mode):
        _fail("repository locator is not a directory")
    return {"device": info.st_dev, "inode": info.st_ino}


def _validate_tree(data, oid_bytes):
    """Validate bounded root-tree framing, names, modes, and canonical order only."""
    offset = 0
    names = set()
    previous = None
    while offset < len(data):
        space = data.find(b" ", offset)
        end = data.find(b"\0", space + 1) if space >= 0 else -1
        if space < 0 or end < 0 or end + 1 + oid_bytes > len(data):
            _fail("malformed base root tree")
        mode, name = data[offset:space], data[space + 1:end]
        oid = data[end + 1:end + 1 + oid_bytes]
        order = name + (b"/" if mode == b"40000" else b"")
        if (mode not in {b"40000", b"100644", b"100755", b"120000", b"160000"}
                or not name or name in {b".", b".."} or b"/" in name or name in names
                or not any(oid) or (previous is not None and order <= previous)):
            _fail("malformed base root tree")
        names.add(name)
        previous = order
        offset = end + 1 + oid_bytes


def _observe(request):
    try:
        root = Path(request["path"]).resolve(strict=True)
        root_identity = _identity(root)
        selected = shutil.which("git")
        if selected is None:
            _fail("trusted Git executable missing from PATH")
        git = Path(selected).resolve(strict=True)
        if not stat.S_ISREG(git.stat().st_mode):
            _fail("Git must resolve to a regular executable")
        deadline = time.monotonic() + OBSERVATION_TIMEOUT
        env = _environment()

        def probe(*args):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                _fail("repository observation budget exceeded")
            return _run_git(git, args, root, env, min(PROBE_TIMEOUT, remaining))

        version = probe("--version")
        match = re.fullmatch(rb"git version (\d+)\.(\d+)\.[^\r\n]+\n", version)
        # Conservative minimum: upstream v2.48.0's promisor-remote.c checks
        # GIT_NO_LAZY_FETCH before spawning fetch, even for missing objects.
        if not match or tuple(map(int, match.groups())) < (2, 48):
            _fail("Git >= 2.48 is required for no-lazy-fetch probing")
        if probe("rev-parse", "--is-bare-repository") != b"false\n":
            _fail("bare repositories are unsupported")

        def locator(flag):
            value = probe("rev-parse", "--path-format=absolute", flag)
            if not value.endswith(b"\n") or b"\0" in value:
                _fail("malformed Git locator")
            path = Path(os.fsdecode(value[:-1])).resolve(strict=True)
            return path

        top = locator("--show-toplevel")
        if top != root:
            _fail("path must select the checkout root, not a subdirectory")
        git_dir, common = locator("--absolute-git-dir"), locator("--git-common-dir")
        identities = {"root": _identity(top), "git_dir": _identity(git_dir), "common_dir": _identity(common)}
        if identities["root"] != root_identity:
            _fail("repository root changed during observation")
        # Never let an index-reading command refresh a split index. Scan both
        # locations because linked worktrees have their own index directory.
        for directory in {git_dir, common}:
            with os.scandir(directory) as entries:
                for index, entry in enumerate(entries):
                    if time.monotonic() >= deadline or index >= 10000:
                        _fail("repository directory observation limit exceeded")
                    if entry.name.casefold().startswith("sharedindex."):
                        _fail("split indexes and stale sharedindex artifacts are unsupported")
        # No includes, filters (status can run clean filters), external object
        # stores, or worktree-specific configuration with uninspected overrides.
        config = probe("config", "--local", "--no-includes", "--null", "--name-only", "--list")
        if config and not config.endswith(b"\0"):
            _fail("malformed local Git configuration")
        for key in config.split(b"\0")[:-1]:
            if not key or b"\n" in key:
                _fail("malformed local Git configuration")
            key = key.lower()
            if key == b"core.splitindex":
                values = probe("config", "--local", "--no-includes", "--bool", "--get-all", "core.splitindex")
                if not values or any(value != b"false" for value in values.splitlines()):
                    _fail("split-index configuration is unsupported")
            if (key.startswith((b"include.", b"includeif.", b"filter."))
                    or key in {b"extensions.partialclone", b"extensions.worktreeconfig"}
                    or (key.startswith(b"extensions.") and key != b"extensions.objectformat")
                    or (key.startswith(b"remote.") and key.endswith((b".promisor", b".partialclonefilter")))
                    or key == b"core.alternaterefscommand"):
                _fail("unsupported Git configuration (includes, filters, or external object storage)")
        objects = common / "objects"
        for directory in (objects, objects / "info", objects / "pack"):
            if not stat.S_ISDIR(directory.lstat().st_mode):
                _fail("unsupported Git object directory")
        for name in ("alternates", "http-alternates"):
            if os.path.lexists(objects / "info" / name):
                _fail("alternate object storage is unsupported")
        with os.scandir(objects / "pack") as entries:
            for index, entry in enumerate(entries):
                if (time.monotonic() >= deadline or index >= 10000
                        or entry.name.casefold().endswith(".promisor") or entry.is_symlink()
                        or not entry.is_file(follow_symlinks=False)):
                    _fail("unsupported or excessive Git pack storage")
        fmt = probe("rev-parse", "--show-object-format").decode("ascii").rstrip("\n")
        length = {"sha1": 40, "sha256": 64}.get(fmt)
        if length is None or len(request["base"]) != length:
            _fail("base object ID does not match repository object format")
        if probe("cat-file", "-t", request["base"]) != b"commit\n":
            _fail("base must name an actual commit object, not a tag, tree, or blob")
        commit = probe("cat-file", "commit", request["base"])
        digest = hashlib.new("sha1" if fmt == "sha1" else "sha256")
        digest.update(b"commit " + str(len(commit)).encode("ascii") + b"\0" + commit)
        if digest.hexdigest() != request["base"]:
            _fail("base commit content does not match its object ID")
        first = commit.split(b"\n", 1)[0]
        if not re.fullmatch(rb"tree [0-9a-f]{" + str(length).encode() + rb"}", first):
            _fail("malformed base commit")
        tree = first[5:].decode("ascii")
        if probe("cat-file", "-t", tree) != b"tree\n":
            _fail("base tree is unavailable")
        raw_tree = probe("cat-file", "tree", tree)
        digest = hashlib.new("sha1" if fmt == "sha1" else "sha256")
        digest.update(b"tree " + str(len(raw_tree)).encode("ascii") + b"\0" + raw_tree)
        if digest.hexdigest() != tree:
            _fail("base root tree content does not match its object ID")
        _validate_tree(raw_tree, length // 2)
        head_oid = probe("rev-parse", "--verify", "--quiet", "HEAD")
        if head_oid:
            if not re.fullmatch(rb"[0-9a-f]{" + str(length).encode() + rb"}\n", head_oid):
                _fail("malformed current HEAD observation")
            head_commit = head_oid[:-1].decode("ascii")
            if probe("cat-file", "-t", head_commit) != b"commit\n":
                _fail("current HEAD does not name a commit")
            head = {"status": "commit", "commit": head_commit}
        else:
            head_ref = probe("symbolic-ref", "--quiet", "HEAD").decode("utf-8")
            if not head_ref.endswith("\n"):
                _fail("malformed unborn HEAD observation")
            validate_inputs(request["path"], request["base"], head_ref[:-1], UNREGISTERED)
            head = {"status": "unborn", "ref": head_ref[:-1]}
        status = _status(probe("status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=all"))
        if (locator("--show-toplevel") != top or locator("--absolute-git-dir") != git_dir
                or locator("--git-common-dir") != common
                or Path(request["path"]).resolve(strict=True) != root
                or {"root": _identity(top), "git_dir": _identity(git_dir),
                    "common_dir": _identity(common)} != identities):
            _fail("repository locators changed during observation")
        frozen = {"canonical_root": str(top), "git_dir": str(git_dir), "common_dir": str(common),
                  "filesystem": identities,
                  "object_format": fmt, "base_commit": request["base"], "base_tree": tree,
                  "delivery_target": request["delivery_target"]}
        return frozen, {"available": True, **status, "head": head,
                        "base_object_validation": "commit_and_root_tree_only",
                        "complete_checkout_availability": "unestablished"}
    except (OSError, ValueError, RuntimeError, UnicodeError) as exc:
        raise RepositoryError("repository observation failed; native details withheld") from exc


def _envelope(record, data, status=None):
    result = {"schema_version": 1, "status": status or ("registered" if record else "unregistered"),
              "revision": _revision(data), "authorization": "none", "dispatchable": False}
    if record is not None:
        result["registration"] = record
    return result


def inspect_repository(workspace, verify=False):
    try:
        root = _workspace(workspace)
        record, data = _read(root)
    except RepositoryError:
        result = _envelope(None, None, "unavailable")
        result["revision"] = None
        result["diagnostics"] = [{"code": "repository_authority_unavailable"}]
        if verify:
            result["observations"] = {"available": False, "reason": "authority_unavailable"}
        return result
    result = _envelope(record, data)
    if verify and record is None:
        result["observations"] = {"available": False, "reason": "unregistered"}
    if verify and record is not None:
        try:
            frozen, status = _observe(record["original_request"])
            if any(record[key] != value for key, value in frozen.items()):
                _fail("registered repository locators or base changed")
            result["observations"] = {"available": True, **status}
        except RepositoryError:
            result["status"] = "unavailable"
            result["observations"] = {"available": False, "reason": "repository_unavailable_or_changed"}
    return result


def _confirm(root, record, data, replayed):
    result = _envelope(record, data) | {"replayed": replayed}
    outcome = "durably_committed"
    try:
        current, current_data = _read(root)
        if current_data != data or current != record:
            _fail("repository authority changed after publication")
        fd = storage._open_regular(root / "state" / "repository.json", os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
        storage.sync_directory(root / "state")
    except Exception:
        # Publication is already known. A failed verification is not a rollback
        # and must never invite callers to submit a different registration.
        outcome = "durability_uncertain"
    result["delivery"] = {"outcome": outcome}
    return result


def register_repository(workspace, path: str, base: str, delivery_target: str,
                        expected_revision: str):
    """Publish once; exact original retries confirm durability without opening Git."""
    request = validate_inputs(path, base, delivery_target, expected_revision)
    root = _workspace(workspace)
    record, data = _read(root)
    if record is None:
        if expected_revision != UNREGISTERED:
            _fail("repository revision conflict")
        frozen, _ = _observe(request)  # Expensive validation precedes lock acquisition.
    else:
        if record["original_request"] != request:
            _fail("repository registration is immutable; original request conflict")
        # This valid initial read is already a known published result. Failures
        # acquiring the lock or rechecking authority cannot erase that fact.
        try:
            with storage.workspace_lock(root, root / "state" / ".workspace.lock"):
                _workspace(root)
                return _confirm(root, record, data, True) | {"observations": {"status": "not_assessed"}}
        except Exception:
            return _envelope(record, data) | {"replayed": True,
                "observations": {"status": "not_assessed"},
                "delivery": {"outcome": "durability_uncertain"}}
    with storage.workspace_lock(root, root / "state" / ".workspace.lock"):
        _workspace(root)
        current, current_data = _read(root)
        if current is not None:
            if current["original_request"] != request:
                _fail("repository registration is immutable; original request conflict")
            return _confirm(root, current, current_data, True) | {"observations": {"status": "not_assessed"}}
        if record is not None or expected_revision != UNREGISTERED:
            _fail("repository revision conflict")
        checked, observations = _observe(request)
        if checked != frozen:
            _fail("repository changed before registration publication")
        record = {"schema_version": 1, "kind": "repository-registration",
                  "registration_id": str(uuid.uuid4()), **frozen,
                  "registered_at": datetime.now(timezone.utc).isoformat(),
                  "original_request": request}
        _validate_record(record)
        data = storage.encode_json(record)
        if len(data) > MAX_RECORD_BYTES:
            _fail("repository registration exceeds size limit")
        try:
            storage.atomic_replace_bytes(root / "state" / "repository.json", data)
        except storage.CommitUncertain:
            return _envelope(record, data) | {"replayed": False,
                                              "observations": observations,
                                              "delivery": {"outcome": "durability_uncertain"}}
        return _confirm(root, record, data, False) | {"observations": observations}
