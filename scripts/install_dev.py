"""Checkout-only release build and immutable development-payload installation."""

from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import stat
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
SIGNALS = (signal.SIGINT, signal.SIGTERM)


@contextmanager
def deferred_signals():
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)
    try:
        yield
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


@contextmanager
def child(command, **kwargs):
    process = None
    pending = []
    handlers = {sig: signal.getsignal(sig) for sig in SIGNALS}
    for sig in SIGNALS:
        signal.signal(sig, lambda signum, frame: pending.append(signum))
    try:
        # Register ownership before a pending signal can start cleanup.
        try:
            process = subprocess.Popen(command, start_new_session=True, **kwargs)
        finally:
            for sig, handler in handlers.items():
                signal.signal(sig, handler)
            if pending:
                signal.raise_signal(pending[0])
        yield process
    finally:
        if process is not None:
            def stop(sig):
                try:
                    os.killpg(process.pid, sig)
                except ProcessLookupError:
                    pass

            with deferred_signals():
                try:
                    stop(signal.SIGTERM)
                    try:
                        process.wait(timeout=1)
                    except subprocess.TimeoutExpired:
                        pass
                finally:
                    try:
                        stop(signal.SIGKILL)  # Include descendants after the leader exits.
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired as error:
                        raise RuntimeError("Child cleanup failed: process did not exit after SIGKILL") from error
                    finally:
                        if process.stdout is not None:
                            process.stdout.close()


def build_environment(env, system=None, machine=None, probe=subprocess.run, root=None):
    """Prefer distro OpenSSL only for an otherwise unconfigured native Linux build."""
    result = dict(env)
    if (system if system is not None else sys.platform) != "linux":
        return result
    for key in env:
        if ("OPENSSL_" in key or "PKG_CONFIG" in key
                or key in {"HOST", "TARGET", "CARGO_BUILD_TARGET", "RUSTFLAGS",
                           "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
                           "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"}
                or key.startswith("CARGO_BUILD_RUSTC")
                or (key.startswith("CARGO_TARGET_") and key != "CARGO_TARGET_DIR")
                or any(key == name or key.startswith(name + "_") or key.endswith("_" + name)
                       for name in ("CC", "CXX", "AR", "CFLAGS", "CPPFLAGS", "LDFLAGS"))):
            return result

    root = ROOT if root is None else Path(root)
    try:
        # Cargo also accepts targets from ancestor and user configuration files.
        config_dirs = [path / ".cargo" for path in (root, *root.parents)]
        cargo_home = env.get("CARGO_HOME")
        if cargo_home:
            config_dirs.append(root / cargo_home)
        elif env.get("HOME"):
            config_dirs.append(Path(env["HOME"]) / ".cargo")
        else:
            return result  # Cannot establish Cargo's user configuration location.
        for directory in config_dirs:
            for name in ("config", "config.toml"):
                config = directory / name
                if config.exists():
                    import tomllib
                    with config.open("rb") as source:
                        settings = tomllib.load(source)
                    if (settings.get("build") or settings.get("target")
                            or settings.get("env") or settings.get("include")):
                        return result

        def query(command, query_env):
            return probe(command, env=dict(query_env), cwd=root, check=True,
                         capture_output=True, text=True, timeout=5).stdout.strip()

        rust_env = dict(env, RUSTUP_AUTO_INSTALL="0")
        version = query(["rustc", "-vV"], rust_env)
        hosts = [line[6:] for line in version.splitlines() if line.startswith("host: ")]
        arch = machine if machine is not None else platform.machine()
        arch = {"amd64": "x86_64", "arm64": "aarch64", "armv7l": "armv7"}.get(arch, arch)
        if (len(hosts) != 1 or hosts[0].split("-")[0] != arch
                or not hosts[0].endswith(("-linux-gnu", "-linux-gnueabihf"))):
            return result

        # No ambient search paths, sysroot, Homebrew executable, or user .pc files.
        clean = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}
        pc = "/usr/bin/pkg-config"
        builtin = query([pc, "--variable=pc_path", "pkg-config"], clean)
        paths = []
        for value in builtin.split(os.pathsep):
            path = Path(value)
            if (path.is_absolute() and path.name == "pkgconfig"
                    and any(path.resolve().is_relative_to(base)
                            for base in (Path("/usr/lib"), Path("/usr/lib64"),
                                         Path("/usr/share")))):
                paths.append(str(path))
        if not paths:
            return result
        clean.update(PKG_CONFIG_PATH="", PKG_CONFIG_LIBDIR=os.pathsep.join(paths),
                     PKG_CONFIG_SYSROOT_DIR="/")
        lib = Path(query([pc, "--variable=libdir", "openssl"], clean))
        include = Path(query([pc, "--variable=includedir", "openssl"], clean))
        if (not lib.is_absolute() or not include.is_absolute()
                or not any(lib.resolve().is_relative_to(base)
                           for base in (Path("/usr/lib"), Path("/usr/lib64")))
                or not include.resolve().is_relative_to(Path("/usr/include"))
                or not (include / "openssl/opensslv.h").is_file()
                or not (include / "openssl/opensslv.h").resolve().is_relative_to(include.resolve())
                or not all((lib / name).is_file() for name in ("libssl.so", "libcrypto.so"))):
            return result
        for name in ("libssl.so", "libcrypto.so"):
            library = lib / name
            if not library.resolve().is_relative_to(lib.resolve()):
                return result
            with library.open("rb") as source:
                if source.read(4) != b"\x7fELF":
                    return result
        result.update(OPENSSL_LIB_DIR=str(lib), OPENSSL_INCLUDE_DIR=str(include))
    except (OSError, ValueError, ImportError, subprocess.SubprocessError):
        pass  # Missing/ambiguous system development files retain Cargo's usual behavior.
    return result


