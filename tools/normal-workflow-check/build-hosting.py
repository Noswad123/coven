#!/usr/bin/env python3
"""Build current sources offline once, run A2 interop and focused regressions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
import check
import hosting


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('coven', 'kitsune', 'djinn'):
        parser.add_argument('--' + name + '-source', required=True, type=Path)
    for name in ('bun', 'zig'):
        parser.add_argument('--' + name, required=True, type=check.absolute)
    parser.add_argument('--zig-package-cache', required=True, type=Path)
    args = parser.parse_args()
    with check.owned_run() as root:
        env = check.environment(root)
        env.update(PATH=os.environ['PATH'], CARGO_HOME=os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')),
                   RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')),
                   CARGO_TARGET_DIR=str(root / 't'), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
                   CARGO_PROFILE_DEV_DEBUG='0', ZIG=args.zig,
                   LIBGHOSTTY_VT_ZIG_SYSTEM_DIR=str(args.zig_package_cache.resolve()),
                   ZIG_LOCAL_CACHE_DIR=str(root / 'z'), ZIG_GLOBAL_CACHE_DIR=str(Path.home() / '.cache/zig'))
        check.require(check.command([args.zig, 'version'], env, root).strip() == '0.15.2', 'need existing Zig 0.15.2')

        def bounded(argv, tag, timeout=900, cwd=None, build=True, context=env):
            print('COMMAND ' + json.dumps(argv), flush=True)
            # spawn intentionally starts in the owned root; Bun needs package resolution/config.
            child = check.spawn(argv, context, root, tag, build=build) if cwd is None else hosting.spawn_bun(argv, context, root, tag, cwd)
            log = root / (tag + '.log')
            end = time.monotonic() + timeout
            while child.poll() is None:
                check.require(time.monotonic() < end and log.stat().st_size <= 32 * 1024**2,
                              tag + ' exceeded time/log bound')
                time.sleep(.2)
            text = log.read_text(errors='replace')
            if '--message-format=json' not in argv:
                print(text[-24000:], flush=True)
            check.require(child.returncode == 0, f'{tag} failed ({child.returncode})')
            return text

        cargo = ['rustup', 'run', '1.96.1', 'cargo']
        manifests = {name: str((getattr(args, name + '_source') / 'Cargo.toml').resolve()) for name in ('coven', 'kitsune')}
        for name in manifests:
            bounded(cargo + ['build', '--locked', '--offline', '--manifest-path', manifests[name], '--bin', name, '-j', '1'], name + '-build')
            binary = root / 't/debug' / name
            with binary.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            print(f'BINARY {name} sha256={digest}', flush=True)
            setattr(args, name + '_bin', str(binary))
        failed = False
        try:
            hosting.run(args, root)
        except Exception as exc:
            print(f'INTEROP FAIL {exc}', flush=True)
            failed = True
        check.stop_children()
        for name, targets, filters in [
            ('coven', ['--lib'], ['agent_management::native::tests', 'agent_management::unix::navigation::tests',
                                'agent_management::facts::tests', 'agent_navigation::tests', 'session_recovery::']),
            ('coven', ['--test', 'native_attachment', '--test', 'native_facts', '--test', 'session_recovery_workflow'], []),
            ('kitsune', ['--bin', 'kitsune'], ['terminal_navigation', 'integration::env::tests', 'server::integration_runtime::tests']),
        ]:
            tag = name + ('-integration' if '--test' in targets else '-units')
            output = bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests[name], *targets,
                                     '--no-run', '--message-format=json', '-j', '1'], tag + '-build')
            executables = [json.loads(line)['executable'] for line in output.splitlines() if line.startswith('{')
                           and json.loads(line).get('reason') == 'compiler-artifact'
                           and json.loads(line).get('profile', {}).get('test') and json.loads(line).get('executable')]
            check.require(executables, 'no test executables')
            for index, executable in enumerate(executables):
                bounded([executable, *filters], tag + str(index), 180, build=False)
        package = args.djinn_source / 'clients/djinn-ui/packages/opencode'
        benv = {**env, 'OPENCODE_DISABLE_DEFAULT_PLUGINS': 'true', 'OPENCODE_DISABLE_MODELS_FETCH': 'true'}
        bounded([args.bun, 'test', '--timeout', '30000', './test/coven/attachment.test.ts',
                 './test/coven/attachment-sdk.test.ts', './test/coven/attachment-transport.test.ts',
                 '../tui/test/selected-session.test.ts'], 'producer-units', 180, cwd=package, build=False, context=benv)
        check.require(not failed, 'composed A2 gate failed (focused regressions still executed)')


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
