#!/usr/bin/env python3
"""Opt-in Linux cross-repo acceptance test; Python standard library + explicit Bun."""
import argparse
import contextlib
import ctypes
import json
import os
from pathlib import Path
import resource
import select
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time


HERE = Path(__file__).resolve().parent
PASS_COUNT = 0


def require(value, message):
    if not value:
        raise AssertionError(message)


def passed(message):
    global PASS_COUNT
    PASS_COUNT += 1
    print("PASS " + message, flush=True)


def absolute(value):
    path = Path(value)
    if not path.is_absolute() or not path.is_file():
        raise argparse.ArgumentTypeError("must be an existing absolute file path")
    return str(path.resolve())


def descendants():
    parents = {}
    for entry in Path('/proc').iterdir():
        if entry.name.isdecimal():
            try:
                text = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                parents[int(entry.name)] = int(text[1])
            except (OSError, IndexError):
                pass
    found = {os.getpid()}
    while True:
        more = {pid for pid, parent in parents.items() if parent in found} | found
        if more == found:
            return found - {os.getpid()}
        found = more


def stop_children():
    # Subreaping also covers PTYs which start their own sessions/process groups.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in descendants():
            with contextlib.suppress(ProcessLookupError):
                os.kill(pid, sig)
        end = time.monotonic() + 3
        while time.monotonic() < end:
            try:
                while os.waitpid(-1, os.WNOHANG)[0]:
                    pass
            except ChildProcessError:
                pass
            if not descendants():
                return
            time.sleep(.05)
    require(not descendants(), "cleanup failed: owned children remain")


@contextlib.contextmanager
def owned_run():
    require(sys.platform == 'linux', 'this harness currently qualifies Linux only')
    require(ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0, 'cannot enable child subreaper')
    base = Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'agent-test-runs'
    require(base.parent.is_dir(), 'cache parent must already exist')
    base.mkdir(exist_ok=True)
    base = base.resolve()
    mount = subprocess.check_output(['findmnt', '-n', '-T', str(base), '-o', 'FSTYPE'], text=True).strip()
    require(mount not in ('tmpfs', 'ramfs', 'devtmpfs'), 'artifact directory is RAM-backed')
    require(shutil.disk_usage(base).free > 12 * 1024**3, 'need at least 12 GiB free disk')
    root = Path(tempfile.mkdtemp(prefix='nw-', dir=base))
    root.chmod(0o700)
    print(f'Owned disk-backed run: {root} ({mount})', flush=True)
    old = {}
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f'signal {signum}')
    for sig in (signal.SIGTERM, signal.SIGINT):
        old[sig] = signal.signal(sig, interrupted)
    try:
        yield root
    finally:
        try:
            stop_children()
            shutil.rmtree(root)
            print('CLEANUP confirmed: all owned children stopped/reaped; run directory removed', flush=True)
        finally:
            for sig, handler in old.items():
                signal.signal(sig, handler)


def limits():
    resource.setrlimit(resource.RLIMIT_FSIZE, (32 * 1024**2, 32 * 1024**2))


def build_limits():
    # Rust rlibs are larger than logs; the build driver separately caps its log.
    resource.setrlimit(resource.RLIMIT_FSIZE, (4 * 1024**3, 4 * 1024**3))


def spawn(argv, env, root, tag, build=False, **kwargs):
    with (root / (tag + '.log')).open('wb') as log:
        return subprocess.Popen(argv, env=env, cwd=root, stdout=log, stderr=log,
                                start_new_session=True, preexec_fn=build_limits if build else limits, **kwargs)


def command(argv, env, root, payload=None):
    result = subprocess.run(argv, input=payload, text=True, capture_output=True,
                            env=env, cwd=root, timeout=10, preexec_fn=limits)
    require(result.returncode == 0, f'{argv}: {result.stdout} {result.stderr}')
    require(len(result.stdout) < 1024**2, 'unexpected command output size')
    return result.stdout


