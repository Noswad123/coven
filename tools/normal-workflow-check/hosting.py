#!/usr/bin/env python3
"""Actual source-native A2 producer -> Coven -> Kit, offline and opt-in."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import check
from bootstrap import helper_for, instance, tree


def spawn_bun(argv, env, root, tag, package):
    with (root / (tag + '.log')).open('wb') as log:
        return subprocess.Popen(argv, cwd=package, env=env, stdout=log, stderr=log,
                                start_new_session=True, preexec_fn=check.limits)


def run(args, root):
    env = check.environment(root)
    env['XDG_STATE_HOME'] = str(root / 'v')
    state = root / 'v/coven/integration'
    check.require(len(os.fsencode(state / 'i' / ('0' * 24) / 'r.sock')) < 108, 'default receiver path too long')
    bindir = root / 'bin'
    bindir.mkdir(mode=0o700)
    (bindir / 'coven').symlink_to(args.coven_bin)
    env['PATH'] = str(bindir) + ':/usr/bin:/bin'
    config = root / 'kit.toml'
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
                      '[session]\nresume_agents_on_restore = false\n[agent_detection]\nmanifest_check = false\n'
                      '[ui]\nsidebar_width = 160\nsidebar_min_width = 160\nsidebar_max_width = 160\n'
                      '[integrations]\nsources = ["coven"]\n')
    kit = root / 'k.sock'
    env.update(KITSUNE_CONFIG_PATH=str(config), KITSUNE_SOCKET_PATH=str(kit),
               KITSUNE_SESSION='a2-interop', KITSUNE_LOG='warn')
    workspace = root / 'w'
    check.command([args.coven_bin, 'init', str(workspace), '--blank'], env, root)
    server = check.spawn([args.kitsune_bin, 'server'], env, root, 'kit', stdin=subprocess.DEVNULL)
    producer = view = None
    try:
        check.wait_for(lambda: check.api(kit, 'session.snapshot'), 'actual Kit startup')
        directory, receipt = check.wait_for(lambda: instance(state, kit), 'default receiver receipt')
        helper = check.wait_for(lambda: helper_for(args, server), 'actual Coven helper')
        check.require(Path(f'/proc/{helper}/cmdline').read_bytes().split(b'\0')[:-1] ==
                      [os.fsencode(x) for x in [args.coven_bin, 'integration', 'runtime', '--state-root', str(state)]],
                      'helper argv mismatch')
        receiver = directory / 'r.sock'
        pane, pane_env = check.pane_environment(kit, root, workspace)
        check.require(json.loads(pane_env['KITSUNE_INTEGRATION_REPORT_COMMAND']) ==
                      [args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--state-root', str(state)],
                      'actual pane report route mismatch')
        # Establish deterministic headless geometry before asserting visible PTY content.
        view = check.ViewClient(root / 'k-client.sock')
        view.expect('Integrations', 'coven')

        def pane_text(pane_id):
            return check.api(kit, 'pane.read', dict(pane_id=pane_id, source='recent_unwrapped',
                                                   format='text', lines=200, strip_ansi=True))['read']['text']

        def generic_canary(name):
            marker = root / name
            check.api(kit, 'pane.send_input', dict(pane_id=pane,
                      text='printf "READ_OK\\n"; : > ' + shlex.quote(str(marker)), keys=['enter']))
            check.wait_for(marker.exists, 'generic shell ' + name)

        def navigation_state():
            snapshot = check.api(kit, 'session.snapshot')['snapshot']
            check.require(snapshot['agents'] == [], 'external snapshot exposed legacy agent inventory')
            return {key: snapshot.get(key) for key in
                    ('focused_workspace_id', 'focused_tab_id', 'focused_pane_id')} | {
                        'terminals': sorted((p['pane_id'], p['terminal_id']) for p in snapshot['panes']),
                        'history': sorted((p['pane_id'], p.get('agent_session')) for p in snapshot['panes'])}

        generic_canary('before-refusals')
        before = navigation_state()
        terminal = dict(before['terminals'])[pane]
        sentinel = 'MUST_NOT_ENQUEUE'
        controls = [
            ('agent.start', dict(name='gate-agent', kind='djinn', pane_id=pane,
                                 args=[sentinel], timeout_ms=4000)),
            ('agent.view.set', dict(source='gate.views', label='Gate', filter=None, sort=[])),
            ('agent.view.clear', dict(source='gate.views')),
            ('agent.view.clear', {}),
        ]
        # Empty/missing selectors must not resolve to the focused pane or bypass ownership.
        for selector in (pane, terminal, 'gate-missing-agent', ''):
            controls.extend([
                ('agent.get', dict(target=selector)),
                ('agent.focus', dict(target=selector)),
                ('agent.rename', dict(target=selector, name='gate-renamed')),
                ('agent.prompt', dict(target=selector, text=sentinel)),
                ('agent.send_keys', dict(target=selector, keys=['enter'])),
                ('agent.wait', dict(target=selector, until=['idle'], timeout_ms=0)),
                ('agent.prompt', dict(target=selector, text=sentinel,
                                     wait=dict(until=['idle'], timeout_ms=0))),
            ])
        for method, params in controls:
            response = check.rpc(kit, method, params)
            check.require(set(response) == {'id', 'error'} and
                          response['error'].get('code') == 'external_agent_management' and
                          sentinel not in json.dumps(response),
                          method + ' did not return a correlated private external-ownership refusal')
            check.require(navigation_state() == before, method + ' refusal changed focus/terminal mappings')
        generic_canary('after-refusals')
        for pane_id, _ in before['terminals']:
            check.require(sentinel not in pane_text(pane_id), 'refused prompt reached a generic PTY')
        check.require(navigation_state() == before, 'refusals/read facades changed focus/terminal mappings')
        check.passed(f'Kit external ownership: {len(controls)} correlated legacy-control refusals including wait/prompt-wait; '
                     'no target fallback, focus/terminal changes or prompt sentinel; generic shell retained')

        reads = [('agent.list', {}), ('server.agent_manifests', {}), ('server.reload_agent_manifests', {})]
        private_selector = 'PRIVATE-MUST-NOT-ECHO-SELECTOR'
        for selector in (pane, terminal, 'gate-missing-agent', '', private_selector):
            reads.extend([
                ('agent.read', dict(target=selector, source='detection', format='text', lines=20, strip_ansi=True)),
                ('agent.explain', dict(target=selector)),
            ])
        for method, params in reads:
            response = check.rpc(kit, method, params)
            check.require(set(response) == {'id', 'error'} and
                          response['error'].get('code') == 'external_agent_management',
                          method + ' did not refuse before legacy lookup/cache access')
            # Static ownership guidance must not expose even a real pane/terminal selector.
            check.require(all(value not in json.dumps(response['error']) for value in
                              (pane, terminal, 'gate-missing-agent', private_selector)),
                          method + ' echoed a private selector')
            check.require(navigation_state() == before, method + ' refusal changed topology/history/focus')

        cli_root = root / 'cli'
        cli_root.mkdir(mode=0o700)
        cli_env = check.environment(cli_root)
        cli_env.update(KITSUNE_SOCKET_PATH=str(kit), KITSUNE_SESSION=env['KITSUNE_SESSION'],
                       KITSUNE_AGENT_DETECTION_MANIFEST_CATALOG_URL='interop-invalid://MUST_NOT_FETCH/catalog.toml')
        cache_before = tree(cli_root)
        result = subprocess.run([args.kitsune_bin, 'server', 'update-agent-manifests', '--json'],
                                env=cli_env, cwd=cli_root, capture_output=True, text=True,
                                timeout=10, preexec_fn=check.limits)
        check.require(result.returncode == 1 and not result.stdout and len(result.stderr) < 16384,
                      'built manifest-update CLI did not return its bounded JSON error')
        response = json.loads(result.stderr)
        check.require(set(response) == {'id', 'error'} and response['id'] == 'cli:server:agent-manifests' and
                      response['error'].get('code') == 'external_agent_management' and
                      'MUST_NOT_FETCH' not in result.stderr,
                      'built CLI did not propagate selected-server preflight refusal before local update')
        check.require(tree(cli_root) == cache_before, 'manifest-update CLI mutated isolated cache/status/config tree')
        check.require(navigation_state() == before, 'manifest-update CLI changed topology/history/focus')
        snapshot = check.api(kit, 'session.snapshot')['snapshot']
        check.require(snapshot['terminal_navigation'] == {'version': 1} and
                      snapshot['process_observation'] == dict(version=1, platform='linux',
                                                              scope='pane_foreground_cohort', selection='unclassified'),
                      'generic navigation/process descriptor unavailable')
        for source in ('visible', 'recent', 'recent_unwrapped', 'detection'):
            read = check.api(kit, 'pane.read', dict(pane_id=pane, source=source, format='text',
                                                   lines=200, strip_ansi=True))['read']
            check.require(read['pane_id'] == pane and isinstance(read['text'], str) and
                          'READ_OK' in read['text'], 'generic ' + source + ' text read unavailable')
        process = check.api(kit, 'pane.process_info', dict(pane_id=pane))['process_info']
        check.require(process['pane_id'] == pane and process.get('shell_pid') in check.descendants(),
                      'generic process inspection did not identify an owned fixture shell')
        check.require(navigation_state() == before, 'generic read/process inspection changed topology/history/focus')
        check.passed(f'Kit read/manifest retirement: {len(reads)} correlated typed refusals without selector echo; '
                     'built CLI preflight refused before invalid catalog/update with unchanged cache/status tree; '
                     'snapshot agents empty, generic text/detection/process and navigation.v1 retained')

        package = args.djinn_source.resolve() / 'clients/djinn-ui/packages/opencode'
        benv = {**env, 'COVEN_HOSTING_INTEROP': str(root), 'OPENCODE_DISABLE_DEFAULT_PLUGINS': 'true',
                'OPENCODE_DISABLE_MODELS_FETCH': 'true'}
        producer = spawn_bun([args.bun, 'test', '--timeout', '120000', './test/coven/hosting-interop.test.ts'],
                             benv, root, 'producer', package)

        def private_file(name):
            file = root / (name + '.json')
            if file.exists():
                return json.loads(file.read_text())
            check.require(producer.poll() is None, 'producer exited: ' + (root / 'producer.log').read_text()[-10000:])
            return None

        ready = check.wait_for(lambda: private_file('ready'), 'real SDK roots and actual capture', 40)
        context = ready['context']
        target = context['target']
        inspected = check.api(kit, 'terminal.inspect', {'terminal_id': target['terminal_id']})
        check.require(inspected['pane_id'] == pane and inspected['runtime_generation'] == target['runtime_generation'],
                      'source capture did not pin actual new pane runtime')
        ids = {name: hashlib.sha256(('native\0' + ready['instance'] + '\0' + ready[name]).encode()).hexdigest()
               for name in ('a', 'b')}
        number = 0

        def operation(value):
            nonlocal number
            number += 1
            path = root / f'command-{number}.json'
            pending = path.with_suffix('.new')
            pending.write_text(json.dumps({'operation': value}))
            pending.replace(path)
            return check.wait_for(lambda: private_file(f'reply-{number}'), 'producer ' + value)

        def cli(*argv):
            return json.loads(check.command([args.coven_bin, *map(str, argv), '--json'], pane_env, root))

        def observations():
            result = cli('agent', 'observations', '--from-kitsune')
            public = json.dumps(result)
            check.require(all(value not in public for value in (ready['instance'], ready['a'], ready['b'], target['terminal_id'])),
                          'raw identity/target in public observations')
            return result

        def row(name):
            return next((r for r in observations()['observations'] if r['observation_id'] == ids[name]), None)

        def hosting(name, value):
            def matches():
                r = row(name)
                return r if r and r['hosting_state'] == value else None
            return check.wait_for(matches, name + ' hosting ' + value)

        def focus(name, revision, execute=True):
            return cli('agent', 'focus', ids[name], '--from-kitsune', '--expected-epoch', context['server_epoch'],
                       '--expected-revision', revision, *(['--execute'] if execute else []))

        def refuse_focus(name, revision):
            result = subprocess.run([args.coven_bin, 'agent', 'focus', ids[name], '--from-kitsune', '--expected-epoch',
                                     context['server_epoch'], '--expected-revision', revision, '--execute', '--json'],
                                    env=pane_env, cwd=root, capture_output=True, text=True, timeout=10, preexec_fn=check.limits)
            check.require(result.returncode != 0 and 'focus_accepted' not in result.stdout, 'obsolete association focused')

        operation('a')
        a = hosting('a', 'local_full_tui_root')
        check.require(a['origin'] == 'djinn_client_attachment' and a['reported_state'] == 'unknown' and a['age_ms'] is None,
                      'quiet root fabricated activity')
        check.require(row('b') is None, 'known unselected backend root materialized/hosted')
        # Inspect private binding using the actual receiver's closed control protocol.
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(2)
            sock.connect(str(receiver))
            sock.sendall((json.dumps(dict(contract='coven.observation-control.v1', id='a2' * 16, method='association.inspect',
                                         params=dict(observation_id=ids['a'], expected_epoch=context['server_epoch'],
                                                     expected_revision=a['association_revision']))) + '\n').encode())
            line = sock.makefile('r').readline(65536)
            check.require(line, 'receiver closed private association.inspect without reply')
            reply = json.loads(line)
        binding = reply.get('result', {})
        check.require(binding.get('provenance') == 'producer_reported' and binding.get('native_hosting') == 'local_full_tui_root'
                      and binding.get('target') == target, 'private producer binding mismatch')
        view.expect('Djinn', 'UNKNOWN')

        def privacy():
            projection = json.dumps(check.api(kit, 'session.snapshot')['snapshot']['integration_views'])
            text = view.poll()
            check.require(all(value not in projection + text for value in
                              (ready['instance'], ready['a'], ready['b'], target['terminal_id'], target['runtime_generation'])),
                          'raw native identity/target leaked into generic projection/frame')
        privacy()
        check.passed('A2 quiet SDK root A: native factory UUID hash joins; UNKNOWN/null age; private producer binding; B unselected; generic privacy')
        first_sample = operation('sample')
        time.sleep(6.2)
        second_sample = operation('sample')
        renewed = hosting('a', 'local_full_tui_root')
        check.require(second_sample['sends'] >= first_sample['sends'] + 5 and renewed['association_revision'] == a['association_revision']
                      and renewed['age_ms'] is None and renewed['reported_state'] == 'unknown', 'heartbeats lost lease or fabricated activity')
        check.passed('A2 real 1s heartbeats maintain real 5s lease beyond TTL; stable association revision, quiet activity remains null')
        snapshot = check.api(kit, 'session.snapshot')['snapshot']
        other = next(p for p in snapshot['panes'] if p['pane_id'] != pane)
        other_info = check.api(kit, 'terminal.inspect', {'terminal_id': other['terminal_id']})
        check.api(kit, 'terminal.focus', dict(terminal_id=other['terminal_id'], expected_runtime_generation=other_info['runtime_generation']))
        check.require(focus('a', a['association_revision'])['outcome'] == 'focus_accepted', 'producer-backed CLI focus refused')
        check.require(check.api(kit, 'session.snapshot')['snapshot']['focused_pane_id'] == pane, 'actual focus did not move')
        rejected = check.rpc(kit, 'terminal.focus', dict(terminal_id=other['terminal_id'], expected_runtime_generation='00000000-0000-4000-8000-000000000000'))
        check.require(rejected.get('error', {}).get('code') == 'runtime_generation_mismatch' and
                      check.api(kit, 'session.snapshot')['snapshot']['focused_pane_id'] == pane, 'wrong generation focus accepted')
        check.passed('A2 actual CLI producer-backed focus accepted exact Kit generation without user assertion; wrong generation refused')
        operation('native')
        native = check.wait_for(lambda: (r if (r := row('a')) and r['origin'] == 'djinn_native_event' and
                                        r['unacknowledged_pending_count'] == 1 else None), 'actual reporter native-stream identity join')
        check.require(len(observations()['observations']) == 1 and native['reported_state'] == 'blocked', 'native identity fork/status mismatch')
        check.passed('A2 actual native adapter/reporter status and decision inputs join same attachment observation, no UUID fork')
        operation('b')
        b = hosting('b', 'local_full_tui_root')
        released_a = hosting('a', 'none')
        check.require(released_a['unacknowledged_pending_count'] == 1 and released_a['reported_state'] == 'blocked', 'selection cleared native decision/status')
        refuse_focus('a', a['association_revision'])
        operation('leave')
        hosting('b', 'none')
        refuse_focus('b', b['association_revision'])
        check.passed('A2 actual selected-load dispose/reload A -> B -> leave releases hosting; stale focus refused; native decisions preserved')
        operation('a')
        current = hosting('a', 'local_full_tui_root')
        operation('freeze')
        time.sleep(5.3)
        hosting('a', 'none')
        refuse_focus('a', current['association_revision'])
        before_stale = operation('stale')
        hosting('a', 'none')
        check.require(operation('sample')['sends'] == before_stale['sends'], 'frozen frontend renewed')
        check.passed('A2 lost-detach/renewal-stop: real 5s lease expires; stale callback cannot revive; old focus fence refuses')
        operation('a')
        current = hosting('a', 'local_full_tui_root')
        operation('dispose-plugin')
        hosting('a', 'none')
        refuse_focus('a', current['association_revision'])
        check.require(row('a')['unacknowledged_pending_count'] == 1, 'plugin disposal cleared pending decision')
        privacy()
        check.passed('A2 actual plugin disposal triggers worker higher-revision release; pending decision preserved')
        operation('done')
        check.require(producer.wait(timeout=15) == 0, 'producer teardown failed')
        print((root / 'producer.log').read_text()[-6000:], flush=True)
    finally:
        if producer and producer.poll() is None:
            producer.terminate()
            producer.wait(timeout=10)
        if view:
            view.close()
        if server.poll() is None:
            check.api(kit, 'server.stop')
        check.require(server.wait(timeout=10) == 0, 'actual Kit shutdown failed')
    released = json.loads((directory / 'receipt.json').read_text())
    check.require(released['released'] and released['nonce'] == receipt['nonce'] and
                  not receiver.exists() and not Path(f'/proc/{helper}').exists(), 'helper/receipt shutdown failed')
    check.stop_children()
    check.require(not check.descendants(), 'owned descendants remain')
    print('SHUTDOWN verified: actual server.stop, helper reaped, same receipt generation released, no owned descendants', flush=True)
    print(f'SUMMARY {check.PASS_COUNT} A2 composed scenarios passed', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('coven-bin', 'kitsune-bin', 'bun'):
        parser.add_argument('--' + name, required=True, type=check.absolute)
    parser.add_argument('--djinn-source', required=True, type=Path)
    args = parser.parse_args()
    try:
        with check.owned_run() as root:
            run(args, root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
