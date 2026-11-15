#!/usr/bin/env python3
"""Small opt-in Linux composed acceptance; no live providers or user configuration."""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

sys.dont_write_bytecode = True
import check
from bootstrap import helper_for, instance, tree


def run(args, root):
    env = check.environment(root)
    env['XDG_STATE_HOME'] = str(root / 'v')
    state = root / 'v/coven/integration'
    check.require(len(os.fsencode(state / 'i' / ('0' * 24) / 'r.sock')) < 108,
                  'cache path too long for default receiver')
    bindir = root / 'bin'
    bindir.mkdir(mode=0o700)
    (bindir / 'coven').symlink_to(args.coven_bin)
    env['PATH'] = str(bindir) + ':/usr/bin:/bin'
    config = root / 'kit.toml'
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\n'
                      'shell_mode = "non_login"\n[session]\nresume_agents_on_restore = false\n'
                      '[agent_detection]\nmanifest_check = false\n'
                      '[ui]\nsidebar_width = 160\nsidebar_min_width = 160\nsidebar_max_width = 160\n'
                      '[integrations]\nsources = ["coven"]\n')
    kit = root / 'k.sock'
    env.update(KITSUNE_CONFIG_PATH=str(config), KITSUNE_SOCKET_PATH=str(kit),
               KITSUNE_SESSION='adoption', KITSUNE_LOG='kitsune=debug')
    workspace = root / 'w'
    check.command([args.coven_bin, 'init', str(workspace), '--blank'], env, root)
    server = check.spawn([args.kitsune_bin, 'server'], env, root, 'kit', stdin=subprocess.DEVNULL)
    view = None
    try:
        check.wait_for(lambda: check.api(kit, 'session.snapshot'), 'actual Kit startup')
        directory, receipt = check.wait_for(lambda: instance(state, kit), 'default active receipt')
        helper = check.wait_for(lambda: helper_for(args, server), 'direct actual Coven helper')
        check.require(Path(f'/proc/{helper}/cmdline').read_bytes().split(b'\0')[:-1] ==
                      [os.fsencode(x) for x in [args.coven_bin, 'integration', 'runtime', '--state-root', str(state)]],
                      'production helper argv mismatch')
        receiver = directory / 'r.sock'
        check.require(receiver.is_socket(), 'default receiver missing')
        pane, pane_env = check.pane_environment(kit, root, workspace)
        expected_report = [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--state-root', str(state)]
        check.require(json.loads(pane_env['KITSUNE_INTEGRATION_REPORT_COMMAND']) == expected_report and
                      pane_env['KITSUNE_SOCKET_PATH'] == str(kit) and
                      pane_env['KITSUNE_AGENT_MANAGEMENT'] == 'external' and
                      pane_env['KITSUNE_INTEGRATION_SOURCE'] == 'coven', 'actual pane route mismatch')
        view = check.ViewClient(root / 'k-client.sock')
        view.expect('Integrations', 'coven')

        def cli(*argv, payload=None, context=pane_env):
            return json.loads(check.command([args.coven_bin, *map(str, argv), '--json'], context, root, payload))

        def observations():
            result = cli('agent', 'observations', '--from-kitsune')
            check.require('PRIVATE-' not in json.dumps(result), 'raw native ID in observations')
            return result

        def row():
            return observations()['observations'][0]

        sequence = 0

        def event(kind, **fields):
            nonlocal sequence
            sequence += 1
            payload = dict(contract='djinn.native-events.v1',
                           instance_id='11111111-1111-4111-8111-111111111111',
                           session_id='PRIVATE-INERT-ROOT', sequence=sequence, event=kind, **fields)
            cli('agent', 'report-native', '--from-kitsune', payload=json.dumps(payload))

        def pending(count, unacked, status='blocked'):
            value = row()
            check.require(value['reported_state'] == status and value['fresh_pending_count'] == count and
                          value['unacknowledged_pending_count'] == unacked, f'unexpected native facts: {value}')
            return value

        def log_text():
            logs = list(root.rglob('kitsune-server.log'))
            check.require(len(logs) == 1, f'expected actual Kit server log: {logs}')
            check.require(logs[0].stat().st_size <= 32 * 1024**2, 'Kit log exceeded cap')
            return logs[0].read_text()

        def methods(text):
            return re.findall(r'event="api.request.start"[^\n]*?method="([^"]+)"', text)

        # Synthetic native input uses only the real reporter and real default receiver.
        event('session.created')
        event('session.status', status='busy')
        pending(0, 0, 'working')
        view.expect_row('Djinn', 'WORKING (fresh)')
        event('question.asked', request_id='PRIVATE-Q1')
        first = pending(1, 1)
        view.expect_row('Djinn', 'BLOCKED (fresh)')
        attention = cli('agent', 'attention', 'list', '--from-kitsune')
        check.require('PRIVATE-' not in json.dumps(attention), 'raw native ID in attention')
        epoch, obs = attention['server_epoch'], first['observation_id']
        ack = ['agent', 'attention', 'acknowledge', obs, '--from-kitsune', '--expected-epoch', epoch,
               '--expected-revision', first['attention_revision']]
        check.require(cli(*ack)['applied'] is False, 'ack preview applied')
        check.require(pending(1, 1)['attention_revision'] == first['attention_revision'], 'preview mutated attention')
        check.require(cli(*ack, '--apply')['observations'][0]['unacknowledged_pending_count'] == 0,
                      'applied acknowledgement did not clear count')
        pending(1, 0)
        view.expect_row('Djinn', 'BLOCKED (fresh)')
        event('permission.asked', request_id='PRIVATE-P1')
        pending(2, 1)
        event('question.replied', request_id='PRIVATE-NOT-Q1')
        pending(2, 1)
        event('question.replied', request_id='PRIVATE-Q1')
        pending(1, 1)
        event('permission.replied', request_id='PRIVATE-P1')
        pending(0, 0, 'working')
        event('question.asked', request_id='PRIVATE-Q1')
        pending(1, 1)
        snapshot = check.api(kit, 'session.snapshot')['snapshot']
        check.require('PRIVATE-' not in json.dumps(snapshot['integration_views']), 'raw ID in generic projection')
        check.passed('scenario 1: real pane route/report-native/receiver/render; busy, asks, ack preview/apply, matching replies, rearmed ask; opaque projections')

        # Only checked generic navigation, never an assertion of provider attachment.
        check.require(len(snapshot['panes']) == 2 and snapshot['terminal_navigation'] == {'version': 1},
                      'expected two real panes and terminal_navigation.v1')
        target = next(p for p in snapshot['panes'] if p['pane_id'] == pane)
        other = next(p for p in snapshot['panes'] if p['pane_id'] != pane)
        terminal = target['terminal_id']
        inspected = check.api(kit, 'terminal.inspect', {'terminal_id': terminal})
        check.require(inspected['availability'] == 'live' and inspected['pane_id'] == pane and
                      inspected['runtime_generation'], 'exact terminal inspection failed')
        before_log = log_text()
        associate = ['agent', 'associate', obs, '--from-kitsune', '--kitsune-socket', str(kit),
                     '--terminal', terminal, '--expected-epoch', epoch]
        preview = cli(*associate)
        check.require(not preview['applied'] and preview['association_revision'] is None, 'association preview mutated')
        association = cli(*associate, '--apply')
        check.require(association['applied'] and association['provenance'] == 'user_asserted' and
                      association['native_hosting'] == 'unestablished' and
                      association['target']['runtime_generation'] == inspected['runtime_generation'], 'association pins mismatch')
        focus = ['agent', 'focus', obs, '--from-kitsune', '--expected-epoch', epoch,
                 '--expected-revision', association['association_revision']]
        other_inspected = check.api(kit, 'terminal.inspect', {'terminal_id': other['terminal_id']})
        check.api(kit, 'terminal.focus', {'terminal_id': other['terminal_id'],
                  'expected_runtime_generation': other_inspected['runtime_generation']})
        cli(*focus)
        check.require(check.api(kit, 'session.snapshot')['snapshot']['focused_pane_id'] == other['pane_id'],
                      'focus preview changed focus')
        check.require(cli(*focus, '--execute')['outcome'] == 'focus_accepted', 'Coven focus not accepted')
        check.require(check.api(kit, 'session.snapshot')['snapshot']['focused_pane_id'] == pane, 'actual focus did not move')
        rejected = check.rpc(kit, 'terminal.focus', {'terminal_id': other['terminal_id'],
                             'expected_runtime_generation': '00000000-0000-4000-8000-000000000000'})
        check.require(rejected.get('error', {}).get('code') == 'runtime_generation_mismatch' and
                      check.api(kit, 'session.snapshot')['snapshot']['focused_pane_id'] == pane,
                      'stale generation changed focus or was accepted')
        navigation_methods = methods(log_text()[len(before_log):])
        check.require(navigation_methods.count('terminal.focus') == 3 and
                      set(navigation_methods) <= {'terminal.focus', 'terminal.inspect', 'session.snapshot',
                                                  'integration.view.replace', 'integration.view.poll'},
                      f'unexpected navigation API calls: {navigation_methods}')
        check.require(row()['unacknowledged_pending_count'] == 1, 'navigation acknowledged attention')
        projection = json.dumps(check.api(kit, 'session.snapshot')['snapshot']['integration_views'])
        check.require('PRIVATE-' not in projection and terminal not in projection,
                      'native ID or private navigation target leaked into generic projection')
        print('NAVIGATION raw methods: ' + json.dumps(navigation_methods), flush=True)
        check.passed('scenario 2: real exact-generation inspection, user-asserted preview/apply, Coven checked focus, stale rejection; no legacy focus/input fallback')

        true = Path('/usr/bin/true')
        check.require(true.is_file() and os.access(true, os.X_OK), 'inert system true unavailable')
        # An explicit command confirms the generic shell is quiescent after true exits.
        def quiet():
            marker = root / 'quiet'
            if marker.exists():
                marker.unlink()
            check.api(kit, 'pane.send_input', {'pane_id': pane, 'text': 'touch ' + shlex.quote(str(marker)), 'keys': ['enter']})
            check.wait_for(marker.exists, 'quiescent generic shell')

        quiet()
        start = ['session', 'resume', str(workspace), '--runtime', 'claude', '--executable', str(true),
                 '--socket', str(kit), '--pane', pane, '--expected-terminal', terminal, '--session', 'inert-a1']
        before_log = log_text()
        check.require(cli(*start)['outcome'] == 'preview', 'A1 preview not read-only')
        check.require('pane.submit_command' not in methods(log_text()[len(before_log):]), 'A1 preview enqueued')
        a1 = cli(*start, '--execute')
        check.require(a1['outcome'] == 'input_queued' and a1['native_lifecycle'] == 'unestablished', f'A1 not queued: {a1}')
        quiet()
        check.require(methods(log_text()[len(before_log):]).count('pane.submit_command') == 1, 'A1 enqueue count mismatch')
        check.passed('scenario 3: additional-provider claude typed resume policy queues only /usr/bin/true; native lifecycle unestablished')

        entry = dict(key='selected', reference=dict(source='kitsune:codex', runtime='codex', kind='id', value='inert-a5'),
                     executable=str(true), pane_id=pane, expected_terminal_id=terminal)
        selection = root / 'selection.json'
        selection.write_text(json.dumps(dict(schema_version=1, entries=[entry, {**entry, 'key': 'duplicate'}])))
        journal = workspace / 'state/session-recovery.json'
        inspect = lambda: cli('session', 'recovery', 'inspect', str(workspace))
        revision = inspect()['recovery_revision']
        recovery = ['session', 'recovery', 'run', str(workspace), '--file', str(selection), '--socket', str(kit)]
        before = tree(workspace)
        before_log = log_text()
        preview = cli(*recovery, '--expect-recovery-revision', revision)
        check.require([e['outcome'] for e in preview['entries']] == ['preview', 'skipped_duplicate'] and
                      tree(workspace) == before and not journal.exists(), 'recovery preview mutated or wrong duplicate policy')
        check.require('pane.submit_command' not in methods(log_text()[len(before_log):]), 'preview enqueued input')
        executed = cli(*recovery, '--expect-recovery-revision', revision, '--execute')
        check.require([e['outcome'] for e in executed['entries']] == ['queued', 'skipped_duplicate'], f'recovery not queued: {executed}')
        quiet()
        persisted = journal.read_bytes()
        check.require(len(json.loads(persisted)['attempts']) == 1 and
                      json.loads(persisted)['attempts'][0]['outcome'] == 'queued', 'unexpected recovery journal')
        fresh = inspect()['recovery_revision']
        check.require(fresh != revision, 'recovery revision unchanged')
        again = cli(*recovery, '--expect-recovery-revision', fresh, '--execute')
        check.require(again['entries'][0]['outcome'] == 'skipped_journal' and journal.read_bytes() == persisted,
                      f'new CLI process did not honor durable duplicate blocker: {again}')
        recovery_methods = methods(log_text()[len(before_log):])
        check.require(recovery_methods.count('pane.submit_command') == 1, f'recovery did not queue exactly once: {recovery_methods}')
        check.require(all(e['native_running'] == 'unestablished' for e in again['entries']), 'recovery claimed native running')
        print('RECOVERY raw methods: ' + json.dumps(recovery_methods), flush=True)
        check.passed('scenario 4: inspect/read-only preview, explicit CAS execute, selected duplicate skip; one actual enqueue and durable fresh-process duplicate blocker')
    finally:
        if view:
            view.close()
        if server.poll() is None:
            check.api(kit, 'server.stop')
        check.require(server.wait(timeout=10) == 0, 'actual Kit shutdown failed')
    released = json.loads((directory / 'receipt.json').read_text())
    check.require(released['released'] and released['nonce'] == receipt['nonce'] and
                  not receiver.exists() and not Path(f'/proc/{helper}').exists(),
                  'helper/receipt/receiver shutdown cleanup failed')
    print('SHUTDOWN verified: server.stop; helper reaped; same receipt released; receiver absent', flush=True)
    print(f'SUMMARY {check.PASS_COUNT} composed scenarios passed', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--coven-bin', required=True, type=check.absolute)
    parser.add_argument('--kitsune-bin', required=True, type=check.absolute)
    try:
        args = parser.parse_args()
        with check.owned_run() as root:
            run(args, root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
