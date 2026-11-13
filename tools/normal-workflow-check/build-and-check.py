#!/usr/bin/env python3
"""Offline source-snapshot build and acceptance run. Never writes into a checkout."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
from check import absolute, check, environment, owned_run, require, spawn


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kitsune-source', required=True)
    parser.add_argument('--coven-source', required=True)
    parser.add_argument('--djinn-plugin', required=True, type=absolute)
    parser.add_argument('--bun', required=True, type=absolute)
    parser.add_argument('--zig', required=True, type=absolute)
    parser.add_argument('--zig-package-cache', required=True)
    args = parser.parse_args()
    with owned_run() as root:
        env = environment(root)
        env.update(CARGO_HOME=os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')),
                   RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')),
                   PATH=os.environ['PATH'], CARGO_BUILD_JOBS='4', CARGO_PROFILE_DEV_DEBUG='0',
                   CARGO_INCREMENTAL='0', ZIG=args.zig,
                   LIBGHOSTTY_VT_ZIG_SYSTEM_DIR=str(Path(args.zig_package_cache).resolve()),
                   ZIG_GLOBAL_CACHE_DIR=str(root / 'zig-global'), ZIG_LOCAL_CACHE_DIR=str(root / 'zig-local'))
        # Copy dirty tracked and untracked source, not git's HEAD. Exclude all
        # existing build trees, caches, repository state, and local user metadata.
        for name, source in [('coven', args.coven_source), ('kitsune', args.kitsune_source)]:
            source = Path(source).resolve()
            require((source / 'Cargo.toml').is_file(), f'invalid source: {source}')
            snapshot = root / (name + '-source')
            shutil.copytree(source, snapshot, symlinks=True,
                            ignore=shutil.ignore_patterns('.git', 'target', 'zig-out', '.zig-cache',
                                                         'node_modules', '.local', '__pycache__'))
            target = root / (name + '-target')
            build_env = {**env, 'CARGO_TARGET_DIR': str(target)}
            argv = ['rustup', 'run', '1.96.1', 'cargo', 'build', '--offline', '--locked',
                    '--manifest-path', str(snapshot / 'Cargo.toml'), '--bin', name, '-j', '4']
            print(f'BUILD {name} dirty source snapshot; offline Rust 1.96.1; jobs=4; debug=0; incremental=false', flush=True)
            child = spawn(argv, build_env, root, name + '-build', build=True)
            deadline = time.monotonic() + 1200
            while child.poll() is None:
                require(time.monotonic() < deadline, 'build exceeded 20-minute bound')
                require((root / (name + '-build.log')).stat().st_size < 32 * 1024**2, 'build log exceeded 32 MiB')
                time.sleep(.2)
            code = child.returncode
            if code:
                text = (root / (name + '-build.log')).read_text(errors='replace')
                print(text[-16000:], file=sys.stderr)
            require(code == 0, f'{name} source build failed ({code})')
            setattr(args, name + '_bin', str(target / 'debug' / name))
        check(args, root)


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
