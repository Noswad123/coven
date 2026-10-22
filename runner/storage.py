"""Single-file POSIX storage primitives, not transactions or a replay protocol.

Path checks reject static redirection; they are not a race-proof sandbox. Callers
must hold the workspace lock for writes. Killed-process temp files are never
promoted, and fsync does not promise durability on every filesystem or device.
"""

from contextlib import contextmanager
from dataclasses import dataclass
import errno
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import tempfile
import threading
import time


class StorageError(Exception):
    """Storage could not be safely read or written."""


class LockBusy(StorageError):
    """Another lock holder did not release the workspace before the deadline."""


class RevisionConflict(StorageError):
    """A conditional task mutation observed a different authoritative snapshot."""


class CommitUncertain(StorageError):
    """Replace succeeded, but syncing its parent directory failed."""

    def __init__(self, path: Path):
        self.path = path
        super().__init__(f"{path}: snapshot replaced; durability uncertain because parent "
                         "directory fsync failed; inspect before retry")


@dataclass
class WorkspaceLock:
    workspace: Path
    lock_path: Path
    fd: int
    active: bool
    owner_pid: int
    owner_thread_id: int

    @property
    def root(self) -> Path:
        return self.workspace


@dataclass
class TransportGate:
    workspace: Path
    path: Path
    fd: int
    exclusive: bool
    active: bool
    owner_pid: int
    owner_thread_id: int


@contextmanager
def transport_gate(workspace: Path, path: Path, *, exclusive: bool = False,
                   held: TransportGate | None = None, timeout: float = 5.0):
    """Shared legacy admission versus exclusive activation, NOT workspace state.

    Acquire this gate BEFORE the workspace lock. Shared holders may do native
    I/O; activation cannot publish while one remains admitted. Never upgrade a
    shared gate or unlink its persistent inode.
    """
    selected = Path(workspace).absolute()
    root = selected.resolve()
    candidate = Path(path).absolute()
    try:
        try:
            relative = candidate.relative_to(selected)
        except ValueError:
            relative = candidate.relative_to(root)
    except ValueError as exc:
        raise StorageError("transport gate must be inside its resolved workspace") from exc
    if not relative.parts or ".." in relative.parts:
        raise StorageError("unsafe transport gate path")
    target = root / relative
    if held is not None:
        if (not isinstance(held, TransportGate) or not held.active or held.workspace != root or held.path != target
                or held.owner_pid != os.getpid() or held.owner_thread_id != threading.get_ident()
                or (exclusive and not held.exclusive)):
            raise StorageError("invalid held transport gate; shared gates cannot be upgraded")
        yield held
        return
    fd = None
    try:
        if not math.isfinite(timeout) or timeout < 0:
            raise StorageError("transport gate timeout must be finite and nonnegative")
        for parent in reversed(target.parents):
            if parent.exists() or parent.is_symlink():
                if not stat.S_ISDIR(parent.lstat().st_mode):
                    raise StorageError(f"unsafe transport gate parent: {parent}")
        target.parent.mkdir(parents=True, exist_ok=True)
        _check_path(target)
        fd = _open_regular(target, os.O_RDWR | os.O_CREAT)
        mode = fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH
        deadline = time.monotonic() + timeout
        while True:
            try:
                fcntl.flock(fd, mode | fcntl.LOCK_NB)
                break
            except OSError as exc:
                if exc.errno not in (errno.EACCES, errno.EAGAIN, errno.EINTR):
                    raise
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise LockBusy("runtime transport gate busy; quiesce legacy operations before activation") from exc
                time.sleep(min(0.05, remaining))
        token = TransportGate(root, target, fd, exclusive, True, os.getpid(), threading.get_ident())
    except OSError as exc:
        if fd is not None:
            os.close(fd)
        raise StorageError(f"cannot acquire transport gate: {exc}") from exc
    except BaseException:
        if fd is not None:
            os.close(fd)
        raise
    try:
        yield token
    finally:
        token.active = False
        try:
            fcntl.flock(fd, fcntl.LOCK_UN)
        finally:
            os.close(fd)


def _check_path(path: Path) -> None:
    """Check every existing component without resolving away symlinks."""
    if ".." in path.parts:
        raise StorageError(f"{path}: parent traversal is unsafe")
    for parent in reversed(path.parents):
        mode = parent.lstat().st_mode
        if not stat.S_ISDIR(mode):
            raise StorageError(f"{path}: unsafe parent {parent}; expected a directory, not a symlink")
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return
    if not stat.S_ISREG(mode):
        raise StorageError(f"{path}: unsafe target; expected a regular file, not a symlink")


def _open_regular(path: Path, flags: int, mode: int = 0o600) -> int:
    fd = os.open(path, flags | getattr(os, "O_NOFOLLOW", 0) | os.O_NONBLOCK, mode)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise StorageError(f"{path}: expected a regular file")
        os.set_inheritable(fd, False)
        return fd
    except BaseException:
        os.close(fd)
        raise


