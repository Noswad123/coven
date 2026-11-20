# Development OpenSSL Selection

Web Push adds an OpenSSL build dependency. The private checkout installer
(`just install`, `scripts/install_dev.py`) prefers distro OpenSSL for an otherwise
unconfigured native GNU/Linux build, even when Homebrew precedes system tools in
`PATH`. It checks the Rust host architecture, queries `/usr/bin/pkg-config` with
only its built-in system search directories, and verifies `openssl/opensslv.h`
and ELF `libssl.so`/`libcrypto.so` in the reported directories. Each query has a
five-second timeout. Only the Cargo child's `OPENSSL_LIB_DIR` and
`OPENSSL_INCLUDE_DIR` are added; the shell environment is never changed.

Any explicit `OPENSSL_*` selection (including target-prefixed variables),
pkg-config setting (including an ambient `PKG_CONFIG_PATH`, even empty), target,
compiler/linker flags or Rust compiler override disables this preference. Cargo
configuration with build, target, environment or include settings is also left
alone conservatively. Non-Linux, non-native or ambiguous hosts, missing tools,
and incomplete system development files retain normal Cargo discovery. There
are no downloads or vendored toolchains. Install distro development packages if
the system probe cannot find headers and shared libraries.

To intentionally use another OpenSSL, set `OPENSSL_DIR` or both
`OPENSSL_LIB_DIR` and `OPENSSL_INCLUDE_DIR` and arrange its runtime loader setup
yourself. An explicitly exported `PKG_CONFIG_PATH` is respected, not guessed to
be accidental dotfile configuration; unset it for this invocation if you want
the system preference. This does not add an RPATH or change public wrappers,
Arcana configuration, payload publication/locking, hooks, or release packaging.

Focused characterization (no build or installation):
`python3 -m unittest discover -s tests -p test_install_dev_environment.py -v`.
