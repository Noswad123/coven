#!/usr/bin/env python3
"""Actual source-native A2 producer -> Coven -> Kit, offline and opt-in."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shlex
import select
import socket
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import check
from bootstrap import helper_for, instance, tree


MANIFEST_RETIREMENT_MESSAGE = ('Built-in detector-manifest management has been retired; Kitsune no longer '
                               'downloads or reloads agent-detection manifests. Generic pane/process operations remain available.')


def retirement_error(method):
    return dict(code='feature_retired', message=f'Legacy agent operation {method} is retired; Kitsune no longer '
                'provides built-in agent lifecycle, readiness, or status. Use generic terminal/pane operations, '
                'or an external agent manager.')


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
                      '[ui]\nsidebar_width = 160\nsidebar_min_width = 160\nsidebar_max_width = 160\n'
                      '[integrations]\nsources = ["coven"]\n')
    kit = root / 'k.sock'
    env.update(KITSUNE_CONFIG_PATH=str(config), KITSUNE_SOCKET_PATH=str(kit),
               KITSUNE_SESSION='a2-interop', KITSUNE_LOG='warn')
    workspace = root / 'w'
    check.command([args.coven_bin, 'init', str(workspace), '--blank'], env, root)
    # Actual dev-build legacy paths: positive stale inputs must survive startup and shutdown untouched.
    retired_cache = root / 'v/kitsune-dev/agent-detection'
    (retired_cache / 'remote').mkdir(parents=True, mode=0o700)
    (retired_cache / 'remote/codex.toml').write_text('id = "codex"\nversion = "9999.1"\n'
                                                  'min_engine_version = 1\nupdated_at = "2026-06-10T12:00:00Z"\n'
                                                  '[[rules]]\nid = "stale-remote"\nstate = "blocked"\n'
                                                  'contains = ["STALE_REMOTE_MUST_NOT_LOAD"]\n')
    (retired_cache / 'status.toml').write_text('last_check_unix = 123\nlast_result = "stale-status"\n'
                                            '[agents.codex]\ncached_version = "9999.1"\nlast_result = "updated"\n')
    retired_before = tree(retired_cache)
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
        view.expect('▾ Coven')

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
                          response['error'] == retirement_error(method) and
                          sentinel not in json.dumps(response),
                          method + ' did not return a correlated private external-ownership refusal')
            check.require(navigation_state() == before, method + ' refusal changed focus/terminal mappings')
        generic_canary('after-refusals')
        for pane_id, _ in before['terminals']:
            check.require(sentinel not in pane_text(pane_id), 'refused prompt reached a generic PTY')
        check.require(navigation_state() == before, 'refusals/read facades changed focus/terminal mappings')
        check.passed(f'Kit global Agent retirement: {len(controls)} correlated feature_retired refusals including wait/prompt-wait; '
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
            manifest = method.startswith('server.')
            check.require(set(response) == {'id', 'error'} and
                          response['error'] == (dict(code='feature_retired', message=MANIFEST_RETIREMENT_MESSAGE)
                                                if manifest else retirement_error(method)),
                          method + ' did not return its exact retirement/ownership error')
            if manifest:
                check.require(response['error'] == dict(code='feature_retired', message=MANIFEST_RETIREMENT_MESSAGE),
                              method + ' did not return the fixed global retirement message')
            # Static ownership guidance must not expose even a real pane/terminal selector.
            check.require(all(value not in json.dumps(response['error']) for value in
                              (pane, terminal, 'gate-missing-agent', private_selector)),
                          method + ' echoed a private selector')
            check.require(navigation_state() == before, method + ' refusal changed topology/history/focus')

        cli_root = root / 'cli'
        cli_root.mkdir(mode=0o700)
        cli_env = check.environment(cli_root)
        # Listen without serving: even a protocol ping must be observable and must fail this gate.
        with contextlib.ExitStack() as stack:
            canaries = []
            for name in ('m.sock', 'm-client.sock', 'n.sock'):
                listener = stack.enter_context(socket.socket(socket.AF_UNIX))
                listener.bind(str(root / name))
                (root / name).chmod(0o600)
                listener.listen(16)
                canaries.append(listener)
            cli_env.update(KITSUNE_SOCKET_PATH=str(root / 'm.sock'),
                           KITSUNE_CLIENT_SOCKET_PATH=str(root / 'n.sock'),
                           KITSUNE_AGENT_DETECTION_MANIFEST_CATALOG_URL='interop-invalid://MUST_NOT_FETCH/catalog.toml')
            cache_before = tree(cli_root)
            valid_forms = [('agent-manifests', []), ('agent-manifests', ['--json']),
                           ('update-agent-manifests', []), ('update-agent-manifests', ['--json']),
                           ('reload-agent-manifests', [])]
            invalid_forms = [(name, ['--invalid']) for name in
                             ('agent-manifests', 'update-agent-manifests', 'reload-agent-manifests')]
            invalid_forms.append(('reload-agent-manifests', ['--json']))
            for index, (name, flags) in enumerate(valid_forms + invalid_forms):
                result = subprocess.run([args.kitsune_bin, 'server', name, *flags],
                                        env=cli_env, cwd=cli_root, capture_output=True, text=True,
                                        timeout=10, preexec_fn=check.limits)
                check.require(not select.select(canaries, [], [], 0)[0], 'retired CLI contacted an API/client canary')
                check.require(not result.stdout and len(result.stderr) < 16384, 'retired CLI output unbounded/unexpected')
                if index >= len(valid_forms):
                    check.require(result.returncode == 2 and result.stderr.startswith('usage: kitsune server ' + name),
                                  'invalid retired CLI syntax did not remain usage exit 2')
                else:
                    check.require(result.returncode == 1, 'retired CLI did not fail locally')
                    if flags:
                        check.require(json.loads(result.stderr) == dict(id='cli:server:' + name,
                                      error=dict(code='feature_retired', message=MANIFEST_RETIREMENT_MESSAGE)),
                                      'retired CLI JSON error/command ID mismatch')
                    else:
                        check.require(result.stderr == 'feature_retired: ' + MANIFEST_RETIREMENT_MESSAGE + '\n',
                                      'retired CLI text error mismatch')
                check.require(tree(cli_root) == cache_before, 'retired CLI mutated isolated HOME/cache/status/config tree')
            agent_forms = [(['list'], 'agent.list'), (['get', 'PRIVATE-CLI'], 'agent.get'),
                           (['read', 'PRIVATE-CLI'], 'agent.read'), (['explain', 'PRIVATE-CLI', '--json'], 'agent.explain'),
                           (['send-keys', 'PRIVATE-CLI', 'enter'], 'agent.send_keys'),
                           (['rename', 'PRIVATE-CLI', 'gate-name'], 'agent.rename'),
                           (['focus', 'PRIVATE-CLI'], 'agent.focus'),
                           (['start', 'gate-name', '--kind', 'djinn', '--pane', pane], 'agent.start'),
                           (['prompt', 'PRIVATE-CLI', sentinel, '--wait', '--until', 'idle', '--timeout', '0'], 'agent.prompt'),
                           (['wait', 'PRIVATE-CLI'], 'agent.wait'), (['attach', 'PRIVATE-CLI', '--takeover'], 'agent.get')]
            targets = ('pi', 'claude', 'codex', 'copilot', 'opencode', 'djinn')
            for argv, method in [(['agent', *argv], method) for argv, method in agent_forms] + [
                    (['integration', 'install', target], 'integration.install') for target in targets]:
                result = subprocess.run([args.kitsune_bin, *argv], env=cli_env, cwd=cli_root,
                                        capture_output=True, text=True, timeout=10, preexec_fn=check.limits)
                check.require(result.returncode == 1 and not result.stdout and len(result.stderr) < 16384,
                              'retired Agent/install CLI did not fail locally')
                if method == 'integration.install':
                    check.require(result.stderr.startswith('feature_retired: integration.install:'),
                                  'installer CLI lost its human-text retirement error')
                else:
                    check.require(json.loads(result.stderr) == dict(id='cli:agent:retired', error=retirement_error(method)),
                                  'Agent CLI local retirement error mismatch')
                check.require(not select.select(canaries, [], [], 0)[0], 'Agent/install CLI contacted an API/client canary')
                check.require(tree(cli_root) == cache_before, 'Agent/install CLI changed HOME/config/cache tree')
            check.require(not select.select(canaries, [], [], 0)[0], 'late retired CLI socket connection')
        check.require(tree(retired_cache) == retired_before, 'startup/API/CLI changed stale remote cache/status inputs')
        check.require(navigation_state() == before, 'retired CLI changed topology/history/focus')
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
        check.passed(f'Kit read/global-manifest retirement: {len(reads)} exact feature_retired API refusals; '
                     '22 local CLI refusals + 4 usage errors, zero API/client canary connections; '
                     'CLI tree and seeded stale remote cache/status unchanged; '
                     'snapshot agents empty, generic text/detection/process and navigation.v1 retained')

        hooks = [('pane.report_agent', dict(pane_id=pane, source='kitsune:djinn', agent='djinn', state='working', seq=1)),
                 ('pane.report_agent_session', dict(pane_id=pane, source='kitsune:djinn', agent='djinn', seq=1)),
                 ('pane.clear_agent_authority', dict(pane_id=pane, source='kitsune:djinn', seq=1)),
                 ('pane.release_agent', dict(pane_id=pane, source='kitsune:djinn', agent='djinn', seq=1))]
        for method, params in hooks:
            response = check.rpc(kit, method, params)
            check.require(set(response) == {'id', 'error'} and response['error']['code'] == 'external_agent_management',
                          'external pane hook lost its distinct ownership error')
        compound = [('integration.install', dict(target=target)) for target in targets]
        for matcher in (dict(type='substring', value='READ_OK'), dict(type='regex', value='[')):
            generic = dict(type='pane.output_matched', pane_id='PRIVATE-OUTPUT', source='recent', match=matcher)
            retired = dict(type='pane.agent_status_changed', pane_id='PRIVATE-STATUS', agent_status='unknown')
            for subscriptions in ([generic, retired], [retired, generic]):
                compound.append(('events.subscribe', dict(subscriptions=subscriptions)))
        for selector in (pane, 'PRIVATE-STATUS', ''):
            for status in ('unknown', 'working'):
                compound.append(('events.wait', dict(match_event=dict(event='pane_agent_status_changed',
                                pane_id=selector, agent_status=status), timeout_ms=0)))
        for method, params in compound:
            if method.startswith('events.'):
                with socket.socket(socket.AF_UNIX) as stream:
                    stream.settimeout(2)
                    stream.connect(str(kit))
                    stream.sendall((json.dumps(dict(id='retirement-stream', method=method, params=params)) + '\n').encode())
                    data = bytearray()
                    while block := stream.recv(16384):
                        data.extend(block)
                        check.require(len(data) < 16384, 'retired event request initialized an unexpected stream')
                    check.require(len(data.splitlines()) == 1, 'retired event request returned stream data')
                    response = json.loads(data)
                    check.require(response.get('id') == 'retirement-stream', 'retired stream reply was uncorrelated')
            else:
                response = check.rpc(kit, method, params)
            check.require(set(response) == {'id', 'error'} and response['error'] == retirement_error(method),
                          method + ' did not retire atomically before semantic validation/probes')
            check.require(navigation_state() == before, 'retired compound/install request changed topology/history')
        check.passed(f'Kit compound retirement: {len(compound)} exact feature_retired event/install requests, '
                     'closed single-frame event refusals; 4 pane hooks retain external_agent_management')

        def tokens():
            return next(p.get('tokens', {}) for p in check.api(kit, 'session.snapshot')['snapshot']['panes'] if p['pane_id'] == pane)

        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(3)
            stream.connect(str(kit))
            stream.sendall((json.dumps(dict(id='tokens', method='events.subscribe',
                            params=dict(subscriptions=[dict(type='pane.updated')]))) + '\n').encode())
            with stream.makefile('r') as reader:
                check.require(json.loads(reader.readline(16384)) == dict(id='tokens', result=dict(type='subscription_started')),
                              'generic metadata event subscription failed')

                def token_event(key, expected):
                    for _ in range(64):
                        value = json.loads(reader.readline(65536))
                        item = value.get('data', {}).get('pane', {})
                        if item.get('pane_id') == pane and item.get('tokens', {}).get(key) == expected:
                            return
                    raise AssertionError('generic metadata token event not observed')

                base = dict(pane_id=pane, source='gate.tokens')
                check.api(kit, 'pane.report_metadata', {**base, 'seq': 1, 'ttl_ms': 60000, 'tokens': {'gate': 'kept'}})
                token_event('gate', 'kept')
                for field in [dict(agent='djinn'), dict(title='retired'), dict(display_agent='retired'),
                              dict(applies_to_source='kitsune:djinn'), dict(state_labels={'working': 'retired'}),
                              dict(clear_title=True), dict(clear_display_agent=True), dict(clear_state_labels=True)]:
                    response = check.rpc(kit, 'pane.report_metadata', {**base, **field, 'seq': 100,
                                         'tokens': {'gate': 'MUST_NOT_PATCH'}})
                    check.require(response.get('error') == retirement_error('pane.report_metadata') and tokens()['gate'] == 'kept',
                                  'retired presentation field patched tokens or lost its exact error')
                check.api(kit, 'pane.report_metadata', {**base, 'seq': 1, 'tokens': {'gate': 'stale'}})
                check.require(tokens()['gate'] == 'kept', 'duplicate metadata sequence changed token')
                check.api(kit, 'pane.report_metadata', {**base, 'seq': 2, 'ttl_ms': 400, 'tokens': {'short': 'expires'}})
                check.require(tokens().get('short') == 'expires', 'new metadata sequence was consumed by a refused mixed frame')
                check.wait_for(lambda: 'short' not in tokens() and tokens().get('gate') == 'kept', 'generic token TTL expiry')
                token_event('short', None)
                check.api(kit, 'pane.report_metadata', {**base, 'seq': 3, 'tokens': {'gate': 'fresh'}})
                token_event('gate', 'fresh')
                check.api(kit, 'pane.report_metadata', {**base, 'seq': 4, 'tokens': {'gate': None}})
                check.require('gate' not in tokens(), 'generic token clear failed')
        matched = check.api(kit, 'pane.wait_for_output', dict(pane_id=pane, source='recent',
                            match=dict(type='substring', value='READ_OK'), timeout_ms=1000))
        check.require(matched['type'] == 'output_matched' and matched['pane_id'] == pane, 'generic output wait failed')
        unsupported = check.rpc(kit, 'events.wait', dict(match_event=dict(event='workspace_created'), timeout_ms=0))
        check.require(unsupported.get('error', {}).get('code') == 'unsupported_event_wait_match',
                      'unsupported generic events.wait was incorrectly claimed as supported')
        check.require(navigation_state() == before, 'generic metadata/waits changed topology/history')
        check.passed('Kit generic metadata: 8 mixed-presentation refusals atomic; token sequence/TTL/clear and pane.updated '
                     'events work; pane.wait_for_output succeeds; unsupported generic events.wait stays explicit')

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
        view.expect_row('Djinn', 'UNKNOWN', 'caller unverified')

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

        check.api(kit, 'pane.rename', dict(pane_id=pane, label='Sample A'))
        started = operation('sampler-start')
        check.require(started['samplingInstance'] != ready['instance'], 'new sampler reused disposed reporter identity')
        ids = {name: hashlib.sha256(('native\0' + started['samplingInstance'] + '\0' + ready[name]).encode()).hexdigest()
               for name in ('a', 'b')}

        def sampled(name, status):
            return check.wait_for(lambda: (r if (r := row(name)) and r['origin'] == 'djinn_native_event' and
                                          r['reported_state'] == status and r['freshness'] == 'fresh' else None),
                                  'fresh bound native service ' + name + ' ' + status)

        sampled('a', 'idle')
        view.expect_row('Djinn', 'IDLE (fresh)', name='Sample A')
        first_query = operation('sample')
        # Timer admission is throttled to at most once per second; scheduling
        # near that boundary can skip a tick. Observe two actual fresh queries.
        time.sleep(4.2)
        second_query = operation('sample')
        check.require(second_query['queries'] >= first_query['queries'] + 2 and sampled('a', 'idle')['age_ms'] < 2000,
                      'mounted quiet root did not obtain periodic fresh native service evidence')
        check.passed('bound actual native Session/SessionStatus services establish quiet root Idle initially and periodically; not inferred from hosting/liveness')
        operation('sampler-busy')
        sampled('a', 'working')
        view.expect_row('Djinn', 'WORKING (fresh)', name='Sample A')
        operation('sampler-idle')
        sampled('a', 'idle')
        view.expect_row('Djinn', 'IDLE (fresh)', name='Sample A')
        check.passed('actual native status service busy -> idle, with no plugin event callbacks, is recovered by fresh selected-root sampling and rendered icon/name/details')
        operation('sampler-fail')
        check.wait_for(lambda: (r if (r := row('a')) and r['reported_state'] == 'unknown' and
                               r['freshness'] == 'expired' else None), 'unavailable-query evidence expires', 35)
        check.require(row('a')['hosting_state'] == 'local_full_tui_root', 'failed query dropped hosting instead of expiring activity')
        view.expect_row('Djinn', 'UNKNOWN (expired)', name='Sample A')
        operation('sampler-recover')
        sampled('a', 'idle')
        view.expect_row('Djinn', 'IDLE (fresh)', name='Sample A')
        check.passed('explicit unavailable-query seam expires activity to Unknown despite live hosting; successful fresh local service query restores Idle')
        operation('sampler-b')
        sampled('b', 'idle')
        check.api(kit, 'pane.rename', dict(pane_id=pane, label='Sample B'))
        view.expect_row('Djinn', 'IDLE (fresh)', name='Sample B')
        check.require(row('a')['hosting_state'] == 'none' and row('b')['hosting_state'] == 'local_full_tui_root',
                      'sampling new selected root retained old association')
        check.passed('second actual native root receives its own fresh Idle evidence and current pane name; previous root loses hosting/name rather than sharing ownership')
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
    check.require(tree(retired_cache) == retired_before, 'hosting/shutdown changed stale remote cache/status inputs')
    check.stop_children()
    check.require(not check.descendants(), 'owned descendants remain')
    print('SHUTDOWN verified: actual server.stop, helper reaped, same receipt generation released, no owned descendants', flush=True)

    phase = root / 'solo'
    phase.mkdir(mode=0o700)
    solo_env = check.environment(phase)
    solo_config = phase / 'kit.toml'
    solo_config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
                           '[integrations]\nsources = []\n')
    solo_socket = phase / 'k.sock'
    solo_env.update(KITSUNE_CONFIG_PATH=str(solo_config), KITSUNE_SOCKET_PATH=str(solo_socket),
                    KITSUNE_SESSION='standalone-retirement', KITSUNE_LOG='warn')
    solo = check.spawn([args.kitsune_bin, 'server'], solo_env, root, 'solo-kit', stdin=subprocess.DEVNULL)
    try:
        check.wait_for(lambda: check.api(solo_socket, 'session.snapshot'), 'owned standalone Kit startup')
        solo_pane, solo_pane_env = check.pane_environment(solo_socket, phase, workspace)
        check.require(all(key not in solo_pane_env for key in ('KITSUNE_INTEGRATION_SOURCE',
                      'KITSUNE_INTEGRATION_REPORT_COMMAND', 'KITSUNE_AGENT_MANAGEMENT')), 'standalone inherited an external route')
        snapshot = check.api(solo_socket, 'session.snapshot')['snapshot']
        check.require(snapshot['agents'] == [] and snapshot['integration_views']['sources'] == [],
                      'standalone restored a built-in agent manager or external integration')
        cases = [('agent.list', {}), ('agent.get', dict(target=solo_pane)),
                 ('agent.start', dict(name='inert', kind='djinn', pane_id=solo_pane)),
                 ('agent.prompt', dict(target=solo_pane, text=sentinel, wait=dict(until=['idle'], timeout_ms=0))),
                 ('agent.read', dict(target=solo_pane, source='detection')),
                 ('server.agent_manifests', {}), ('server.reload_agent_manifests', {}),
                 ('integration.install', dict(target='djinn')),
                 ('events.wait', dict(match_event=dict(event='pane_agent_status_changed', pane_id=solo_pane, agent_status='unknown'))),
                 ('events.subscribe', dict(subscriptions=[dict(type='workspace.created'),
                  dict(type='pane.agent_status_changed', pane_id=solo_pane)]))]
        cases.extend((method, {**params, 'pane_id': solo_pane}) for method, params in hooks)
        for method, params in cases:
            response = check.rpc(solo_socket, method, params)
            expected = dict(code='feature_retired', message=MANIFEST_RETIREMENT_MESSAGE) if method.startswith('server.') else retirement_error(method)
            # Pane ingress guidance is handler-specific; its code must still retire globally.
            check.require(set(response) == {'id', 'error'} and response['error'].get('code') == 'feature_retired' and
                          (method.startswith('pane.') or response['error'] == expected), 'standalone legacy operation was not retired')
        marker = phase / 'quiet'
        check.api(solo_socket, 'pane.send_input', dict(pane_id=solo_pane,
                  text='printf "SOLO_OK\\n"; : > ' + shlex.quote(str(marker)), keys=['enter']))
        check.wait_for(marker.exists, 'standalone generic shell input')
        read = check.api(solo_socket, 'pane.read', dict(pane_id=solo_pane, source='recent_unwrapped', format='text'))['read']
        process = check.api(solo_socket, 'pane.process_info', dict(pane_id=solo_pane))['process_info']
        terminal = next(p['terminal_id'] for p in snapshot['panes'] if p['pane_id'] == solo_pane)
        info = check.api(solo_socket, 'terminal.inspect', dict(terminal_id=terminal))
        focused = check.api(solo_socket, 'terminal.focus', dict(terminal_id=terminal,
                            expected_runtime_generation=info['runtime_generation']))
        check.require('SOLO_OK' in read['text'] and sentinel not in read['text'] and
                      process.get('shell_pid') in check.descendants() and focused['type'] == 'terminal_focused',
                      'standalone generic shell/read/process/generation focus failed')
        check.require(check.api(solo_socket, 'session.snapshot')['snapshot']['agents'] == [] and
                      not (phase / 'state/coven').exists(), 'standalone retirement acquired hidden Coven state')
        check.passed(f'Kit without Coven: {len(cases)} exact feature_retired refusals including four pane hooks; '
                     'no external route/state, empty agent inventory; generic shell/read/process/exact-generation focus works')
    finally:
        if solo.poll() is None:
            check.api(solo_socket, 'server.stop')
        check.require(solo.wait(timeout=10) == 0, 'standalone Kit shutdown failed')
    check.stop_children()
    print(f'SUMMARY {check.PASS_COUNT} A2 composed scenarios passed', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('coven-bin', 'kitsune-bin', 'bun'):
        parser.add_argument('--' + name, required=True, type=check.absolute)
    parser.add_argument('--djinn-source', required=True, type=Path)
    args = parser.parse_args()
    try:
        with check.owned_run(short=True) as root:
            run(args, root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
