#!/usr/bin/env python3
"""Opt-in Linux cross-repo acceptance test; Python standard library + explicit Bun."""
import argparse
import contextlib
import ctypes
import json
import os
from pathlib import Path
import resource
import re
import secrets
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
def owned_run(short=False):
    require(sys.platform == 'linux', 'this harness currently qualifies Linux only')
    require(ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0, 'cannot enable child subreaper')
    base = Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'agent-test-runs'
    require(base.parent.is_dir(), 'cache parent must already exist')
    base.mkdir(exist_ok=True)
    base = base.resolve()
    mount = subprocess.check_output(['findmnt', '-n', '-T', str(base), '-o', 'FSTYPE'], text=True).strip()
    require(mount not in ('tmpfs', 'ramfs', 'devtmpfs'), 'artifact directory is RAM-backed')
    require(shutil.disk_usage(base).free > 12 * 1024**3, 'need at least 12 GiB free disk')
    if short:
        for _ in range(100):
            root = base / secrets.token_hex(2)[:3]
            try:
                root.mkdir(mode=0o700)
                break
            except FileExistsError:
                continue
        else:
            raise AssertionError('cannot allocate short owned run')
    else:
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


def uint(value):
    require(0 <= value < 2**64, 'invalid unsigned bincode integer')
    if value < 251:
        return bytes([value])
    size, tag = (2, 251) if value < 2**16 else (4, 252) if value < 2**32 else (8, 253)
    return bytes([tag]) + value.to_bytes(size, 'little')


