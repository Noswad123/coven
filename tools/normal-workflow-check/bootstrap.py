"""Acceptance through the public setup CLI and generated attached bootstrap."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time
import tomllib

from check import (Plugin, ViewClient, api, command, descendants, environment, native_checks,
                   limits, pane_environment, passed, require, spawn, startup_modes, wait_for)


def tree(root):
    result = {}
    if not root.exists():
        return result
    for path in [root, *sorted(root.rglob('*'))]:
        m = path.lstat()
        data = hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(m.st_mode) else None
        result[str(path.relative_to(root))] = (m.st_mode, m.st_ino, m.st_mtime_ns, data)
    return result


def refused(argv, env, root, error, payload=None):
    output = subprocess.run(argv, env=env, cwd=root, input=payload, text=True,
                            capture_output=True, timeout=10, preexec_fn=limits)
    require(output.returncode != 0 and error in output.stdout + output.stderr,
            f'expected {error}: {argv}: {output.returncode} {output.stdout} {output.stderr}')


def setup(args, env, root, operation, state=None, apply=False):
    argv = [args.coven_bin, 'integration', operation, '--json']
    if state is not None:
        argv += ['--state-root', str(state)]
    if apply:
        argv += ['--apply']
    return json.loads(command(argv, env, root))


def instance(state, kit):
    matches = []
    for receipt in (state / 'i').glob('*/receipt.json'):
        value = json.loads(receipt.read_text())
        if value['address'] == str(kit) and value['receiver'] and not value['released']:
            matches.append((receipt.parent, value))
    return matches[0] if len(matches) == 1 else None


def helper_for(args, server):
    found = []
    for pid in descendants():
        try:
            argv = Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0')
            status = Path(f'/proc/{pid}/status').read_text()
            if argv[:3] == [os.fsencode(args.coven_bin), b'integration', b'runtime'] and \
                    status.split('PPid:\t')[1].splitlines()[0] == str(server.pid):
                found.append(pid)
        except FileNotFoundError:
            pass
    return found[0] if len(found) == 1 else None


def launch(args, env, root, tag):
    # API/client addresses are Kit fixture inputs, never generated runtime pins.
    kit = root / (tag + '.sock')
    child_env = {**env, 'KITSUNE_SOCKET_PATH': str(kit), 'KITSUNE_SESSION': tag,
                 'KITSUNE_INTEGRATION_SOURCE': 'poison', 'KITSUNE_AGENT_MANAGEMENT': 'poison',
                 'KITSUNE_INTEGRATION_REPORT_COMMAND': '["/not-used"]', 'KITSUNE_LOG': 'warn'}
    server = spawn([args.kitsune_bin, 'server'], child_env, root, tag, stdin=subprocess.DEVNULL)
    return server, kit


def ready(args, root, state, tag, server, kit):
    wait_for(lambda: api(kit, 'session.snapshot'), tag + ' actual Kit startup')
    directory, receipt = wait_for(lambda: instance(state, kit), tag + ' bootstrap active receipt')
    helper = wait_for(lambda: helper_for(args, server), tag + ' direct bootstrap child')
    view = ViewClient(root / (tag + '-client.sock'))
    view.expect('Integrations', 'coven')
    probe_dir = root / (tag + '-probe')
    probe_dir.mkdir(mode=0o700, exist_ok=True)
    probe = probe_dir / 'pane-env.json'
    if probe.exists():
        probe.unlink()
    pane_id, pane_env = pane_environment(kit, probe_dir, root)
    for path in (state, state / 'i', directory, directory / 'w', directory / 'w/state'):
        require(stat.S_IMODE(path.stat().st_mode) == 0o700 and path.stat().st_uid == os.getuid(),
                f'bootstrap directory not private: {path}')
    receiver = directory / 'r.sock'
    require(stat.S_ISSOCK(receiver.stat().st_mode) and stat.S_IMODE(receiver.stat().st_mode) == 0o600,
            'receiver not a private socket')
    require(json.loads((directory / 'w/coven.json').read_text())['agents'] == [], 'bootstrap created agents')
    report = [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--state-root', str(state)]
    require(pane_env.get('KITSUNE_AGENT_MANAGEMENT') == 'external' and
            pane_env.get('KITSUNE_INTEGRATION_SOURCE') == 'coven' and
            json.loads(pane_env.get('KITSUNE_INTEGRATION_REPORT_COMMAND', 'null')) == report and
            pane_env.get('KITSUNE_SOCKET_PATH') == str(kit), 'actual pane bootstrap route mismatch')
    return dict(server=server, kit=kit, directory=directory, receipt=receipt, helper=helper,
                receiver=receiver, view=view, pane_id=pane_id, pane_env=pane_env)


def start(args, env, root, state, tag):
    return ready(args, root, state, tag, *launch(args, env, root, tag))


def stop(context):
    context['view'].close()
    api(context['kit'], 'server.stop')
    require(context['server'].wait(timeout=10) == 0, 'Kit shutdown failed')
    receipt = json.loads((context['directory'] / 'receipt.json').read_text())
    require(receipt['released'] and not context['receiver'].exists(), 'bootstrap state not cleanly released')
    require(not Path(f'/proc/{context["helper"]}').exists(), 'Kit did not reap bootstrap helper')
    return receipt


def provider_routes(args, root, context, other):
    for provider in ('claude', 'codex', 'copilot', 'pi', 'opencode'):
        if provider in ('pi', 'opencode'):
            event = 'session_start' if provider == 'pi' else 'session.created'
            argv = ['agent', 'report-extension', '--provider', provider, '--event', event]
            payload = {'contract': f'coven.{provider}-extension.v1', 'event': event,
                       'session_id': 'PRIVATE-' + provider}
        else:
            argv = ['agent', 'report', '--provider', provider]
            payload = {'hook_event_name': 'SessionStart', 'session_id': 'PRIVATE-' + provider}
        command([args.coven_bin, *argv, '--from-kitsune', '--json'], context['pane_env'], root, json.dumps(payload))
        context['view'].expect_row(provider, 'UNKNOWN')
        other['view'].poll()
        require(f'Coven / {provider} /' not in other['view'].text, 'provider receipt crossed Kit instance')
    passed('all five actual provider CLI reporters resolve pane-selected --from-kitsune route without state override')


def check_setup(args, root):
    # Cheap compatibility gates retain default, presentation-only, and old explicit
    # socket/workspace runtime behavior without a second 30-second expiry wait.
    startup_modes(args, root)
    phase = root / 'flow'
    phase.mkdir(mode=0o700)
    env = environment(phase)
    env['XDG_STATE_HOME'] = str(root / 'd')
    state = root / 'r'
    before = tree(phase)
    plan = setup(args, env, phase, 'plan')
    require(tree(phase) == before and not state.exists() and not (root / 'd').exists(), 'default plan created files')
    require(plan['executable'] == args.coven_bin and
            plan['kitsune_config'] == env['XDG_CONFIG_HOME'] + '/kitsune/config.toml', 'default selection mismatch')
    passed('default integration plan uses current built executable/XDG config and has zero filesystem footprint')
    home_env = {k: v for k, v in env.items() if k != 'XDG_CONFIG_HOME'}
    before = tree(phase)
    home = setup(args, home_env, phase, 'plan')
    require(home['kitsune_config'] == env['HOME'] + '/.config/kitsune/config.toml' and tree(phase) == before,
            'HOME plan selection or footprint mismatch')
    passed('HOME-only default config plan has zero filesystem footprint')
    config = Path(plan['kitsune_config'])
    config.parent.mkdir(mode=0o700)
    original = (b'# unrelated settings stay byte-exact on disable\nonboarding = false\n'
                b'[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
                b'[session]\nresume_agents_on_restore = false\n[agent_detection]\nmanifest_check = false\n'
                b'[ui]\nsidebar_width = 160\nsidebar_min_width = 160\nsidebar_max_width = 160\n'
                b'[integrations]\n# existing source comment\nsources = ["coven", "build"] # keep this comment\n')
    config.write_bytes(original)
    config.chmod(0o600)
    for operation in ('plan', 'enable', 'disable'):
        before = tree(phase)
        setup(args, env, phase, operation, state)
        require(tree(phase) == before and not state.exists(), operation + ' preview mutated filesystem')
    passed('plan and unapplied enable/disable preserve complete existing tree with no runtime state')
    enabled = setup(args, env, phase, 'enable', state, True)
    managed = config.read_bytes()
    doc = tomllib.loads(managed.decode())
    runtime = doc['integrations']['runtime']
    require(runtime['command'] == [args.coven_bin, 'integration', 'runtime', '--state-root', str(state)] and
            runtime['report_command'] == [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--state-root', str(state)]
            and runtime['source'] == 'coven' and runtime['external_agent_management'], 'generated arrays incorrect')
    require(doc['integrations']['sources'] == ['coven', 'build'] and
            b'# existing source comment' in managed and b'# keep this comment' in managed and
            managed.startswith(original.split(b'[integrations]')[0]) and not state.exists() and not enabled['live_api_used'],
            'enable lost unrelated bytes or started runtime')
    passed('enable --apply preserves unrelated config/source/comments; generated arrays have no API/receiver/workspace pins')
    before = tree(phase)
    recorded = setup(args, env, phase, 'plan')
    require(all(recorded[key] == enabled[key] for key in
                ('executable', 'state_root', 'command', 'report_command')) and
            recorded['state_root'] == str(state) and tree(phase) == before and
            not state.exists() and not (root / 'd').exists(), 'bare plan lost recorded custom pins or mutated filesystem')
    passed('bare integration plan selects recorded custom root/executable/argv with zero filesystem footprint')
    before = tree(phase)
    require(setup(args, env, phase, 'enable', state, True)['noop'] and tree(phase) == before, 'enable not idempotent')
    passed('repeated enable --apply is byte/inode/tree-idempotent')
    require(not state.exists(), 'concurrent bootstrap must start against a fresh shared state root')
    # Spawn both production servers before any API/receipt/readiness wait. This
    # permits startup admission to overlap without adding test hooks to core.
    launch_a = launch(args, env, phase, 'a')
    launch_b = launch(args, env, phase, 'b')
    a = ready(args, phase, state, 'a', *launch_a)
    b = ready(args, phase, state, 'b', *launch_b)
    require(a['directory'] != b['directory'] and a['receiver'] != b['receiver'] and
            a['receipt']['nonce'] != b['receipt']['nonce'], 'bootstrap instances were shared')
    passed('both actual Kit servers spawned before readiness waits on fresh shared root/config/executable; private instances distinct')
    passed('generated runtime is a direct Kit-owned actual Coven bootstrap child with private initialized state')
    passed('actual new PTY pane gets same-build --from-kitsune reporter/source/external route, replacing inherited poison')
    pa, pb = Plugin(args, a['pane_env'], phase), Plugin(args, b['pane_env'], phase)
    pa.event('session.created', info={'id': 'PRIVATE-A'})
    pa.event('session.status', sessionID='PRIVATE-A', status={'type': 'busy'})
    a['view'].expect_row('Djinn', 'WORKING (fresh)')
    pb.event('session.created', info={'id': 'PRIVATE-B'})
    pb.event('session.status', sessionID='PRIVATE-B', status={'type': 'idle'})
    b['view'].expect_row('Djinn', 'IDLE (fresh)')
    a['view'].expect_row('Djinn', 'WORKING (fresh)')
    require('IDLE (fresh)' not in a['view'].text and 'WORKING (fresh)' not in b['view'].text, 'events crossed instances')
    passed('actual Djinn plugin events through each real pane environment remain isolated across simultaneous Kit servers')
    provider_routes(args, phase, a, b)
    native = json.dumps({'contract': 'djinn.native-events.v1', 'instance_id': '11111111-1111-4111-8111-111111111111',
                         'session_id': 'PRIVATE-ROUTE-CHECK', 'sequence': 1, 'event': 'session.created'})
    route = [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--json']
    before = tree(state)
    refused(route + ['--state-root', str(root / 'other')], a['pane_env'], phase, 'integration_report_root_mismatch', native)
    refused(route, {**a['pane_env'], 'KITSUNE_INTEGRATION_SOURCE': 'wrong'}, phase, 'integration_context_required', native)
    poison = json.loads(a['pane_env']['KITSUNE_INTEGRATION_REPORT_COMMAND'])
    poison[0] = args.kitsune_bin
    refused(route, {**a['pane_env'], 'KITSUNE_INTEGRATION_REPORT_COMMAND': json.dumps(poison)},
            phase, 'integration_report_executable_mismatch', native)
    require(tree(state) == before, 'refused reporters mutated state')
    passed('actual native CLI refuses mismatched root, inherited context, or different selected executable without state mutation')
    pb.close()
    released = stop(b)
    passed('generated bootstrap clean shutdown releases receipt only after receiver cleanup and Kit helper reaping')
    b2 = start(args, env, phase, state, 'b')
    require(b2['directory'] == b['directory'] and b2['receipt']['nonce'] != released['nonce'], 'clean restart did not reuse released state')
    b2['view'].expect('No items')
    sibling = tree(b2['directory'])
    passed('clean actual Kit restart on same API address reuses only released instance with a new generation')
    # A clean primary restart clears volatile isolation-only rows, preventing an
    # older root's expiry from satisfying the later single-root expiry assertion.
    pa.close()
    stop(a)
    a = start(args, env, phase, state, 'a')
    pa = Plugin(args, a['pane_env'], phase)
    native_checks(args, phase, a['kit'], a['receiver'], a['server'], a['helper'],
                  a['pane_id'], a['pane_env'], a['view'], pa)
    stale = tree(a['directory'])
    require(not json.loads((a['directory'] / 'receipt.json').read_text())['released'], 'crash falsely released instance')
    crashed, kit = launch(args, env, phase, 'a')
    wait_for(lambda: api(kit, 'session.snapshot'), 'crash-state actual Kit restart')
    time.sleep(1)
    refused([args.coven_bin, 'integration', 'runtime', '--state-root', str(state), '--json'],
            {**env, 'KITSUNE_SOCKET_PATH': str(kit)}, phase, 'integration_socket_conflict')
    failed_view = ViewClient(phase / 'a-client.sock')
    failed_view.expect('Unavailable')
    require(tree(a['directory']) == stale, 'stale crash state was adopted/overwritten')
    refused(route, a['pane_env'], phase, 'integration_inactive_generation', native)
    require(tree(a['directory']) == stale, 'stale reporter repaired state')
    b2['view'].expect('No items')
    require('Coven / Djinn /' not in b2['view'].text and tree(b2['directory']) == sibling and
            next(s for s in api(b2['kit'], 'session.snapshot')['snapshot']['integration_views']['sources']
                 if s['id'] == 'coven')['epoch'] is not None, 'failed route fell back to or damaged live sibling')
    stop(b2)
    failed_view.close()
    api(kit, 'server.stop')
    require(crashed.wait(timeout=10) == 0, 'failed-helper Kit stop failed')
    passed('crashed active state survives unchanged; actual Kit restart stays Unavailable; runtime/reporter fail-closed with live sibling untouched')
    config.write_bytes(managed + b'\n# later user edit must not be lost\n')
    before = tree(config.parent)
    refused([args.coven_bin, 'integration', 'disable', '--apply', '--json'], env, phase, 'integration_setup_config_edited')
    require(tree(config.parent) == before, 'disable refusal changed later user edit or receipt')
    passed('actual disable --apply refuses later user edits without changing config or setup receipt')
    # Restore only this harness-owned edit to test the separate exact-disable path.
    config.write_bytes(managed)
    setup(args, env, phase, 'disable', apply=True)
    require(config.read_bytes() == original and stat.S_IMODE(config.stat().st_mode) == 0o600,
            'disable did not restore exact preimage/source/comments/mode')
    passed('default-config disable --apply restores exact original bytes/source/comments/mode')
    extra = phase / 'default-setup'
    extra.mkdir(mode=0o700)
    default_env = environment(extra)
    default_env['XDG_STATE_HOME'] = str(root / 'd')
    default_config = Path(default_env['XDG_CONFIG_HOME']) / 'kitsune/config.toml'
    default_config.parent.mkdir(mode=0o700)
    source_preimage = b'# source-only setup\n[integrations]\nsources = ["build"] # preserve\n'
    default_config.write_bytes(source_preimage)
    setup(args, default_env, extra, 'enable', apply=True)
    generated = tomllib.loads(default_config.read_text())
    require(generated['integrations']['sources'] == ['build', 'coven'] and
            generated['integrations']['runtime']['command'] ==
            [args.coven_bin, 'integration', 'runtime', '--state-root', str(root / 'd/coven/integration')],
            'fully default enable selection/source append mismatch')
    setup(args, default_env, extra, 'disable', apply=True)
    require(default_config.read_bytes() == source_preimage and not (root / 'd').exists(),
            'fully default disable did not restore source preimage or setup started runtime')
    passed('fully default enable/disable CLI appends missing Coven source and restores exact source-only preimage without runtime state')