def rpc(path, method, params=None):
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(2)
        sock.connect(str(path))
        sock.sendall((json.dumps({'id': 'acceptance', 'method': method, 'params': params or {}}) + '\n').encode())
        data = bytearray()
        while b'\n' not in data:
            block = sock.recv(65536)
            require(block, 'API closed without reply')
            data.extend(block)
            require(len(data) <= 2 * 1024**2, 'API reply too large')
        result = json.loads(data.split(b'\n', 1)[0])
        require(result.get('id') == 'acceptance', f'uncorrelated {method} response: {result}')
        return result


def api(path, method, params=None):
    result = rpc(path, method, params)
    require('error' not in result, f'{method}: {result}')
    return result['result']


def wait_for(fn, description, timeout=10):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            last = fn()
            if last:
                return last
        except (OSError, KeyError) as exc:
            last = str(exc)
        time.sleep(.05)
    raise AssertionError(f'timeout: {description}; last={last}')


class Decoder:
    """Small fail-closed reader for bincode 2 standard unsigned integers."""
    def __init__(self, data):
        self.data, self.pos = data, 0

    def raw(self, size):
        result = self.data[self.pos:self.pos + size]
        require(len(result) == size, 'truncated frame')
        self.pos += size
        return result

    def uint(self):
        tag = self.raw(1)[0]
        if tag < 251:
            return tag
        require(tag in (251, 252, 253), 'unsupported bincode integer')
        return int.from_bytes(self.raw({251: 2, 252: 4, 253: 8}[tag]), 'little')

    def string(self):
        return self.raw(self.uint()).decode('utf-8')


class ViewClient:
    """Real semantic-frame app client, no local terminal or GUI."""
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(2)
        self.sock.connect(str(path))
        self.pending = bytearray()
        self.text = ''
        self.generation = 0
        # ClientMessage::Hello, protocol 18, 200x60, no graphics, semantic,
        # server keys, app launch. All fields fit bincode's single-byte encoding.
        payload = bytes([0, 18, 200, 60, 0, 0, 0, 0, 0])
        self.sock.sendall(struct.pack('<I', len(payload)) + payload)
        self.welcome = False

    def poll(self):
        if select.select([self.sock], [], [], .05)[0]:
            data = self.sock.recv(1024**2)
            require(data, 'render client disconnected')
            self.pending.extend(data)
        while len(self.pending) >= 4:
            size = struct.unpack('<I', self.pending[:4])[0]
            require(size <= 2 * 1024**2, 'oversized semantic frame')
            if len(self.pending) < size + 4:
                break
            d = Decoder(self.pending[4:size + 4])
            del self.pending[:size + 4]
            kind = d.uint()
            if kind == 0:
                require(d.uint() == 18 and d.uint() == 0 and d.uint() == 0, 'handshake rejected')
                self.welcome = True
            elif kind == 1:
                count = d.uint()
                require(count <= 200 * 60, 'unexpected frame dimensions')
                cells = []
                for _ in range(count):
                    cells.append(d.string())
                    d.uint(); d.uint(); d.uint()
                    d.raw(1)
                    if d.raw(1)[0]:
                        d.uint()
                width, height = d.uint(), d.uint()
                require(width == 200 and height == 60 and count == width * height, 'invalid frame')
                self.text = '\n'.join(''.join(cells[y * width:(y + 1) * width]) for y in range(height))
                self.generation += 1
        return self.text

    def expect(self, *words, timeout=10):
        def matches():
            text = self.poll()
            return text if all(word in text for word in words) else None
        try:
            text = wait_for(matches, f'rendered {words}', timeout)
        except AssertionError:
            print('Last actual frame:\n' + self.text, file=sys.stderr)
            raise
        require('PRIVATE-' not in text, 'native identifiers leaked to rendered generic view')
        return text

    def expect_row(self, provider, *facts, timeout=10):
        def matches():
            lines = self.poll().splitlines()
            for label, detail in zip(lines, lines[1:]):
                if f'Coven / {provider} /' in label[:160] and all(fact in detail[:160] for fact in facts):
                    require('PRIVATE-' not in label[:160] + detail[:160], 'native identifier leaked into view row')
                    return True
            return False
        try:
            wait_for(matches, f'actual {provider} row {facts}', timeout)
        except AssertionError:
            print('Last actual frame:\n' + self.text, file=sys.stderr)
            raise

    def close(self):
        self.sock.close()