@contextmanager
def workspace_lock(workspace: Path, lock_path: Path, *,
                   held: WorkspaceLock | None = None, timeout: float = 5.0):
    """Hold a persistent flock inode; nested callers must pass their held token."""
    try:
        workspace_path = Path(workspace).absolute()
        root = workspace_path.resolve()
        candidate = Path(lock_path).absolute()
        # Accept the caller's workspace alias, but inspect paths below the root.
        try:
            relative = candidate.relative_to(workspace_path)
        except ValueError:
            try:
                relative = candidate.relative_to(root)
            except ValueError as exc:
                raise StorageError(f"{candidate}: lock path must be inside workspace {root}") from exc
        if not relative.parts or ".." in relative.parts:
            raise StorageError(f"{candidate}: lock path must be inside workspace {root}")
        path = root / relative
    except OSError as exc:
        raise StorageError(f"{lock_path}: cannot resolve workspace lock: {exc}") from exc
    if held is not None:
        if (not isinstance(held, WorkspaceLock) or not held.active
                or held.workspace != root or held.lock_path != path
                or held.owner_pid != os.getpid()
                or held.owner_thread_id != threading.get_ident()):
            raise StorageError(f"{path}: invalid held workspace lock (root, path, owner, or active state)")
        yield held
        return
    fd = None
    try:
        if not math.isfinite(timeout) or timeout < 0:
            raise StorageError(f"{path}: lock timeout must be finite and nonnegative")
        # Check before mkdir so an existing symlink cannot redirect creation.
        for parent in reversed(path.parents):
            try:
                mode = parent.lstat().st_mode
            except FileNotFoundError:
                continue
            if not stat.S_ISDIR(mode):
                raise StorageError(f"{path}: unsafe parent {parent}")
        path.parent.mkdir(parents=True, exist_ok=True)
        _check_path(path)
        fd = _open_regular(path, os.O_RDWR | os.O_CREAT)
        deadline = time.monotonic() + timeout
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except OSError as exc:
                if exc.errno not in (errno.EACCES, errno.EAGAIN, errno.EINTR):
                    raise
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise LockBusy(f"{path}: workspace lock busy after {timeout:g}s; "
                                   "another process may hold it; retry after it exits") from exc
                time.sleep(min(0.05, remaining))
        token = WorkspaceLock(root, path, fd, True, os.getpid(), threading.get_ident())
    except OSError as exc:
        if fd is not None:
            os.close(fd)
        raise StorageError(f"{lock_path}: cannot acquire workspace lock: {exc}") from exc
    except BaseException:
        if fd is not None:
            os.close(fd)
        raise
    try:
        yield token
    finally:
        token.active = False
        try:
            fcntl.flock(fd, fcntl.LOCK_UN)
        finally:
            os.close(fd)


def snapshot_revision(data: bytes | None) -> str:
    return "tasks-missing:v1" if data is None else "tasks-sha256:" + hashlib.sha256(data).hexdigest()


def validate_revision(token: str | None) -> None:
    if token is not None and (not isinstance(token, str) or not re.fullmatch(
            r"tasks-missing:v1|tasks-sha256:[0-9a-f]{64}", token)):
        raise StorageError("invalid tasks revision; expected tasks-missing:v1 or tasks-sha256:<64 lowercase hex digits>")


def encode_json(value) -> bytes:
    try:
        return (json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")
    except (TypeError, ValueError, UnicodeError, RecursionError) as exc:
        raise StorageError(f"cannot serialize strict JSON: {exc}") from exc


def sync_directory(path: Path) -> None:
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
                 | getattr(os, "O_NOFOLLOW", 0))
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_replace_bytes(path: Path, data: bytes) -> None:
    path = Path(path).absolute()
    temp = None
    try:
        _check_path(path)
        try:
            mode = stat.S_IMODE(path.lstat().st_mode)
        except FileNotFoundError:
            mode = 0o600
        fd, name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
        temp = Path(name)
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fchmod(stream.fileno(), mode)
            os.fsync(stream.fileno())
        _check_path(path)
        os.replace(temp, path)
        temp = None
        try:
            sync_directory(path.parent)
        except OSError as exc:
            raise CommitUncertain(path) from exc
    except CommitUncertain:
        raise
    except (OSError, TypeError, ValueError) as exc:
        raise StorageError(f"{path}: snapshot was not replaced: {exc}") from exc
    finally:
        if temp is not None:
            try:
                temp.unlink(missing_ok=True)
            except OSError:
                # An inert orphan is not authority. Never mask the original
                # pre-replace failure (or promote a temp to recover implicitly).
                pass


def _reject_constant(value: str):
    raise ValueError(f"nonfinite JSON constant {value}")


def _finite_float(value: str) -> float:
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"nonfinite JSON number {value}")
    return result


def read_jsonl(path: Path) -> list[dict]:
    path = Path(path).absolute()
    line_number = 1
    try:
        _check_path(path)
        fd = _open_regular(path, os.O_RDONLY)
        with os.fdopen(fd, "rb") as stream:
            data = stream.read()
        records = []
        lines = data.split(b"\n")
        for line_number, raw in enumerate(lines, 1):
            if line_number == len(lines):
                if raw:
                    raise ValueError("truncated final line (missing terminating newline)")
                break
            text = raw.decode("utf-8")
            if not text.strip():
                continue
            record = json.loads(text, parse_constant=_reject_constant, parse_float=_finite_float)
            if not isinstance(record, dict):
                raise ValueError("expected a JSON object")
            records.append(record)
        return records
    except FileNotFoundError:
        return []
    except (OSError, ValueError, StorageError, RecursionError) as exc:
        raise StorageError(f"{path}: line {line_number}: {exc}; inspect/repair the log explicitly before retry") from exc


def append_jsonl(path: Path, value: dict) -> None:
    """Append under the caller's workspace lock; no replay or deduplication."""
    path = Path(path).absolute()
    try:
        read_jsonl(path)
        if not isinstance(value, dict):
            raise StorageError("JSONL record must be an object")
        record = (json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
                  + "\n").encode("utf-8")
        _check_path(path)
        fd = _open_regular(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT)
        with os.fdopen(fd, "ab") as stream:
            stream.write(record)
            stream.flush()
            os.fsync(stream.fileno())
    except (OSError, TypeError, ValueError, StorageError, RecursionError) as exc:
        raise StorageError(f"{path}: append may be partial; inspect/repair the log explicitly before retry: {exc}") from exc