class ViewClient:
    """Real semantic-frame app client, no local terminal or GUI."""
    def __init__(self, path, width=200, height=60):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(2)
        self.sock.connect(str(path))
        self.pending = bytearray()
        self.text = ''
        self.generation = 0
        self.width, self.height = width, height
        # ClientMessage::Hello, protocol 18, 200x60, no graphics, semantic,
        # server keys, app launch. All fields fit bincode's single-byte encoding.
        self.send(b''.join(uint(n) for n in (0, 18, width, height, 0, 0, 0, 0, 0)))
        self.welcome = False

    def send(self, payload):
        self.sock.sendall(struct.pack('<I', len(payload)) + payload)

    def resize(self, width, height):
        require(1 <= width <= 500 and 1 <= height <= 200, 'invalid requested size')
        self.width, self.height = width, height
        self.send(b''.join(uint(n) for n in (3, width, height, 0, 0)))

    def mouse(self, column, row, kind='click'):
        require(0 <= column < self.width and 0 <= row < self.height, 'mouse outside client')
        kinds = {'click': [b'\x00\x00', b'\x01\x00'], 'up': [b'\x04'], 'down': [b'\x05']}
        events = [b'\x02' + k + uint(column) + uint(row) + b'\x00' for k in kinds[kind]]
        self.send(b'\x07' + uint(len(events)) + b''.join(events))

    def key(self, text):
        data = text.encode('utf-8')
        self.send(b'\x01' + uint(len(data)) + data)

    def press(self, key, modifiers=0):
        """Protocol-18 InputEvents key press/release, not escape decoding."""
        codes = {'enter': 1, 'up': 4, 'down': 5, 'pageup': 8, 'pagedown': 9,
                 'tab': 10, 'backtab': 11, 'esc': 14}
        code = uint(codes[key]) if key in codes else uint(15) + key.encode('utf-8')
        require(key in codes or len(key) == 1, 'expected named key or one character')
        event = b'\x00' + code + uint(modifiers)
        self.send(b'\x07\x02' + event + b'\x00' + event + b'\x02')

    def poll(self):
        deadline = time.monotonic() + .15
        received = frames = 0
        while frames < 32 and time.monotonic() < deadline:
            if len(self.pending) >= 4:
                size = struct.unpack('<I', self.pending[:4])[0]
                require(size <= 2 * 1024**2, 'oversized semantic frame')
            else:
                size = None
            if size is None or len(self.pending) < size + 4:
                if received >= 8 * 1024**2 or not select.select([self.sock], [], [], .05 if not received and not frames else 0)[0]:
                    break
                data = self.sock.recv(1024**2)
                require(data, 'render client disconnected')
                self.pending.extend(data)
                received += len(data)
                continue
            frames += 1
            d = Decoder(self.pending[4:size + 4])
            del self.pending[:size + 4]
            kind = d.uint()
            if kind == 0:
                require(d.uint() == 18 and d.uint() == 0 and d.uint() == 0, 'handshake rejected')
                self.welcome = True
            elif kind == 1:
                count = d.uint()
                require(count <= 500 * 200, 'unexpected frame dimensions')
                cells = []
                for _ in range(count):
                    cells.append(d.string())
                    d.uint(); d.uint(); d.uint()
                    d.raw(1)
                    if d.raw(1)[0]:
                        d.uint()
                width, height = d.uint(), d.uint()
                require(1 <= width <= 500 and 1 <= height <= 200 and count == width * height, 'invalid frame')
                # Queued frames at the preceding size may arrive after Resize.
                if (width, height) != (self.width, self.height):
                    continue
                self.text = '\n'.join(''.join(cells[y * width:(y + 1) * width]) for y in range(height))
                self.generation += 1
                guard = getattr(self, 'selection_guard', None)
                if guard is not None:
                    require(self.inspector_identity() == guard,
                            f'inspector selection continuity lost at frame {self.generation}: '
                            f'expected={guard!r} actual={self.inspector_identity()!r}\n{self.text}')
        return self.text

    def inspector_identity(self):
        """Rendered source/query/selected item only; protocol 18 exposes no item IDs."""
        width, height = min(self.width, 100), min(self.height, 32)
        left, top = (self.width - width) // 2, (self.height - height) // 2
        rows = [line[left + 1:left + width - 1].strip()
                for line in self.text.splitlines()[top + 1:top + height - 1]]
        available = height - 4
        start = 2 + min(available // 4, 8)
        selected = [line[2:].split(' ', 1)[-1] for line in rows[start:start + available // 3]
                    if line.startswith('> ')]
        source = [line for line in rows[2:start] if line.startswith('> ')]
        return (tuple(source), rows[1], selected[0]) if len(selected) == 1 and len(rows) > 1 else None

    @contextlib.contextmanager
    def selected_frames(self):
        previous = getattr(self, 'selection_guard', None)
        identity = self.inspector_identity()
        require(identity is not None, 'inspector has no selected item:\n' + self.text)
        require(previous is None or identity == previous, 'inspector identity changed before interaction')
        self.selection_guard = identity
        try:
            yield identity
        finally:
            self.selection_guard = previous

    def inspector_click(self, locate, expected, description, timeout=3):
        """Retry only local inspection clicks, with fresh hits and rendered postconditions."""
        end = time.monotonic() + timeout
        attempts = 0
        while time.monotonic() < end:
            self.poll()
            if expected():
                if attempts > 1:
                    print(f'INPUT TRACE {description}: {attempts} read-only attempts; frame={self.generation}', flush=True)
                return
            point = locate()
            if point is None:
                time.sleep(.02)
                continue
            attempts += 1
            self.mouse(*point)
            reply_end = min(end, time.monotonic() + .35)
            while time.monotonic() < reply_end:
                self.poll()
                if expected():
                    if attempts > 1:
                        print(f'INPUT TRACE {description}: {attempts} read-only attempts; frame={self.generation}', flush=True)
                    return
                time.sleep(.01)
        raise AssertionError(f'timeout: {description}; attempts={attempts}; frame={self.generation}\n{self.text}')

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

    def compact_rows(self, provider, state=None, name=None):
        provider = {'opencode': 'OpenCode', 'copilot': 'Copilot', 'codex': 'Codex',
                    'claude': 'Claude', 'djinn': 'Djinn', 'pi': 'Pi'}[provider.lower()]
        glyphs = {'Working': '[\u2800-\u28ff◌]', 'Blocked': '!', 'Idle': '·', 'Ended': '·', 'Unknown': r'\?'}
        icon = '|'.join(glyphs[s] for s in (state or '|'.join(glyphs)).split('|'))
        pattern = rf'^({icon}) ({re.escape(name or provider)})\s*$'
        return [match.groups() for line in self.poll().splitlines()
                if (match := re.fullmatch(pattern, line.split('│', 1)[0].strip()))]

    def expect_row(self, provider, *facts, timeout=10, name=None, main_facts=()):
        """Check the actual summary and its read-only inspector, never RPC models."""
        state = next((m.group().title() for fact in facts
                      if (m := re.match(r'WORKING|BLOCKED|IDLE|ENDED|UNKNOWN', fact))), None)
        opened = searching = False
        try:
            rows = wait_for(lambda: self.compact_rows(provider, state, name), f'actual {provider} icon/name summary {state}', timeout)
            name = rows[0][1]
            self.press('b', 2)
            self.press('a')
            opened = True
            self.expect('Integration details', timeout=timeout)
            width, height = min(self.width, 100), min(self.height, 32)
            left, top = (self.width - width) // 2, (self.height - height) // 2

            def panel():
                return [line[left + 1:left + width - 1] for line in self.text.splitlines()[top + 1:top + height - 1]]

            self.inspector_click(
                lambda: next(((left + 3, top + 1 + n) for n, line in enumerate(panel())
                              if line.strip() in ('coven', '> coven')), None),
                lambda: 'Tab: Sources' in self.text and any(line.strip() == '> coven' for line in panel()),
                'inspector Coven source')
            self.press('/')
            searching = True
            for char in name:
                self.press(char)
            self.expect('/ ' + name)
            self.press('enter')  # Ends search only; never activate a row.
            searching = False
            self.expect('Filter: ' + name)
            available = height - 4
            detail_start = 2 + min(available // 4, 8) + available // 3

            def matches():
                # Duplicate names are intentional. Inspect each rendered hit;
                # identities stay in the owner protocol, never in the sidebar.
                def hits():
                    return [top + 1 + n for n, line in enumerate(panel())
                            if 2 + min(available // 4, 8) <= n < detail_start and
                            re.fullmatch(rf'[ >]*\S {re.escape(name)}\s*', line)]

                for ordinal in range(len(hits())):
                    def locate():
                        current = hits()
                        return (left + 3, current[ordinal]) if ordinal < len(current) else None

                    def selected():
                        point = locate()
                        return point is not None and panel()[point[1] - top - 1].strip().startswith('> ') and all(
                            heading in '\n'.join(panel()[detail_start:])
                            for heading in ('Attention', 'Current activity', 'Working context'))

                    self.inspector_click(locate, selected, f'initial {name} selection {ordinal}', timeout)
                    with self.selected_frames():
                        detail = '\n'.join(panel()[detail_start:]).rstrip()
                        if '▾ Diagnostics [d]' in detail:
                            self.diagnostics(False)
                            detail = '\n'.join(panel()[detail_start:]).rstrip()
                        require(all(heading in detail for heading in ('Attention', 'Current activity', 'Working context')),
                                'structured main information missing: ' + detail)
                        require('▸ Diagnostics [d]' in detail and 'Coven:' not in detail and 'Status:' not in detail,
                                'diagnostics leaked into default main information: ' + detail)
                        require(name not in detail, 'item name repeated in Details: ' + detail)
                        require(all(fact in detail for fact in main_facts), 'missing main information: ' + detail)
                        self.diagnostics(True)
                        detail = self.inspector_details()
                        if 'Coven:' in detail and all(fact in detail for fact in facts):
                            return detail
                        self.diagnostics(False)
                return False

            detail = matches()
            require(detail, f'actual {provider} full inspector detail missing: {facts}')
            require('PRIVATE-' not in self.text, 'native identifier leaked into generic inspector')
            return detail
        except AssertionError:
            print('Last actual frame:\n' + self.text, file=sys.stderr)
            raise
        finally:
            try:
                if searching:
                    self.press('esc')
                    self.expect('Filter:')
            finally:
                if opened:
                    self.press('esc')
                    wait_for(lambda: 'Integration details' not in self.poll(), 'inspector closed after row check')

    def diagnostics(self, expanded=True):
        """Use the visible header to focus Details, then its read-only key."""
        with self.selected_frames():
            self.expect('Diagnostics [d]')
            width, height = min(self.width, 100), min(self.height, 32)
            left, top = (self.width - width) // 2, (self.height - height) // 2

            def focus_point():
                return next(((left + 3, y - 1) for y, line in enumerate(self.text.splitlines())
                             if top + 1 < y < top + height - 1 and
                             'Diagnostics [d]' in line[left + 1:left + width - 1]), None)

            target = ('▾' if expanded else '▸') + ' Diagnostics [d]'
            if target in self.text:
                return
            self.inspector_click(focus_point, lambda: 'Tab: Details' in self.text, 'Details focus')
            for attempt in range(4):
                self.poll()
                if target in self.text:
                    return
                require(('▸' if expanded else '▾') + ' Diagnostics [d]' in self.text,
                        'diagnostic header disappeared before toggle')
                self.press('d')
                end = time.monotonic() + .5
                while time.monotonic() < end:
                    self.poll()
                    if target in self.text:
                        if attempt:
                            print(f'INPUT TRACE diagnostics: {attempt + 1} read-only attempts; frame={self.generation}', flush=True)
                        return
                    time.sleep(.01)
            raise AssertionError('diagnostic toggle did not produce expected state:\n' + self.text)

    def inspector_details(self):
        """Collect wrapped detail text through actual read-only viewport scrolling."""
        width, height = min(self.width, 100), min(self.height, 32)
        left, top = (self.width - width) // 2, (self.height - height) // 2
        available = height - 4
        start = top + 1 + 2 + min(available // 4, 8) + available // 3
        with self.selected_frames():
            chunks = []
            for _ in range(8):
                chunks.append(''.join(line[left + 1:left + width - 1].rstrip()
                                      for line in self.poll().splitlines()[start:top + height - 1]))
                generation = self.generation
                self.press('down')
                wait_for(lambda: self.poll() and self.generation > generation, 'scrolled diagnostic frame')
            for _ in range(8):
                generation = self.generation
                self.press('pageup')
                wait_for(lambda: self.poll() and self.generation > generation, 'restored detail viewport')
                if 'Attention' in self.text:
                    break
            return '\n'.join(chunks)

    def close(self):
        self.sock.close()

    def expect_missing(self, name, timeout=10):
        """Verify a removed row in the actual filtered inspector, not just a viewport."""
        self.press('b', 2)
        self.press('a')
        try:
            self.expect('Integration details', timeout=timeout)
            width, height = min(self.width, 100), min(self.height, 32)
            left, top = (self.width - width) // 2, (self.height - height) // 2
            def panel():
                return [line[left + 1:left + width - 1]
                        for line in self.poll().splitlines()[top + 1:top + height - 1]]
            source_y = wait_for(lambda: next((top + 1 + n for n, line in enumerate(panel())
                                              if line.strip() in ('coven', '> coven')), None), 'inspector Coven source')
            self.mouse(left + 3, source_y)
            self.press('/')
            for char in name:
                self.press(char)
            self.expect('/ ' + name)
            self.press('enter')  # Finish the search, never activate an item.
            # Kit also searches details: a named live row can legitimately match
            # "Djinn" through its origin. Reject the old exact row label, not all hits.
            self.expect('Filter: ' + name, timeout=timeout)
            available = height - 4
            item_start = 2 + min(available // 4, 8)
            item_rows = panel()[item_start:item_start + available // 3]
            require(not any(re.fullmatch(rf'[ >]*[?·!◌\u2800-\u28ff✓×] {re.escape(name)}\s*', line)
                            for line in item_rows),
                    'removed row remains in actual inspector: ' + name)
        finally:
            self.press('esc')
            wait_for(lambda: 'Integration details' not in self.poll(), 'inspector closed after absence check')


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
    for mode in ('default', 'presentation-only', 'sources-only', 'attached-shutdown'):
        phase = root / ('auto' if mode == 'sources-only' else mode)
        phase.mkdir(mode=0o700)
        env = environment(phase)
        if mode == 'sources-only':
            # Default-root receiver paths consume all 107 bytes at the usual cache base.
            env['XDG_STATE_HOME'] = str(phase / 'v')
            state = phase / 'v/coven/integration'
            probe = state / 'i' / ('0' * 24) / 'r.sock'
            require(len(os.fsencode(probe)) <= 107, 'artifact root too long for default receiver')
            bindir = phase / 'bin'
            bindir.mkdir(mode=0o700)
            (bindir / 'coven').symlink_to(args.coven_bin)
            env['PATH'] = str(bindir) + ':/usr/bin:/bin'
        sockets = phase / 's'
        sockets.mkdir(mode=0o700)
        kit, receiver = sockets / 'k.sock', sockets / 'c.sock'
        # Keep nested paths below sockaddr_un's limit even for the longer phase.
        require(len(os.fsencode(receiver)) <= 107, 'artifact root too long for phase sockets')
        workspace = phase / 'w'
        workspace.mkdir(mode=0o700)
        (workspace / 'coven.json').write_text('{}\n')
        config = phase / 'kit.toml'
        text = 'onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
        if mode != 'default':
            text += '[integrations]\nsources = ["coven"]\n'
        if mode == 'presentation-only':
            text += '[integrations.coven]\nauto_start = false\n'
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
        if mode == 'sources-only':
            from bootstrap import helper_for, instance
            directory, receipt = wait_for(lambda: instance(state, kit), 'source-only default active receipt')
            helper = wait_for(lambda: helper_for(args, server), 'source-only direct production helper')
            runtime = [args.coven_bin, 'integration', 'runtime', '--state-root', str(state)]
            require(Path(f'/proc/{helper}/cmdline').read_bytes().split(b'\0')[:-1] ==
                    [os.fsencode(arg) for arg in runtime], 'source-only helper argv mismatch')
            receiver = directory / 'r.sock'
            require(len(os.fsencode(receiver)) <= 107 and receiver.is_socket(),
                    'source-only default receiver missing or too long')
            _, pane_env = pane_environment(kit, phase, workspace)
            report = [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--state-root', str(state)]
            require(pane_env.get('KITSUNE_INTEGRATION_SOURCE') == 'coven' and
                    pane_env.get('KITSUNE_AGENT_MANAGEMENT') == 'external' and
                    pane_env.get('KITSUNE_SOCKET_PATH') == str(kit) and
                    json.loads(pane_env.get('KITSUNE_INTEGRATION_REPORT_COMMAND', 'null')) == report,
                    'source-only pane native/external route mismatch')
            wait_for(lambda: any(s['id'] == 'coven' and s['epoch'] is not None
                                for s in api(kit, 'session.snapshot')['snapshot']['integration_views']['sources']),
                     'source-only live integration lease')
        elif mode == 'attached-shutdown':
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
            require(not (Path(env['XDG_STATE_HOME']) / 'coven/integration').exists(),
                    mode + ' created runtime state')
            for pid in descendants():
                try:
                    parent = Path(f'/proc/{pid}/status').read_text().split('PPid:\t')[1].splitlines()[0]
                    require(parent != str(server.pid) or
                            Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')[1:3] not in
                            ([b'integration', b'runtime'], [b'agent', b'run']),
                            mode + ' launched integration helper')
                except FileNotFoundError:
                    pass
        api(kit, 'server.stop')
        require(server.wait(timeout=10) == 0, mode + ' server stop failed')
        if mode in ('sources-only', 'attached-shutdown'):
            require(not Path(f'/proc/{helper}').exists(), 'Kit did not reap attached Coven helper')
            require(not receiver.exists(), 'Coven did not clean its receiver socket on stdin EOF')
            if mode == 'sources-only':
                released = json.loads((directory / 'receipt.json').read_text())
                require(released['released'] and released['nonce'] == receipt['nonce'],
                        'source-only shutdown did not release the active generation')
                passed('source-only default auto-start discovers actual Coven on isolated PATH; production helper/native pane route/lease; EOF cleanup, reap and released receipt')
            else:
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
        passed('native idle rendered as neutral icon + Djinn only, with full inspector IDLE facts')
        event('session.created', info={'id': 'PRIVATE-CHILD', 'parentID': sid})
        event('session.status', sessionID='PRIVATE-CHILD', status={'type': 'busy'})
        event('permission.asked', sessionID='PRIVATE-CHILD', id='PRIVATE-CHILD-REQUEST')
        event('session.status', sessionID='PRIVATE-UNKNOWN', status={'type': 'busy'})
        end = time.monotonic() + 1
        while time.monotonic() < end:
            require(view.compact_rows('Djinn', 'Idle') and not view.compact_rows('Djinn', 'Blocked|Working'),
                    'child/unestablished-root status changed root')
        view.expect_row('Djinn', 'IDLE (fresh)')
        passed('child and unestablished-root input cannot change root idle state')
        command([args.coven_bin, 'agent', 'report', '--from-kitsune', '--provider', 'codex', '--json'],
                pane_env, root, json.dumps({'session_id': 'PRIVATE-HOOK-REFERENCE', 'hook_event_name': 'SessionStart'}))
        view.expect_row('codex', 'UNKNOWN', 'reference_only')
        passed('reference-only codex hook remains UNKNOWN in actual generic view')
        def observations():
            return json.loads(command([args.coven_bin, 'agent', 'observations', '--from-kitsune', '--json'],
                                      pane_env, root))['observations']
        native = next(row for row in observations() if row['provider'] == 'djinn')
        native_id = native['observation_id']
        def expired():
            row = next(row for row in observations() if row['observation_id'] == native_id)
            return row if row['reported_state'] == 'unknown' and row['freshness'] == 'expired' else None
        old = wait_for(expired, 'public native evidence expiry with history retained', 35)
        wait_for(lambda: not view.compact_rows('Djinn'), 'expired unhosted Djinn removed from sidebar')
        view.expect_missing('Djinn')
        require(old['age_ms'] >= 30000 and old['unacknowledged_pending_count'] == native['unacknowledged_pending_count'],
                'expiry erased history or acknowledged native attention')
        require(Path(f'/proc/{helper}').exists(), 'helper died instead of evidence expiry')
        passed('real 30s event-only expiry removes unhosted Djinn from sidebar/inspector; same public ID/history retained, presenter alive')
        event('session.status', sessionID=sid, status={'type': 'busy'})
        view.expect_row('Djinn', 'WORKING (fresh)')
        os.kill(helper, signal.SIGKILL)
        wait_for(lambda: next(s for s in api(kit, 'session.snapshot')['snapshot']['integration_views']['sources']
                              if s['id'] == 'coven')['epoch'] is None,
                 'helper-loss five-second lease expiry', 8)
        view.expect('Unavailable')
        require(not view.compact_rows('Djinn'), 'stale generic rows survived helper loss')
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
        rejected = rpc(kit, 'agent.list')
        require(set(rejected) == {'id', 'error'} and
                rejected['error'].get('code') == 'feature_retired',
                'legacy read facade revived after transport loss')
        require(api(kit, 'session.snapshot')['snapshot']['agents'] == [], 'legacy agent appeared after transport loss')
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
        with owned_run(short=True) as root:
            check(args, root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