def environment(root):
    env = {'PATH': '/usr/bin:/bin', 'TERM': 'xterm-256color', 'LANG': 'C.UTF-8', 'SHELL': '/bin/sh'}
    for key, name in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                      ('XDG_STATE_HOME', 'state'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('TMPDIR', 'tmp')]:
        path = root / name
        path.mkdir(mode=0o700, exist_ok=True)
        env[key] = str(path)
    return env


def pane_environment(kit, root, workspace):
    if not api(kit, 'session.snapshot')['snapshot']['workspaces']:
        api(kit, 'workspace.create', {'cwd': str(workspace), 'focus': True})
    api(kit, 'pane.split', {'direction': 'right', 'cwd': str(workspace), 'focus': True})
    pane_id = api(kit, 'session.snapshot')['snapshot']['focused_pane_id']
    probe = root / 'pane-env.json'
    probe_code = 'import os,json;open(' + repr(str(probe)) + ',"w").write(json.dumps(dict(os.environ)))'
    import shlex
    shell_command = shlex.join([sys.executable, '-c', probe_code])
    api(kit, 'pane.send_input', {'pane_id': pane_id, 'text': shell_command, 'keys': ['enter']})
    try:
        wait_for(probe.exists, 'new-pane environment probe')
    except AssertionError:
        print(api(kit, 'pane.read', {'pane_id': pane_id, 'source': 'viewport', 'format': 'text'}), file=sys.stderr)
        raise
    return pane_id, json.loads(probe.read_text())


def startup_modes(args, root):
    for mode in ('default', 'sources-only', 'attached-shutdown'):
        phase = root / mode
        phase.mkdir(mode=0o700)
        env = environment(phase)
        sockets = phase / 's'
        sockets.mkdir(mode=0o700)
        kit, receiver = sockets / 'k.sock', sockets / 'c.sock'
        # Keep nested paths below sockaddr_un's limit even for the longer phase.
        require(len(str(receiver)) < 108, 'artifact root too long for phase sockets')
        workspace = phase / 'w'
        workspace.mkdir(mode=0o700)
        (workspace / 'coven.json').write_text('{}\n')
        config = phase / 'kit.toml'
        text = ('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
                '[session]\nresume_agents_on_restore = false\n[agent_detection]\nmanifest_check = false\n')
        if mode != 'default':
            text += '[integrations]\nsources = ["coven"]\n'
        if mode == 'attached-shutdown':
            runtime = [args.coven_bin, 'agent', 'run', str(workspace), '--socket', str(receiver),
                       '--kitsune-socket', str(kit), '--source', 'coven', '--json']
            report = [args.coven_bin, 'agent', 'report-native', '--socket', str(receiver)]
            text += ('[integrations.runtime]\nsource = "coven"\nexternal_agent_management = true\ncommand = '
                     + json.dumps(runtime) + '\nreport_command = ' + json.dumps(report) + '\n')
        config.write_text(text)
        env.update(KITSUNE_CONFIG_PATH=str(config), KITSUNE_SOCKET_PATH=str(kit), KITSUNE_SESSION=mode,
                   KITSUNE_INTEGRATION_SOURCE='poison', KITSUNE_INTEGRATION_REPORT_COMMAND='["/not-used"]',
                   KITSUNE_AGENT_MANAGEMENT='external', KITSUNE_LOG='warn')
        server = spawn([args.kitsune_bin, 'server'], env, phase, 'kit', stdin=subprocess.DEVNULL)
        wait_for(lambda: api(kit, 'session.snapshot'), mode + ' startup')
        if mode == 'attached-shutdown':
            wait_for(receiver.exists, 'attached helper startup')
            helper = wait_for(lambda: next((pid for pid in descendants()
                             if Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')[:4]
                             == [os.fsencode(args.coven_bin), b'agent', b'run', os.fsencode(workspace)]), None),
                             'attached helper identity')
        else:
            _, pane_env = pane_environment(kit, phase, workspace)
            for key in ('KITSUNE_INTEGRATION_SOURCE', 'KITSUNE_INTEGRATION_REPORT_COMMAND', 'KITSUNE_AGENT_MANAGEMENT'):
                require(key not in pane_env, mode + ' inherited route was not stripped: ' + key)
            sources = api(kit, 'session.snapshot')['snapshot']['integration_views']['sources']
            require(sources == ([] if mode == 'default' else [{'id': 'coven', 'epoch': None}]),
                    f'{mode} unexpectedly activated integration: {sources}')
            require(not receiver.exists(), 'presentation-only source launched helper')
        api(kit, 'server.stop')
        require(server.wait(timeout=10) == 0, mode + ' server stop failed')
        if mode == 'attached-shutdown':
            require(not Path(f'/proc/{helper}').exists(), 'Kit did not reap attached Coven helper')
            require(not receiver.exists(), 'Coven did not clean its receiver socket on stdin EOF')
            passed('actual Kit shutdown closes helper stdin; actual Coven exits, unlinks socket, and is reaped')
        else:
            passed(mode + ' startup creates no helper/lease and strips nested inherited reporting routes in actual pane')
        stop_children()