def build():
    executable = None
    command = ["cargo", "build", "--locked", "--release", "--bin", "coven",
               "--message-format=json-render-diagnostics"]
    env = build_environment(os.environ)
    env["RUSTUP_AUTO_INSTALL"] = "0"
    with child(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True) as cargo:
        for line in cargo.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                print(line, end="", file=sys.stderr, flush=True)
                continue
            if not isinstance(message, dict):
                continue
            if message.get("reason") == "compiler-message":
                rendered = message.get("message", {}).get("rendered")
                if rendered:
                    print(rendered, end="", file=sys.stderr, flush=True)
            elif message.get("reason") == "compiler-artifact":
                target = message.get("target", {})
                if (target.get("name") == "coven" and "bin" in target.get("kind", [])
                        and message.get("executable")):
                    executable = Path(message["executable"])
        result = cargo.wait()
    if result:
        raise subprocess.CalledProcessError(result, command)
    if executable is None:
        raise RuntimeError("Cargo succeeded without reporting the bin/coven executable")
    return executable if executable.is_absolute() else ROOT / executable


def native_file(path):
    if path.is_symlink() or not path.is_file() or not os.access(path, os.X_OK):
        return False
    with path.open("rb") as source:
        magic = source.read(4)
    return magic in (b"\x7fELF", b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe",
                     b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe",
                     b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
                     b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca")


def selection(prefix):
    if not os.path.lexists(prefix):
        return None
    if prefix.is_symlink():
        target = prefix.resolve()
        if (not (target / "bin").is_symlink()
                and native_file(target / "bin/coven")
                and (target / "share/coven/examples").is_dir()
                and not any((target / part).is_symlink()
                            for part in ("share", "share/coven", "share/coven/examples"))):
            return os.readlink(prefix)
    raise RuntimeError(
        f"Refusing unmanaged or broken prefix {prefix}. Use another absolute "
        "INSTALL_PREFIX, or arrange a manual handoff of the existing entry; "
        "directories are never migrated or deleted.")


def install_prefix():
    explicit = "INSTALL_PREFIX" in os.environ
    value = os.environ.get("INSTALL_PREFIX")
    if not explicit:
        home = os.environ.get("HOME")
        if not home:
            raise RuntimeError("HOME is missing; set an absolute INSTALL_PREFIX")
        value = home + "/.local/libexec/arcana-dev/coven"
    prefix = Path(value)
    if not prefix.is_absolute() or ".." in prefix.parts or prefix == Path("/"):
        raise RuntimeError("INSTALL_PREFIX must be an absolute application prefix without ..")
    parent = prefix.parent
    # Never traverse symlink ancestors while creating or publishing a payload.
    for ancestor in reversed((parent, *parent.parents)):
        if ancestor.is_symlink():
            raise RuntimeError(f"Install parent ancestor must not be a symlink: {ancestor}")
        if not ancestor.exists():
            if explicit:
                raise RuntimeError(f"INSTALL_PREFIX parent must already exist: {parent}")
            trusted_parent(ancestor.parent)
            ancestor.mkdir()
    trusted_parent(parent)
    return prefix


def trusted_parent(parent):
    info = parent.stat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
            or info.st_mode & 0o022):
        raise RuntimeError(f"Install parent must be a trusted user-owned directory: {parent}")


