"""Build-environment characterization only: no Cargo build or installation."""

import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("install_dev_environment", ROOT / "scripts/install_dev.py")
dev = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dev)


class BuildEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coven-build-env-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = {"HOME": str(self.root), "PATH": "/home/linuxbrew/.linuxbrew/bin:/usr/bin",
                    "UNKNOWN_SETTING": "preserved", "CARGO_TARGET_DIR": "custom target"}
        self.outputs = {
            ("rustc", "-vV"): "rustc test\nhost: x86_64-unknown-linux-gnu\n",
            ("/usr/bin/pkg-config", "--variable=pc_path", "pkg-config"):
                "/home/linuxbrew/.linuxbrew/lib/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig",
            ("/usr/bin/pkg-config", "--variable=libdir", "openssl"): "/usr/lib",
            ("/usr/bin/pkg-config", "--variable=includedir", "openssl"): "/usr/include",
        }
        self.files = {"/usr/include/openssl/opensslv.h", "/usr/lib/libssl.so",
                      "/usr/lib/libcrypto.so"}
        self.magic = b"\x7fELF"
        real_open = Path.open

        def open_file(path, *args, **kwargs):
            if str(path) in {"/usr/lib/libssl.so", "/usr/lib/libcrypto.so"}:
                return io.BytesIO(self.magic)
            return real_open(path, *args, **kwargs)

        self.open_patch = patch.object(Path, "open", open_file)
        self.open_patch.start()
        self.addCleanup(self.open_patch.stop)
        self.file_patch = patch.object(Path, "is_file", lambda path: str(path) in self.files)
        self.file_patch.start()
        self.addCleanup(self.file_patch.stop)
        # Characterization must not depend on the real distro's symlink layout.
        self.resolve_patch = patch.object(Path, "resolve", lambda path: path)
        self.resolve_patch.start()
        self.addCleanup(self.resolve_patch.stop)
        self.probe = Mock(side_effect=lambda command, **kwargs:
                          subprocess.CompletedProcess(command, 0, self.outputs[tuple(command)], ""))

    def environment(self, env=None, **kwargs):
        options = dict(system="linux", machine="x86_64", probe=self.probe, root=self.root)
        options.update(kwargs)
        return dev.build_environment(self.env if env is None else env, **options)

    def test_native_default_copy_and_clean_bounded_system_probe(self):
        before = self.env.copy()
        result = self.environment()
        self.assertEqual(result, dict(before, OPENSSL_LIB_DIR="/usr/lib",
                                      OPENSSL_INCLUDE_DIR="/usr/include"))
        self.assertEqual(self.env, before)
        self.assertIsNot(result, self.env)
        self.assertEqual(self.probe.call_count, 4)
        for call in self.probe.call_args_list:
            self.assertEqual(call.kwargs["timeout"], 5)
            self.assertTrue(call.kwargs["check"])
            self.assertTrue(call.kwargs["capture_output"])
        calls = self.probe.call_args_list
        self.assertEqual(calls[0].kwargs["env"]["RUSTUP_AUTO_INSTALL"], "0")
        self.assertEqual(calls[1].kwargs["env"], {"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
        self.assertEqual(calls[2].kwargs["env"], {"PATH": "/usr/bin:/bin", "LC_ALL": "C",
                         "PKG_CONFIG_PATH": "", "PKG_CONFIG_LIBDIR":
                         "/usr/lib/pkgconfig:/usr/share/pkgconfig", "PKG_CONFIG_SYSROOT_DIR": "/"})
        for call in calls[1:]:
            self.assertEqual(call.args[0][0], "/usr/bin/pkg-config")
            self.assertNotIn("UNKNOWN_SETTING", call.kwargs["env"])

    def test_explicit_selection_and_cross_flags_are_unchanged_even_when_empty(self):
        keys = ("OPENSSL_DIR", "OPENSSL_LIB_DIR", "OPENSSL_INCLUDE_DIR", "OPENSSL_STATIC",
                "OPENSSL_NO_VENDOR", "X86_64_UNKNOWN_LINUX_GNU_OPENSSL_DIR",
                "AARCH64_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR", "PKG_CONFIG_PATH",
                "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR", "PKG_CONFIG",
                "PKG_CONFIG_ALLOW_CROSS", "HOST_PKG_CONFIG", "TARGET_PKG_CONFIG_PATH",
                "PKG_CONFIG_PATH_x86_64_unknown_linux_gnu", "HOST", "TARGET",
                "CARGO_BUILD_TARGET", "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER",
                "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
                "RUSTC", "RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "CC", "CC_aarch64",
                "CFLAGS", "LDFLAGS")
        for key in keys:
            for value in ("", "explicit"):
                with self.subTest(key=key, value=value):
                    env = dict(self.env, **{key: value})
                    result = self.environment(env)
                    self.assertEqual(result, env)
                    self.assertIsNot(result, env)
        self.probe.assert_not_called()

    def test_non_linux_unchanged_without_probe(self):
        for system in ("darwin", "win32", "freebsd"):
            self.assertEqual(self.environment(system=system), self.env)
        self.probe.assert_not_called()

    def test_ambiguous_or_non_native_rust_host_unchanged(self):
        for host in ("aarch64-unknown-linux-gnu", "x86_64-unknown-linux-musl",
                     "x86_64-apple-darwin", "", "x86_64-unknown-linux-gnu\nhost: duplicate"):
            with self.subTest(host=host):
                self.outputs[("rustc", "-vV")] = "host: " + host
                self.probe.reset_mock()
                self.assertEqual(self.environment(), self.env)
                self.assertEqual(self.probe.call_count, 1)

    def test_cargo_config_target_and_other_build_customizations_unchanged(self):
        directory = self.root / ".cargo"
        directory.mkdir()
        for content in ('[build]\ntarget = "aarch64-unknown-linux-gnu"\n',
                        '[target.x86_64-unknown-linux-gnu]\nlinker = "custom"\n',
                        '[env]\nOPENSSL_DIR = "/custom"\n', "invalid = ["):
            with self.subTest(content=content):
                (directory / "config.toml").write_text(content)
                self.assertEqual(self.environment(), self.env)
        self.probe.assert_not_called()

    def test_missing_home_unchanged(self):
        env = {key: value for key, value in self.env.items() if key != "HOME"}
        self.assertEqual(self.environment(env), env)
        self.probe.assert_not_called()

    def test_probe_failures_at_every_stage_leave_original_selection(self):
        for index in range(4):
            for error in (FileNotFoundError(), subprocess.TimeoutExpired("probe", 5),
                          subprocess.CalledProcessError(1, "probe")):
                with self.subTest(index=index, error=type(error)):
                    def probe(command, **kwargs):
                        if list(self.outputs).index(tuple(command)) == index:
                            raise error
                        return subprocess.CompletedProcess(command, 0, self.outputs[tuple(command)], "")
                    self.assertEqual(self.environment(probe=probe), self.env)

    def test_missing_headers_or_either_shared_library_unchanged(self):
        original = self.files.copy()
        for missing in original:
            with self.subTest(missing=missing):
                self.files = original - {missing}
                self.assertEqual(self.environment(), self.env)

    def test_non_elf_library_unchanged(self):
        self.magic = b"/* linker script */"
        self.assertEqual(self.environment(), self.env)

    def test_non_system_or_invalid_probe_paths_unchanged(self):
        for command, value in (
                (("/usr/bin/pkg-config", "--variable=pc_path", "pkg-config"), "/home/brew/lib/pkgconfig"),
                (("/usr/bin/pkg-config", "--variable=libdir", "openssl"), "/home/brew/lib"),
                (("/usr/bin/pkg-config", "--variable=libdir", "openssl"), ""),
                (("/usr/bin/pkg-config", "--variable=includedir", "openssl"), "relative/include")):
            with self.subTest(value=value):
                with patch.dict(self.outputs, {command: value}):
                    self.assertEqual(self.environment(), self.env)

    def test_build_uses_helper_child_environment_without_changing_process(self):
        cargo = Mock()
        cargo.stdout = ['{"reason":"compiler-artifact","target":{"name":"coven",'
                        '"kind":["bin"]},"executable":"/not-built/coven"}\n']
        cargo.wait.return_value = 0
        with patch.object(dev, "build_environment", return_value={"MARKER": "helper"}) as helper:
            with patch.object(dev, "child") as child:
                child.return_value.__enter__.return_value = cargo
                self.assertEqual(dev.build(), Path("/not-built/coven"))
                helper.assert_called_once_with(os.environ)
                self.assertEqual(child.call_args.kwargs["env"],
                                 {"MARKER": "helper", "RUSTUP_AUTO_INSTALL": "0"})


class SystemProbeTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "linux" and Path("/usr/bin/pkg-config").is_file(),
                         "requires native Linux system pkg-config")
    def test_real_read_only_system_probe(self):
        # Deliberately exercise the default path, independent of ambient overrides.
        env = {key: os.environ[key] for key in ("HOME", "PATH", "CARGO_HOME") if key in os.environ}
        result = dev.build_environment(env)
        if "OPENSSL_LIB_DIR" not in result:
            self.skipTest("no unambiguous native system OpenSSL development installation")
        self.assertTrue((Path(result["OPENSSL_INCLUDE_DIR"]) / "openssl/opensslv.h").is_file())
        for name in ("libssl.so", "libcrypto.so"):
            with (Path(result["OPENSSL_LIB_DIR"]) / name).open("rb") as source:
                self.assertEqual(source.read(4), b"\x7fELF")
        self.assertNotIn("OPENSSL_LIB_DIR", env)


if __name__ == "__main__":
    unittest.main()