class Plugin:
    def __init__(self, args, env, cwd):
        self.child = subprocess.Popen([args.bun, str(HERE / 'plugin-fixture.ts'), args.djinn_plugin],
                                     env=env, cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, start_new_session=True, preexec_fn=limits)
        require(self.reply().get('ready'), 'actual plugin did not load')

    def reply(self):
        require(select.select([self.child.stdout], [], [], 5)[0], 'Bun fixture timed out')
        line = self.child.stdout.readline()
        if not line:
            self.child.wait(timeout=2)
            raise AssertionError('Bun fixture failed: ' + self.child.stderr.read(4096))
        return json.loads(line)

    def event(self, kind, **properties):
        self.child.stdin.write(json.dumps({'type': kind, 'properties': properties}) + '\n')
        self.child.stdin.flush()
        require(self.reply().get('dispatched'), 'event fixture dispatch failed')

    def close(self):
        self.child.stdin.close()
        require(self.child.wait(timeout=5) == 0, 'plugin fixture did not dispose cleanly')


def native_checks(args, root, kit, receiver, server, helper, pane_id, pane_env, view, plugin):
    event = plugin.event
    try:
        legacy = {'pane_id': pane_id, 'source': 'kitsune:djinn', 'agent': 'djinn', 'seq': 1, 'state': 'working'}
        rejected = rpc(kit, 'pane.report_agent', legacy)
        require(rejected.get('error', {}).get('code') == 'external_agent_management', f'legacy report not rejected: {rejected}')
        passed('legacy pane.report_agent rejected under external ownership')
        sid = 'PRIVATE-NATIVE-ROOT'
        event('session.created', info={'id': sid})
        event('session.status', sessionID=sid, status={'type': 'busy'})
        view.expect_row('Djinn', 'WORKING (fresh)')
        passed('actual Djinn built-in plugin imported by Bun -> actual reporter CLI stdin -> Coven -> Kit rendered WORKING')
        for kind in ('permission', 'question'):
            event(kind + '.asked', sessionID=sid, id='PRIVATE-REQUEST')
            view.expect_row('Djinn', 'BLOCKED (fresh)')
            event(kind + '.cancelled', sessionID=sid, requestID='PRIVATE-REQUEST')
            view.expect_row('Djinn', 'WORKING (fresh)')
            passed(f'native {kind} ask -> BLOCKED; actual cancellation delivery -> WORKING')
        event('session.status', sessionID=sid, status={'type': 'idle'})
        view.expect_row('Djinn', 'IDLE (fresh)')
        passed('native idle rendered as Coven / Djinn IDLE')
        event('session.created', info={'id': 'PRIVATE-CHILD', 'parentID': sid})
        event('session.status', sessionID='PRIVATE-CHILD', status={'type': 'busy'})
        event('permission.asked', sessionID='PRIVATE-CHILD', id='PRIVATE-CHILD-REQUEST')
        event('session.status', sessionID='PRIVATE-UNKNOWN', status={'type': 'busy'})
        end = time.monotonic() + 1
        while time.monotonic() < end:
            view.poll()
            section = '\n'.join(line[:160] for line in view.text.splitlines()[30:])
            require('BLOCKED' not in section and 'WORKING' not in section, 'child/unestablished-root status changed root')
        view.expect_row('Djinn', 'IDLE (fresh)')
        passed('child and unestablished-root input cannot change root idle state')
        command([args.coven_bin, 'agent', 'report', '--from-kitsune', '--provider', 'codex', '--json'],
                pane_env, root, json.dumps({'session_id': 'PRIVATE-HOOK-REFERENCE', 'hook_event_name': 'SessionStart'}))
        view.expect_row('codex', 'UNKNOWN', 'reference_only')
        passed('reference-only codex hook remains UNKNOWN in actual generic view')
        view.expect_row('Djinn', 'UNKNOWN (expired)', timeout=35)
        require(Path(f'/proc/{helper}').exists(), 'helper died instead of evidence expiry')
        passed('native evidence expires after real 30s while presenter remains alive')
        event('session.status', sessionID=sid, status={'type': 'busy'})
        view.expect_row('Djinn', 'WORKING (fresh)')
        os.kill(helper, signal.SIGKILL)
        wait_for(lambda: next(s for s in api(kit, 'session.snapshot')['snapshot']['integration_views']['sources']
                              if s['id'] == 'coven')['epoch'] is None,
                 'helper-loss five-second lease expiry', 8)
        view.expect('Unavailable')
        require('Coven / Djinn' not in view.text, 'stale generic rows survived helper loss')
        event('session.status', sessionID=sid, status={'type': 'idle'})
        time.sleep(1)
        # A separate invalid-API route checks no legacy socket attempt. The main
        # plugin above keeps the actual pane API identity for --from-kitsune.
        with socket.socket(socket.AF_UNIX) as canary:
            canary_path = root / 'legacy.sock'
            canary.bind(str(canary_path))
            canary_path.chmod(0o600)
            canary.listen(8)
            failed = Plugin(args, {**pane_env, 'KITSUNE_SOCKET_PATH': str(canary_path)}, root)
            failed.event('session.created', info={'id': 'PRIVATE-FAILURE'})
            failed.event('session.status', sessionID='PRIVATE-FAILURE', status={'type': 'busy'})
            time.sleep(1)
            require(not select.select([canary], [], [], 0)[0], 'Djinn attempted legacy socket fallback')
            failed.close()
        rejected = rpc(kit, 'pane.report_agent', {**legacy, 'seq': 2})
        require(rejected.get('error', {}).get('code') == 'external_agent_management', 'legacy authority revived')
        require(not api(kit, 'session.snapshot')['snapshot']['agents'], 'legacy agent appeared after transport loss')
        for pid in descendants():
            try:
                argv = Path(f'/proc/{pid}/cmdline').read_bytes()
                parent = Path(f'/proc/{pid}/status').read_text().split('PPid:\t')[1].splitlines()[0]
                require(not (parent == str(server.pid) and argv.startswith(
                    os.fsencode(args.coven_bin) + b'\0integration\0runtime\0')), 'helper restarted')
            except FileNotFoundError:
                pass
        passed('helper SIGKILL -> actual TTL Unavailable; plugin transport loss is nonfatal; no helper restart or legacy socket attempt')
        plugin.close()
        api(kit, 'server.stop')
        require(server.wait(timeout=10) == 0, 'Kit did not shut down cleanly')
        passed('actual Kit server.stop exits cleanly after helper loss')
    finally:
        view.close()


def check(args, root):
    sys.modules.setdefault('check', sys.modules[__name__])
    from bootstrap import check_setup
    check_setup(args, root)
    print(f'SUMMARY {PASS_COUNT} acceptance gates passed', flush=True)


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kitsune-bin', required=True, type=absolute)
    parser.add_argument('--coven-bin', required=True, type=absolute)
    parser.add_argument('--djinn-plugin', required=True, type=absolute)
    parser.add_argument('--bun', required=True, type=absolute)
    return parser.parse_args()


if __name__ == '__main__':
    try:
        args = arguments()
        with owned_run() as root:
            check(args, root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
