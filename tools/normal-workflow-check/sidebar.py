#!/usr/bin/env python3
"""Actual headless sidebar qualification; synthetic native facts, no provider apps."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import check
from bootstrap import helper_for, instance


def ascii_frame(rows):
    return '\n'.join(line.rstrip() for line in rows).translate(str.maketrans({
        '▾': 'v', '▸': '>', '«': '<<', '»': '>>', '…': '...', '·': '.',
    })).encode('ascii', errors='replace').decode('ascii')


def run(args, root):
    env = check.environment(root)
    env['XDG_STATE_HOME'] = str(root / 'v')
    state = root / 'v/coven/integration'
    check.require(len(os.fsencode(state / 'i' / ('0' * 24) / 'r.sock')) < 108, 'receiver path too long')
    bindir = root / 'bin'
    bindir.mkdir(mode=0o700)
    (bindir / 'coven').symlink_to(args.coven_bin)
    env['PATH'] = str(bindir) + ':/usr/bin:/bin'
    config = root / 'kit.toml'
    config.write_text('onboarding = false\n[terminal]\ndefault_shell = "/bin/sh"\nshell_mode = "non_login"\n'
                      '[ui]\nsidebar_width = 100\nsidebar_min_width = 100\nsidebar_max_width = 100\n'
                      '[integrations]\nsources = ["coven", "build"]\n')
    kit = root / 'k.sock'
    env.update(KITSUNE_CONFIG_PATH=str(config), KITSUNE_SOCKET_PATH=str(kit),
               KITSUNE_SESSION='sidebar', KITSUNE_LOG='warn')
    workspace = root / 'w'
    check.command([args.coven_bin, 'init', str(workspace), '--blank'], env, root)
    server = view = None
    owner = None

    def snapshot():
        return check.api(kit, 'session.snapshot')['snapshot']

    def navigation():
        s = snapshot()
        return {key: s.get(key) for key in ('focused_workspace_id', 'focused_tab_id', 'focused_pane_id')} | {
            'workspaces': [(w['workspace_id'], w['label']) for w in s['workspaces']],
            'history': [(p['pane_id'], p.get('agent_session')) for p in s['panes']]}

    def poll():
        return check.api(kit, 'integration.view.poll', owner)['intents']

    def settle(predicate=lambda text: True):
        generation = view.generation
        return check.wait_for(lambda: (text if view.generation > generation and predicate(text) else None)
                              if (text := view.poll()) else None, 'latest semantic frame')

    def lines():
        view.poll()
        return [line[:99] for line in view.text.splitlines()]

    def header(name):
        rows = lines()
        matches = [y for y, line in enumerate(rows) if line.strip().startswith(('▾ ' + name, '▸ ' + name))]
        check.require(len(matches) == 1, f'header {name} not unique: {rows}')
        return matches[0]

    def click_header(name, collapsed):
        y = header(name)
        view.mouse(2, y)
        settle(lambda text: ('▸ ' if collapsed else '▾ ') + name in text)

    def shutdown(directory, receipt, helper):
        nonlocal server, view
        if view:
            view.close()
            view = None
        check.api(kit, 'server.stop')
        check.require(server.wait(timeout=10) == 0, 'server.stop failed')
        released = json.loads((directory / 'receipt.json').read_text())
        check.require(released['released'] and released['nonce'] == receipt['nonce'] and
                      not (directory / 'r.sock').exists() and not Path(f'/proc/{helper}').exists(),
                      'helper/receiver/receipt not cleanly released')
        check.stop_children()

    try:
        server = check.spawn([args.kitsune_bin, 'server'], env, root, 'kit', stdin=subprocess.DEVNULL)
        check.wait_for(snapshot, 'Kit startup')
        directory, receipt = check.wait_for(lambda: instance(state, kit), 'actual default Coven receipt')
        helper = check.wait_for(lambda: helper_for(args, server), 'actual Coven child')
        pane, pane_env = check.pane_environment(kit, root, workspace)
        for i, name in enumerate(('Atlas', 'Birch')):
            path = root / name
            path.mkdir()
            check.api(kit, 'workspace.create', dict(cwd=str(path), label=name, focus=i == 0))
        view = check.ViewClient(root / 'k-client.sock', 200, 36)
        view.expect('Spaces', 'Coven', 'build', 'Atlas', 'Birch')
        sequence = 0

        def report(i, event, **fields):
            nonlocal sequence
            sequence += 1
            payload = dict(contract='djinn.native-events.v1', instance_id='11111111-1111-4111-8111-111111111111',
                           session_id=f'PRIVATE-ROOT-{i}', sequence=sequence, event=event, **fields)
            check.command([args.coven_bin, 'agent', 'report-native', '--from-kitsune', '--json'],
                          pane_env, root, json.dumps(payload))

        def observations():
            return json.loads(check.command([args.coven_bin, 'agent', 'observations', '--from-kitsune', '--json'],
                                             pane_env, root))['observations']

        def coven_rows():
            return [line.strip() for line in lines()[header('Coven') + 1:header('build')] if line.strip()]

        def coven_state(state, glyph=None):
            rows = coven_rows()
            return rows and all(re.fullmatch(r'. Djinn', row)
                                and (glyph is None or row.startswith(glyph + ' ')) for row in rows)

        for i in range(16):
            for event, fields in [('session.created', {}), ('session.status', {'status': 'busy'})]:
                report(i, event, **fields)
        view.expect('Djinn')
        shared_id = 'observation:' + observations()[0]['observation_id']
        opened = check.api(kit, 'integration.view.open', dict(source='build', version=1))
        owner = dict(source='build', lease=opened['lease'], epoch=opened['epoch'])
        items = [dict(id=shared_id if i == 0 else f'build-{i}', label=f'Build task {i}', detail=f'Build detail {i}')
                 for i in range(12)]
        check.api(kit, 'integration.view.replace', {**owner, 'revision': 1, 'items': items})
        view.expect('Build task 0')
        settle(lambda text: coven_state('Working'))
        print('ACTUAL WORKING ROW: ' + coven_rows()[0], flush=True)
        check.require('Build detail 0' not in view.text and 'last-reported' not in view.text,
                      'one-line sidebar exposed inspector details')
        check.require('supported_versions' in snapshot()['integration_views'], 'new Kit descriptor missing versions')
        check.passed('actual new Kit accepts v1 plain build rows beside negotiated Coven status rows; one line, no inline detail')

        # No report/replace/poll inside this sampling window. Actual Coven may keep
        # its transport lease alive; that must not renew native evidence.
        evidence = observations()
        phases = set()
        end = time.monotonic() + 1.2
        while time.monotonic() < end:
            view.poll()
            phases.add(next(line.strip()[0] for line in lines() if 'Djinn' in line))
        later = observations()
        check.require(len(phases) > 1 and
                      [{k: v for k, v in row.items() if k != 'age_ms'} for row in later] ==
                      [{k: v for k, v in row.items() if k != 'age_ms'} for row in evidence] and
                      all(b['age_ms'] > a['age_ms'] for a, b in zip(evidence, later)),
                      f'working spinner did not animate locally or renewed evidence: {phases}')
        check.passed('actual Coven Working spinner advances without new native reports or changed observation evidence')

        check.require(poll() == [], 'animation queued a fixture intent')
        for event, fields, glyph, state_word in [('session.status', {'status': 'idle'}, '·', 'Idle'),
                                                 ('permission.asked', {'request_id': 'PRIVATE-REQUEST'}, '!', 'Blocked'),
                                                 ('session.deleted', {}, '·', 'Ended')]:
            for i in range(16):
                report(i, event, **fields)
            settle(lambda text: coven_state(state_word, glyph))
            view.expect_row('Djinn', state_word.upper() + ' (fresh)')
            print('ACTUAL STATE ROW: ' + coven_rows()[0], flush=True)
            check.require(not any(mark in ''.join(coven_rows()) for mark in ('✓', '×')),
                          'native idle/blocked/ended fabricated a job outcome')
            # Keep only the manual peer alive across CLI/report/render work.
            check.require(poll() == [], 'native status rendering queued a fixture intent')
        check.passed('actual Coven shows icon + Djinn only: no state words, IDs or suffixes; Idle/Ended neutral, Blocked attention; no job outcome inferred')
        before = navigation()
        epoch = next(s['epoch'] for s in snapshot()['integration_views']['sources'] if s['id'] == 'coven')
        rows = lines()
        boundaries = [header(name) for name in ('Spaces', 'Coven', 'build')]
        toggle = next(y for y, line in enumerate(rows) if '«' in line)
        heights = [b - a - 1 for a, b in zip(boundaries, boundaries[1:] + [toggle])]
        check.require(max(heights) - min(heights) <= 1 and min(heights) > 0, f'unequal peer bodies {heights}')
        check.require(not any(word in '\n'.join(rows) for word in ('Integrations', 'coven: Available', 'PRIVATE-', 'Unavailable')),
                      'old selector/status wrapper or private ID in healthy sidebar')
        print('ACTUAL SIDEBAR (synthetic fixture, ASCII transcription):\n' + ascii_frame(rows), flush=True)
        check.passed(f'simultaneous named Spaces/Coven/build; fair body heights {heights}; own menu/toggle chrome; private IDs redacted')

        click_header('Coven', True)
        check.require('Djinn' not in '\n'.join(lines()) and header('build') < boundaries[2], 'collapse did not free body')
        check.require(navigation() == before and poll() == [], 'header activated item or changed native navigation')
        check.require(next(s['epoch'] for s in snapshot()['integration_views']['sources'] if s['id'] == 'coven') == epoch,
                      'collapse changed Coven lease')
        click_header('Coven', False)
        click_header('Spaces', True)
        check.require('Atlas' not in '\n'.join(lines()) and navigation() == before, 'Spaces collapse changed native state')
        click_header('Spaces', False)
        check.passed('mouse header collapse/re-expand isolates bodies, redistributes space, preserves navigation and Coven lease; no build intent')

        for column in (5, 40):
            view.expect('Build task 0')
            y = next(y for y, line in enumerate(lines()) if 'Build task 0' in line)
            view.mouse(column, y)
            intents = check.wait_for(poll, 'source-routed build selection')
            check.require(len(intents) == 1 and intents[0]['item_id'] == shared_id, f'wrong build intent {intents}')
        check.require(navigation() == before, 'build selection changed native focus')
        view.poll()
        check.require('Selected' not in '\n'.join(lines()[header('Coven') + 1:header('build')]),
                      'build selection reached actual Coven publisher')
        check.passed('actual mouse label/row blank both enqueue exact v1 build intent with Coven-colliding ID, no native focus change')

        coven_body = lines()[header('Coven') + 1:header('build')]
        view.mouse(5, header('build') + 1, 'down')
        settle(lambda text: 'Build task 0' not in text)
        check.require(lines()[header('Coven') + 1:header('build')] == coven_body, 'build wheel scrolled Coven')
        build_body = lines()[header('build') + 1:toggle]
        check.command([args.coven_bin, 'agent', 'report', '--from-kitsune', '--provider', 'codex', '--json'],
                      pane_env, root, json.dumps({'session_id': 'PRIVATE-REFERENCE', 'hook_event_name': 'SessionStart'}))
        for _ in range(18):
            view.mouse(5, header('Coven') + 1, 'down')
            settle()
            if '? Codex' in coven_rows():
                break
        check.require(lines()[header('build') + 1:toggle] == build_body, 'Coven wheel scrolled build')
        scrolled = lines()[header('Coven') + 1:header('build')]
        check.require(scrolled != coven_body, 'Coven wheel did not change its body')
        click_header('Coven', True)
        click_header('Coven', False)
        check.require(lines()[header('Coven') + 1:header('build')] == scrolled, 'Coven collapse lost source scroll')
        check.require(poll() == [], 'wheel/header queued activation')
        check.passed('wheel is source-local for both peers; Coven source scroll survives collapse/re-expand')

        check.api(kit, 'integration.view.replace', {**owner, 'revision': 2, 'items': []})
        settle(lambda text: 'Build task' not in text)
        empty = lines()[header('build') + 1:toggle]
        check.require(empty[0].strip() == 'No items' and all(not line.strip() for line in empty[1:]),
                      'empty status escaped its otherwise blank body')
        view.mouse(5, header('build') + 2)
        click_header('build', True)
        click_header('build', False)
        check.require(poll() == [], 'empty/header click activated item')
        check.passed('live empty peer has body-only No items, remaining cells blank; blank/header clicks do not activate items')

        for height in (9, 12, 36):
            poll()  # Explicitly renew only this fixture's lease, never Coven's.
            view.resize(200, height)
            settle(lambda text: all(name in text for name in ('Spaces', 'Coven', 'build')))
            check.require(len(view.text.splitlines()) == height, 'resize produced invalid semantic dimensions')
        check.require(poll() == [] and navigation() == before, 'resize queued intent or changed navigation')
        check.passed('actual client resizes at heights 9/12/36 preserve valid peer frames and native navigation')

        # Derive the toggle and compact launcher from the latest rendered cells.
        rows = lines()
        y = next(y for y, line in enumerate(rows) if '«' in line)
        view.mouse(rows[y].index('«'), y)
        settle(lambda text: '»' in text)
        compact = view.text.splitlines()
        separator = compact[1].index('│')
        compact_sidebar = [line[:separator] for line in compact]
        print('ACTUAL COMPACT SIDEBAR (ASCII transcription):\n' + ascii_frame(compact_sidebar), flush=True)
        expected_headers = ['▾Sp', '▾Co', '▾bu']
        actual_headers = [line for line in compact_sidebar if line.startswith(('▾', '▸'))]
        compact_named = actual_headers == expected_headers
        check.require(compact_named, f'compact section identities must be exactly {expected_headers}, got {actual_headers}')
        check.require(any('·' in row for row in compact_sidebar[compact_sidebar.index('▾Co') + 1:compact_sidebar.index('▾bu')]),
                      'compact Coven body lost neutral native status')
        view.mouse(1, compact_sidebar.index('▾Co'))
        settle(lambda text: [line[:separator] for line in text.splitlines() if line.startswith(('▾', '▸'))]
               == ['▾Sp', '▸Co', '▾bu'])
        collapsed = [line[:separator] for line in view.text.splitlines()]
        check.require(collapsed.index('▾bu') == collapsed.index('▸Co') + 1,
                      'compact Coven collapse did not free only its body')
        check.require(poll() == [] and navigation() == before and
                      next(s['epoch'] for s in snapshot()['integration_views']['sources'] if s['id'] == 'coven') == epoch,
                      'compact header click activated item, changed navigation, or changed Coven lease')
        view.mouse(1, collapsed.index('▸Co'))
        settle(lambda text: [line[:separator] for line in text.splitlines() if line.startswith(('▾', '▸'))]
               == expected_headers)
        check.passed('exact distinct compact Sp/Co/bu headers; Coven-only collapse/re-expand preserves source identity, native navigation and lease')
        space_start, space_end = boundaries[0] + 1, boundaries[1]
        for number in range(1, 4):
            check.require(any(line.strip().lstrip('>') == str(number)
                              for line in compact_sidebar[space_start:space_end]), 'compact workspace number missing')
        menu_y = next(y for y, line in enumerate(compact_sidebar) if line.strip() == 'm')
        view.mouse(compact_sidebar[menu_y].index('m'), menu_y)
        settle(lambda text: 'settings' in text.lower() and 'reload' in text.lower())
        view.key('\x1b')
        settle(lambda text: 'reload' not in text.lower())
        rows = view.text.splitlines()
        y = next(y for y, line in enumerate(rows) if '»' in line[:separator])
        view.mouse(rows[y].index('»'), y)
        settle(lambda text: '▾ Spaces' in text and '▾ Coven' in text and '▾ build' in text)
        check.require(poll() == [] and navigation() == before, 'compact chrome changed native navigation or activated item')
        check.passed('compact workspace numbers remain visible; actual m launcher opens/closes global menu and bottom toggle restores sidebar')

        view.press('b', 2)
        view.press('a')
        view.expect('Integration details', 'Status: neutral', 'caller unverified', 'success claim')
        view.press('/')
        for ch in 'Codex':
            view.press(ch)
        view.expect('/ Codex')
        view.press('enter')  # Finish search, not activation.
        view.expect('Filter: Codex', '? Codex')
        y = next(y for y, line in enumerate(view.text.splitlines()) if '? Codex' in line[51:149])
        check.require(view.text.splitlines()[y][51:149].strip(' │>') == '? Codex',
                      'Unknown inspector row exposed a state word or opaque ID')
        view.mouse(55, y)
        view.expect('Codex', 'Status: unknown', 'reference_only')
        check.require('✓' not in view.text and '×' not in view.text and poll() == [] and navigation() == before,
                      'unknown reference manufactured an outcome or inspector navigation activated a row')
        print('ACTUAL COVEN INSPECTOR (ASCII transcription):\n' + ascii_frame(view.text.splitlines()), flush=True)
        view.press('esc')
        settle(lambda text: 'Integration details' not in text)
        check.passed('actual prefix+a inspector exposes Coven provenance/detail; search finds untrusted Codex UNKNOWN/reference_only, no inferred job outcome or navigation change')

        # Replace only the fixture peer, never the actual Coven lease.
        check.api(kit, 'integration.view.close', owner)
        opened = check.api(kit, 'integration.view.open', dict(source='build', version=2))
        owner = dict(source='build', lease=opened['lease'], epoch=opened['epoch'])
        items = [dict(id=f'manual-{i}', label=f'Manual row {i}', detail=('detail-start ' + 'bounded detail ' * 28 + ' detail-end'),
                      presentation=dict(label=f'Fixture {i}', status=status))
                 for i, status in enumerate(('running', 'error', 'success'))]
        check.api(kit, 'integration.view.replace', {**owner, 'revision': 1, 'items': items})
        view.expect('Fixture 0', '×', '✓')
        phases = set()
        end = time.monotonic() + 1.1
        while time.monotonic() < end:
            view.poll()
            phases.add(next(line.strip()[0] for line in lines() if 'Fixture 0' in line))
        result = check.api(kit, 'integration.view.poll', owner)
        check.require(len(phases) > 1 and result['revision'] == 1 and result['intents'] == [],
                      'host-local fixture spinner required owner renewal/revision or queued intent')
        check.passed('v2 manual fixture explicitly declares running/error/success; spinner advances with no intervening owner API/revision, not native lifecycle proof')

        view.press('b', 2)
        view.press('a')
        view.expect('Integration details')
        y = next(y for y, line in enumerate(view.text.splitlines()) if line[51:149].strip(' │') == 'build')
        view.mouse(55, y)
        view.expect('detail-start', 'Fixture 0')
        check.require(poll() == [], 'source click activated fixture item')
        view.press('tab')
        view.expect('Tab: Items')
        view.press('down')
        view.expect('Fixture 1: error')
        view.press('/')
        for ch in 'Manual row 2':
            view.press(ch)
        view.expect('/ Manual row 2')
        view.press('enter')
        view.expect('Filter: Manual row 2')
        view.press('down')
        view.expect('Fixture 2: success')
        check.require(poll() == [], 'item navigation/search activated fixture item')
        view.resize(200, 16)
        settle(lambda text: 'Integration details' in text)
        view.press('tab')
        view.expect('Tab: Details')
        view.press('pagedown')
        settle(lambda text: 'detail-end' in text and 'Fixture 2: success' not in text)
        check.require(poll() == [] and navigation() == before, 'search/scroll/source/item navigation queued intent')
        view.press('enter')
        check.require(poll() == [], 'Enter in Details activated item')
        view.press('backtab')
        view.expect('Tab: Items')
        view.press('enter')
        intents = check.wait_for(poll, 'inspector Enter fixture intent')
        check.require(len(intents) == 1 and intents[0] == dict(epoch=owner['epoch'], revision=1, item_id='manual-2'),
                      f'inspector Enter routed wrong intent: {intents}')
        check.passed('actual inspector source/item navigation, search, resize and detail scrolling are read-only; Enter alone queues exact source/epoch/revision/item')

        # Leave the inspector open while the fixture owner stops renewing.
        check.wait_for(lambda: next(s for s in snapshot()['integration_views']['sources'] if s['id'] == 'build')['epoch'] is None,
                       'fixture source lease expiry', 8)
        view.expect('build (unavailable)', 'Select an item')
        view.press('enter')
        check.require('error' in check.rpc(kit, 'integration.view.poll', owner), 'expired fixture lease still polls')
        view.press('esc')
        view.resize(200, 36)
        settle(lambda text: 'Integration details' not in text and 'Unavailable' in text)
        check.require(navigation() == before, 'expiry changed native navigation')
        check.passed('actual source expiry clears inspector data; stale Enter cannot revive expired owner; native navigation unchanged')

        # Persist visual preferences only, then cleanly restart the same owned session.
        click_header('Spaces', True)
        click_header('Coven', True)
        shutdown(directory, receipt, helper)
        server = check.spawn([args.kitsune_bin, 'server'], env, root, 'restart-kit', stdin=subprocess.DEVNULL)
        check.wait_for(snapshot, 'restarted Kit')
        directory2, receipt2 = check.wait_for(lambda: instance(state, kit), 'restarted actual Coven receipt')
        helper2 = check.wait_for(lambda: helper_for(args, server), 'restarted actual helper')
        view = check.ViewClient(root / 'k-client.sock', 200, 36)
        view.expect('▸ Spaces', '▸ Coven', '▾ build', 'Unavailable')
        check.require(directory2 == directory and receipt2['nonce'] != receipt['nonce'], 'receipt generation not safely reused')
        after = navigation()
        check.require(after == before, f'restart changed navigation/history: {before} -> {after}')
        click_header('Spaces', False)
        click_header('Coven', False)
        view.expect('Atlas', 'Birch')
        check.require('Djinn' not in '\n'.join(lines()), 'volatile native facts restored after restart')
        check.passed('clean server restart preserves section preferences/workspace/focus/history; fresh helper generation, no volatile native facts/build lease')

        # Synthetic producer attachments use the real receiver's closed protocol
        # and actual Kit endpoint/generation. They do not prove a native UI mount.
        for i, status in ((0, 'busy'), (1, 'idle')):
            report(i, 'session.status', status=status)
        report(0, 'permission.asked', request_id='PRIVATE-NAMING-REQUEST')

        def naming_rows():
            # The intentionally durable Codex reference survives restart; it is
            # not one of these two volatile native roots.
            return [row for row in coven_rows() if row != '? Codex']

        settle(lambda text: len(naming_rows()) == 2 and all(re.fullmatch(r'. Djinn', row) for row in naming_rows()))
        view.expect_row('Djinn', 'IDLE (fresh)')  # Select among duplicate names by rendered detail.
        current = snapshot()
        target_pane = next(p for p in current['panes'] if p['pane_id'] == pane)
        foreign_pane = next(p for p in current['panes'] if p['pane_id'] != pane)
        check.api(kit, 'pane.rename', dict(pane_id=foreign_pane['pane_id'], label='Foreign only'))
        check.api(kit, 'pane.rename', dict(pane_id=pane, label=None))
        inspected = check.api(kit, 'terminal.inspect', dict(terminal_id=target_pane['terminal_id']))
        metadata = kit.stat()
        target = dict(socket=str(kit.resolve()), device=metadata.st_dev, inode=metadata.st_ino,
                      terminal_id=target_pane['terminal_id'], runtime_generation=inspected['runtime_generation'])
        native_view = json.loads(check.command([args.coven_bin, 'agent', 'observations', '--from-kitsune', '--json'], pane_env, root))
        epoch2 = native_view['server_epoch']
        ids = {row['observation_id'] for row in native_view['observations'] if row['provider'] == 'djinn'}
        evidence = {row['observation_id']: row for row in native_view['observations'] if row['provider'] == 'djinn'}
        expected_ids = {hashlib.sha256(('native\0' + '11111111-1111-4111-8111-111111111111' + '\0PRIVATE-ROOT-' + str(i)).encode()).hexdigest()
                        for i in range(2)}
        check.require(ids == expected_ids and all(re.fullmatch(r'. Djinn', row) for row in naming_rows()),
                      'duplicate default names changed opaque identities or borrowed foreign pane label')
        check.passed('two native roots render exact duplicate Djinn names, no IDs/status words/suffixes; inspector distinguishes idle detail; no foreign pane name borrowed')

        revision = 0

        def attach(binding):
            nonlocal revision
            revision += 1
            request = dict(id='a2' * 16, contract='coven.client-attachments.v1', method='report', params=dict(
                expected_epoch=epoch2, client_id='22222222-2222-4222-8222-222222222222', revision=revision,
                operation='attach', instance_id='11111111-1111-4111-8111-111111111111',
                session_id='PRIVATE-ROOT-0', scope='local_full_tui_root', target=binding))
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(2)
                sock.connect(str(directory2 / 'r.sock'))
                sock.sendall((json.dumps(request) + '\n').encode())
                reply = json.loads(sock.makefile('r').readline(65536))
            check.require(reply.get('type') == 'result', f'attachment fixture refused: {reply}')

        def named(name):
            rows = naming_rows()
            return len(rows) == 2 and rows.count('· Djinn') == 1 and '! ' + name in rows

        before_names = navigation()
        pane_text = check.api(kit, 'pane.read', dict(pane_id=pane, source='recent_unwrapped', format='text'))['read']['text']
        check.api(kit, 'pane.rename', dict(pane_id=pane, label='API work'))
        attach(target)
        settle(lambda text: named('API work'))
        print('ACTUAL NAMED ROOTS: ' + json.dumps(naming_rows(), ensure_ascii=False), flush=True)
        check.api(kit, 'pane.rename', dict(pane_id=pane, label='Review API'))
        settle(lambda text: named('Review API'))
        check.api(kit, 'pane.rename', dict(pane_id=pane, label=''))
        settle(lambda text: len(naming_rows()) == 2 and all(re.fullmatch(r'. Djinn', row) for row in naming_rows()))
        check.passed('verified association names only its own root API work; explicit rename updates Review API and clear restores Djinn without native events')

        check.api(kit, 'pane.rename', dict(pane_id=pane, label='API work'))
        attach(target)
        settle(lambda text: named('API work'))
        attach({**target, 'runtime_generation': '00000000-0000-4000-8000-000000000000'})
        settle(lambda text: len(naming_rows()) == 2 and all(re.fullmatch(r'. Djinn', row) for row in naming_rows()))
        attach(target)
        settle(lambda text: named('API work'))
        check.wait_for(lambda: len(naming_rows()) == 2 and all(re.fullmatch(r'. Djinn', row) for row in naming_rows()),
                       'producer association expiry restores default names', 8)
        after_names = {row['observation_id']: row for row in observations() if row['provider'] == 'djinn'}
        check.require(set(after_names) == ids and navigation() == before_names and
                      any(row['unacknowledged_pending_count'] == 1 for row in evidence.values()) and
                      check.api(kit, 'pane.read', dict(pane_id=pane, source='recent_unwrapped', format='text'))['read']['text'] == pane_text and
                      all(after_names[key]['reported_state'] == evidence[key]['reported_state'] and
                          after_names[key]['age_ms'] > evidence[key]['age_ms'] and
                          after_names[key]['unacknowledged_pending_count'] == evidence[key]['unacknowledged_pending_count']
                          for key in ids), 'naming changed identity, activity age, input/focus or acknowledgement')
        check.passed('wrong generation and expired attachment fail closed to Djinn; names never renew facts, change IDs, focus/input or acknowledge attention')
        shutdown(directory2, receipt2, helper2)
        print(f'SUMMARY {check.PASS_COUNT} sidebar gates passed; compact naming {"passed" if compact_named else "FAILED"}', flush=True)
        check.require(compact_named, 'compact section names are not distinguishable; report to Kit coordinator before production changes')
    finally:
        if view:
            print('LAST FRAME:\n' + view.text, flush=True)
            view.close()
        if server and server.poll() is None:
            check.api(kit, 'server.stop')
            check.require(server.wait(timeout=10) == 0, 'final server shutdown failed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('coven-bin', 'kitsune-bin'):
        parser.add_argument('--' + name, required=True, type=check.absolute)
    try:
        with check.owned_run(short=True) as root:
            run(parser.parse_args(), root)
    except (Exception, KeyboardInterrupt) as exc:
        print(f'FAIL {exc}', file=sys.stderr)
        sys.exit(1)
