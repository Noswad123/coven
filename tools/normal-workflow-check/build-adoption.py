#!/usr/bin/env python3
"""Offline two-job builds, composed adoption, and focused default-parallel tests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import check


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--coven-source', required=True, type=Path)
    parser.add_argument('--kitsune-source', required=True, type=Path)
    parser.add_argument('--zig', required=True, type=check.absolute)
    parser.add_argument('--zig-package-cache', required=True, type=Path)
    args = parser.parse_args()
    with check.owned_run() as root:
        env = check.environment(root)
        env.update(PATH=os.environ['PATH'], CARGO_HOME=os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')),
                   RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')),
                   CARGO_TARGET_DIR=str(root / 't'), CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0',
                   CARGO_PROFILE_DEV_DEBUG='0', ZIG=args.zig,
                   LIBGHOSTTY_VT_ZIG_SYSTEM_DIR=str(args.zig_package_cache.resolve()),
                   ZIG_LOCAL_CACHE_DIR=str(root / 'z'), ZIG_GLOBAL_CACHE_DIR=str(Path.home() / '.cache/zig'))
        check.require(subprocess.check_output([args.zig, 'version'], text=True).strip() == '0.15.2', 'need Zig 0.15.2')

        def bounded(argv, tag, timeout=1200, build=True):
            print('COMMAND ' + json.dumps(argv), flush=True)
            child = check.spawn(argv, env, root, tag, build=build)
            log = root / (tag + '.log')
            end = time.monotonic() + timeout
            while child.poll() is None:
                check.require(time.monotonic() < end and log.stat().st_size <= 32 * 1024**2,
                              tag + ' exceeded time/log bound')
                time.sleep(.2)
            text = log.read_text(errors='replace')
            if '--message-format=json' not in argv:
                print(text[-20000:], flush=True)
            check.require(child.returncode == 0, f'{tag} failed ({child.returncode})')
            return text

        cargo = ['rustup', 'run', '1.96.1', 'cargo']
        manifests = {}
        for name in ('coven', 'kitsune'):
            manifest = (getattr(args, name + '_source') / 'Cargo.toml').resolve()
            check.require(manifest.is_file(), 'missing manifest')
            manifests[name] = str(manifest)
            bounded(cargo + ['build', '--locked', '--offline', '--manifest-path', str(manifest),
                             '--bin', name, '-j', '2'], name + '-build')
            binary = root / 't/debug' / name
            with binary.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            print(f'BINARY {binary} sha256={digest}', flush=True)
        # The runtime process gets its own 32 MiB limit, never the Rust build limit.
        acceptance = [sys.executable, '-B', str(Path(__file__).with_name('adoption.py')),
                      '--coven-bin', str(root / 't/debug/coven'), '--kitsune-bin', str(root / 't/debug/kitsune')]
        print('COMMAND ' + json.dumps(acceptance), flush=True)
        result = subprocess.Popen(acceptance, env={**os.environ, 'TMPDIR': env['TMPDIR']})
        try:
            result.wait(timeout=120)
        except subprocess.TimeoutExpired:
            # Let the nested owned_run finalizer reap children and remove its root.
            result.terminate()
            result.wait(timeout=15)
            raise AssertionError('adoption exceeded two-minute runtime bound')
        # Still run the focused regressions when composed acceptance finds a bug.
        failed = result.returncode != 0
        for name, target, filters in [
            ('coven', ['--lib'], ['agent_management::native::tests', 'agent_management::unix::navigation::tests',
                                 'agent_navigation::tests', 'interactive_sessions::tests', 'session_recovery::',
                                 'native_extensions::']),
            ('kitsune', ['--bin', 'kitsune'], ['terminal_navigation', 'integration_views::tests',
                                             'integration::env::tests', 'server::integration_runtime::tests',
                                             'app::api::command_submission::tests']),
            ('coven', ['--bin', 'coven'], ['cli_operands::tests::navigation',
                                        'cli_operands::tests::native_extensions', 'cli_operands::tests::session']),
        ]:
            tag = name + ('-cli' if name == 'coven' and '--bin' in target else '')
            output = bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests[name],
                                      *target, '--no-run', '--message-format=json', '-j', '2'], tag + '-test-build')
            executables = [json.loads(line)['executable'] for line in output.splitlines()
                           if line.startswith('{') and json.loads(line).get('reason') == 'compiler-artifact'
                           and json.loads(line).get('profile', {}).get('test') and json.loads(line).get('executable')]
            check.require(len(executables) == 1, f'ambiguous test binary: {executables}')
            try:
                bounded([executables[0], *filters], tag + '-units', 180, build=False)
            except AssertionError:
                failed = True
        try:
            output = bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests['coven'],
                                      '--test', 'interactive_sessions_workflow', '--test', 'session_recovery_workflow',
                                      '--no-run', '--message-format=json', '-j', '2'], 'coven-workflow-build', 300)
            executables = [json.loads(line)['executable'] for line in output.splitlines()
                           if line.startswith('{') and json.loads(line).get('reason') == 'compiler-artifact'
                           and json.loads(line).get('target', {}).get('kind') == ['test']
                           and json.loads(line).get('executable')]
            check.require(len(executables) == 2, 'expected two workflow test binaries')
            for executable in executables:
                bounded([executable], Path(executable).name, 180, build=False)
        except AssertionError:
            failed = True
        check.require(not failed, 'one or more acceptance/regression gates failed')


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