@contextmanager
def publication_lock(prefix):
    # Keep this inode permanently: unlinking it could split cooperating lockers.
    fd = None
    try:
        with deferred_signals():
            fd = os.open(prefix.with_name(f".{prefix.name}.install.lock"),
                         os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                         0o600)
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_mode & 0o022 or info.st_nlink != 1):
            raise RuntimeError("Install lock must be a trusted user-owned regular file")
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        if fd is not None:
            os.close(fd)


def install(prefix, executable, previous):
    payload = link = None
    published = False
    try:
        with deferred_signals():
            payload = Path(tempfile.mkdtemp(prefix="coven-native-", dir=prefix.parent))
        (payload / "bin").mkdir()
        if not native_file(executable):
            raise RuntimeError(f"Cargo artifact is not a regular native executable: {executable}")
        shutil.copyfile(executable, payload / "bin/coven")
        (payload / "bin/coven").chmod(0o755)
        shutil.copytree(ROOT / "examples", payload / "share/coven/examples")
        # mkdtemp's private mode is useful while copying; expose only complete data.
        payload.chmod(0o755)
        with publication_lock(prefix):
            if selection(prefix) != previous:
                raise RuntimeError(f"Install selection changed concurrently: {prefix}; nothing published")
            with deferred_signals():
                if previous is None:
                    prefix.symlink_to(payload.name)  # No-clobber even for noncooperating creators.
                else:
                    candidate = payload.with_name(payload.name + ".select")
                    candidate.symlink_to(payload.name)
                    link = candidate
                    os.replace(link, prefix)
                published = True
        print(f"Installed development coven: {prefix}")
        print(f"Native payload: {payload}")
        print(f"Executable: {payload / 'bin/coven'}")
        print("Previous payloads retained for pinned consumers; public launchers unchanged.")
    finally:
        if link is not None:
            link.unlink(missing_ok=True)
        if payload is not None and not published:
            shutil.rmtree(payload)


def main():
    if len(sys.argv) != 1:
        print("Usage: just install (optional environment: INSTALL_PREFIX=/absolute/prefix)",
              file=sys.stderr)
        return 1

    def interrupted(signum, frame):
        for sig in SIGNALS:
            signal.signal(sig, signal.SIG_IGN)
        raise SystemExit(128 + signum)

    handlers = {sig: signal.getsignal(sig) for sig in SIGNALS}
    for sig in SIGNALS:
        signal.signal(sig, interrupted)
    try:
        prefix = install_prefix()
        previous = selection(prefix)
        install(prefix, build(), previous)
        return 0
    except subprocess.CalledProcessError as error:
        print(f"install-dev: {error}", file=sys.stderr)
        return error.returncode if error.returncode > 0 else 128 - error.returncode
    except (OSError, RuntimeError, ValueError) as error:
        print(f"install-dev: {error}", file=sys.stderr)
        return 1
    finally:
        for sig, handler in handlers.items():
            signal.signal(sig, handler)


if __name__ == "__main__":
    sys.exit(main())
