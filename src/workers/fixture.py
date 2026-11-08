"""Controlled native help fixture for acceptance; never an installed Djinn."""
import json
import os
from pathlib import Path
import sys
import time

args = tuple(sys.argv[1:])
allowed = {('--help',), ('--version',), ('session', '--help'),
           ('session', 'run', '--help'), ('session', 'status', '--help'),
           ('session', 'watch', '--help'), ('ask', '--help'), ('agents', '--help')}
assert args in allowed
assert sys.stdin.buffer.read() == b''
assert set(os.environ) <= {'PATH', 'HOME', 'SHELL', 'LANG', 'LC_ALL', 'TERM'}
root = Path(__file__).resolve().parent
assert Path.cwd() == root / 'workspace'
with (root / 'trace').open('a') as stream:
    stream.write(json.dumps(args) + '\n')
mode = Path(__file__).stem
if mode == 'timeout':
    os.close(1)
    os.close(2)
    time.sleep(5)
if mode in {'inherited', 'closed-descendant'}:
    if os.fork() == 0:
        if mode == 'closed-descendant':
            os.close(1)
            os.close(2)
        time.sleep(0.7)
        (root / 'escaped-descendant').write_text('unexpected')
        os._exit(0)
if mode == 'oversize':
    os.write(1, b'x' * 100000)
    raise SystemExit(0)
if mode == 'nonzero':
    os.write(2, b'TOKEN=private')
    raise SystemExit(7)
if mode == 'stderr-invalid':
    os.write(2, b'\xff')
else:
    os.write(2, b'bounded warning\n')
if args == ('--version',):
    os.write(1, b'\x1b[32mdjinn 1.2.3\x1b[0m\n')
    raise SystemExit(0)
route = args[:-1]
commands = ('session', 'ask', 'agents') if not route else (
    ('run', 'status', 'watch') if route == ('session',) else ())
flags = ('--help', '--version') if not route else ('--help',)
if mode in {'missing', 'missing-failure'} and not route:
    commands = ('ask',)
if mode == 'missing-failure' and route == ('ask',):
    raise SystemExit(7)
if mode == 'no-version' and not route:
    flags = ('--help',)
lines = ['Usage: djinn ' + ' '.join((*route, '[OPTIONS]'))]
if commands:
    lines += ['', 'Commands:', *('  ' + name + '  Description' for name in commands)]
lines += ['', 'Options:', *('  ' + name + '  Description' for name in flags), '']
os.write(1, '\n'.join(lines).encode())
