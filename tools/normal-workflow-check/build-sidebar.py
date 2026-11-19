#!/usr/bin/env python3
"""Build current Kit/Coven offline, qualify sidebar, then run all native Kit tests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
import check


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('coven', 'kitsune'):
        parser.add_argument('--' + name + '-source', required=True, type=Path)
    parser.add_argument('--zig', required=True, type=check.absolute)
    parser.add_argument('--zig-package-cache', required=True, type=Path)
    parser.add_argument('--sidebar-only', action='store_true', help='skip Kit native test builds/runs; keep composed sidebar and Coven presentation compatibility tests')
    parser.add_argument('--bun', type=check.absolute, help='also run normal-workflow and adoption checks with this cached Bun')
    parser.add_argument('--djinn-plugin', type=check.absolute, help='actual native plugin for normal-workflow checks; requires --bun')
    parser.add_argument('--djinn-source', type=Path, help='also run hosting checks; requires --bun and --djinn-plugin')
    parser.add_argument('--typecheck', action='store_true', help='also run installed Djinn tsgo without emit/incremental output; requires --djinn-source')
    args = parser.parse_args()
    os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
    check.require(bool(args.bun) == bool(args.djinn_plugin) and (not args.djinn_source or args.bun),
                  'provide --bun and --djinn-plugin together; --djinn-source also requires both')
    check.require(not args.typecheck or args.djinn_source, '--typecheck requires --djinn-source')
    with check.owned_run(short=True) as root:
        check.require(len(os.fsencode(root)) <= 40, 'need owned build TMPDIR of at most 40 bytes')
        env = check.environment(root)
        env.update(PATH=os.environ['PATH'], CARGO_HOME=os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')),
                   RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')),
                   CARGO_TARGET_DIR=str(root / 't'), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
                   CARGO_PROFILE_DEV_DEBUG='0', ZIG=args.zig,
                    LIBGHOSTTY_VT_ZIG_SYSTEM_DIR=str(args.zig_package_cache.resolve()),
                    LIBGHOSTTY_VT_OUT_DIR=str(root / 'vt'),
                   ZIG_LOCAL_CACHE_DIR=str(root / 'z'), ZIG_GLOBAL_CACHE_DIR=str(Path.home() / '.cache/zig'),
                   TMPDIR=str(root))
        check.require(check.command([args.zig, 'version'], env, root).strip() == '0.15.2', 'need cached Zig 0.15.2')

        def bounded(argv, tag, timeout=1200, context=env, build=True):
            print('COMMAND ' + json.dumps(argv), flush=True)
            child = check.spawn(argv, context, root, tag, build=build)
            log = root / (tag + '.log')
            end = time.monotonic() + timeout
            while child.poll() is None:
                # Subreaped PTY grandchildren must not look alive to kill(pid, 0)
                # assertions. Leave the directly managed Popen child to Popen.
                for pid in check.descendants() - {child.pid}:
                    try:
                        stat = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
                        if stat[0] == 'Z' and int(stat[1]) == os.getpid():
                            os.waitpid(pid, os.WNOHANG)
                    except (FileNotFoundError, ProcessLookupError, ChildProcessError):
                        pass
                check.require(time.monotonic() < end and sum(p.stat().st_size for p in root.rglob('*.log')) <= 32 * 1024**2,
                              tag + ' exceeded deadline/log bound')
                time.sleep(.2)
            text = log.read_text(errors='replace')
            if '--message-format=json' not in argv or child.returncode != 0:
                print(text if tag == 'djinn-typecheck' else text[-24000:], flush=True)
            check.require(child.returncode == 0, f'{tag} failed ({child.returncode})')
            return text

        cargo = ['rustup', 'run', '1.96.1', 'cargo']
        if args.bun:
            check.require(check.command([args.bun, '--version'], env, root).strip() == '1.4.4', 'need cached Bun 1.4.4')
        manifests = {name: str((getattr(args, name + '_source') / 'Cargo.toml').resolve())
                     for name in ('coven', 'kitsune')}
        for name, manifest in manifests.items():
            bounded(cargo + ['build', '--locked', '--offline', '--manifest-path', manifest, '--bin', name, '-j', '1'], name + '-build')
            binary = root / 't/debug' / name
            with binary.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            print(f'BINARY {name} sha256={digest}', flush=True)
        executables = []
        if not args.sidebar_only:
            output = bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests['kitsune'],
                                     '--no-run', '--message-format=json', '-j', '1'], 'kit-test-build')
            artifacts = [json.loads(line) for line in output.splitlines() if line.startswith('{')]
            executables = [(a['target']['name'], a['executable']) for a in artifacts
                           if a.get('reason') == 'compiler-artifact' and a.get('profile', {}).get('test') and a.get('executable')]
            check.require(executables, 'no native test executables')
        failed = []
        try:
            bounded([sys.executable, '-B', str(Path(__file__).with_name('sidebar.py')),
                     '--kitsune-bin', str(root / 't/debug/kitsune'), '--coven-bin', str(root / 't/debug/coven')],
                    'sidebar', 180, context={**env, 'XDG_CACHE_HOME': os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))}, build=False)
        except AssertionError as exc:
            failed.append(str(exc))
        if args.bun:
            suites = [('check.py', ['--bun', args.bun, '--djinn-plugin', args.djinn_plugin]), ('adoption.py', [])]
            if args.djinn_source:
                suites.append(('hosting.py', ['--bun', args.bun, '--djinn-source', str(args.djinn_source.resolve())]))
            for script, extra in suites:
                try:
                    bounded([sys.executable, '-B', str(Path(__file__).with_name(script)),
                             '--kitsune-bin', str(root / 't/debug/kitsune'), '--coven-bin', str(root / 't/debug/coven'), *extra],
                            script.removesuffix('.py'), 240,
                            context={**env, 'XDG_CACHE_HOME': os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))}, build=False)
                except AssertionError as exc:
                    failed.append(str(exc))
            if args.djinn_source:
                package = args.djinn_source.resolve() / 'clients/djinn-ui/packages/opencode'
                try:
                    bounded([args.bun, '--cwd', str(package), 'test', '--timeout', '30000',
                             './test/coven/native-status-sampling.test.ts', './test/coven/native-status-contract.test.ts',
                             './test/coven/native-status-publication.test.ts'],
                            'native-status-units', 180, context={**env, 'OPENCODE_DISABLE_DEFAULT_PLUGINS': 'true',
                                                               'OPENCODE_DISABLE_MODELS_FETCH': 'true'}, build=False)
                except AssertionError as exc:
                    failed.append(str(exc))
        try:
            bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests['coven'],
                             '--test', 'agent_management_presentation', '-j', '1', '--', '--test-threads=1'],
                    'coven-presentation', 180)
        except AssertionError as exc:
            failed.append(str(exc))
        for test_filter, tag in [('agent_management', 'coven-presentation-unit'),
                                 ('guarded_view', 'coven-guarded-transport')]:
            try:
                bounded(cargo + ['test', '--locked', '--offline', '--manifest-path', manifests['coven'],
                                 '--lib', test_filter, '-j', '1', '--', '--test-threads=1'], tag, 180)
            except AssertionError as exc:
                failed.append(str(exc))
        test_env = {**env, 'COVEN_TEST_BIN': str(root / 't/debug/coven')}
        for name, executable in executables:
            if name == 'kitsune':
                try:
                    bounded([executable, 'integration_views_real_coven_roundtrip', '--ignored', '--nocapture',
                              '--test-threads=1'], 'real-roundtrip', 90, test_env, False)
                except AssertionError as exc:
                    failed.append(str(exc))
            try:
                bounded([executable, '--test-threads=1'], 'full-' + name, 1200, test_env, False)
            except AssertionError as exc:
                failed.append(str(exc))
        if args.typecheck:
            package = args.djinn_source.resolve() / 'clients/djinn-ui/packages/opencode'
            check.require((package / 'node_modules/.bin/tsgo').is_file(), 'typecheck requires preinstalled tsgo; no installation attempted')
            try:
                bounded([args.bun, '--no-install', '--no-env-file', str(package / 'node_modules/.bin/tsgo'),
                         '--project', str(package / 'tsconfig.json'), '--noEmit', '--incremental', 'false', '--pretty', 'false'],
                        'djinn-typecheck', 180, context={**env, 'GOMAXPROCS': '1', 'GOMEMLIMIT': '2GiB', 'GOGC': '50'}, build=False)
            except AssertionError as exc:
                failed.append(str(exc))
        check.require(not failed, '; '.join(failed))


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
