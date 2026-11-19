#!/usr/bin/env python3
"""Small input-pump regressions; actual Kit behavior is qualified separately."""
import struct
import unittest
from unittest.mock import patch

import check


def frame(text, width=1, height=1):
    cells = text.ljust(width * height)
    payload = check.uint(1) + check.uint(width * height)
    for char in cells:
        encoded = char.encode()
        payload += check.uint(len(encoded)) + encoded + bytes(5)
    payload += check.uint(width) + check.uint(height)
    return struct.pack('<I', len(payload)) + payload


def inspector(selected=True):
    rows = [' ' * 100 for _ in range(32)]
    for y, text in [(1, 'Tab: Items'), (2, 'Filter: Sample A'), (3, '> coven'),
                    (10, ('> ' if selected else '  ') + '? Sample A'),
                    (19, 'Attention' if selected else 'Select an item.')]:
        rows[y] = '|' + text.ljust(98) + '|'
    return '\n'.join(rows)


class PumpTests(unittest.TestCase):
    def client(self, chunks, width=1, height=1):
        client = check.ViewClient.__new__(check.ViewClient)
        client.width, client.height = width, height
        client.text, client.generation, client.pending = '', 0, bytearray()
        client.sock = unittest.mock.Mock()
        client.sock.recv.side_effect = lambda size: chunks.pop(0)
        self.enterContext(patch.object(check.select, 'select', side_effect=lambda *args: ([client.sock] if chunks else [], [], [])))
        return client

    def test_drains_multiple_receives_and_partial_frame_to_newest(self):
        last = frame('c')
        client = self.client([frame('a'), frame('b') + last[:7], last[7:]])
        self.assertEqual(client.poll(), 'c')
        self.assertEqual(client.generation, 3)

    def test_continuous_queue_stops_at_frame_budget_and_resumes(self):
        client = self.client([frame('a') * 40])
        client.poll()
        self.assertEqual(client.generation, 32)
        client.poll()
        self.assertEqual(client.generation, 40)

    def test_selection_loss_cannot_hide_behind_newer_recovery(self):
        lost = frame(inspector(False).replace('\n', ''), 100, 32)
        recovered = frame(inspector().replace('\n', ''), 100, 32)
        client = self.client([lost + recovered], 100, 32)
        client.text = inspector()
        with client.selected_frames():
            with self.assertRaisesRegex(AssertionError, 'selection continuity lost at frame 1'):
                client.poll()

    def test_read_only_click_recomputes_hit_and_requires_postcondition(self):
        client = check.ViewClient.__new__(check.ViewClient)
        client.generation, client.text = 0, ''
        clicks = []

        def poll():
            client.generation += 1
            return client.text

        def mouse(x, y):
            clicks.append((x, y))
            if len(clicks) == 2:
                client.text = 'Tab: Details'

        client.poll, client.mouse = poll, mouse
        client.inspector_click(lambda: (3, client.generation), lambda: 'Tab: Details' in client.text,
                               'unit focus', timeout=2)
        self.assertEqual(len(clicks), 2)
        self.assertGreater(clicks[1][1], clicks[0][1])


if __name__ == '__main__':
    unittest.main()
