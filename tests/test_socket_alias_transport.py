"""Offline alias transport regressions; never discover or start a real runtime."""

import contextlib
import io
import json
import os
from pathlib import Path
import socket
import tempfile
import time
import unittest
from unittest import mock

from runner import coven, group_publisher as publisher, groups
from runner import kitsune_observation as observation
from runner import kitsune_process_observation as process_observation
import test_group_publisher as publication_fixtures
import test_kitsune_observation as observation_fixtures
import test_process_groups as process_fixtures


ROOT = Path(__file__).resolve().parents[1]


@contextlib.contextmanager
def long_canonical_endpoint(server):
    """Move a bound socket without changing its inode, then alias its old parent."""
    original = Path(server.directory.name)
    before = Path(server.path).stat()
    with tempfile.TemporaryDirectory(dir=ROOT, prefix=".alias-") as directory:
        nested = Path(directory) / ("long-" + "x" * 100) / ("y" * 60)
        nested.mkdir(parents=True)
        moved = nested / "endpoint"
        original.rename(moved)
        try:
            original.symlink_to(moved, target_is_directory=True)
            canonical = moved / "s"
            # The fixture already bound this selected address successfully; do
            # not impose a smaller arbitrary limit on deeper checkout paths.
            assert len(os.fsencode(server.path)) < len(os.fsencode(canonical))
            assert len(os.fsencode(canonical)) > 160
            assert (canonical.stat().st_dev, canonical.stat().st_ino) == (before.st_dev, before.st_ino)
            yield str(canonical)
        finally:
            if original.is_symlink():
                original.unlink()
            moved.rename(original)


@contextlib.contextmanager
def intercepted_socket(module, callback, *, stage="connect"):
    """Inject a filesystem race at a real connection or its final EOF receipt."""
    real_socket = socket.socket
    proxies = []

    @contextlib.contextmanager
    def make_socket(*args, **kwargs):
        with real_socket(*args, **kwargs) as stream:
            proxy = mock.Mock(wraps=stream)
            proxies.append(proxy)

            def connect(address):
                stream.connect(address)
                if stage == "connect":
                    callback()

            def recv(size):
                chunk = stream.recv(size)
                if not chunk and stage == "eof":
                    callback()
                return chunk

            proxy.connect.side_effect = connect
            proxy.recv.side_effect = recv
            yield proxy

    network = mock.Mock(AF_UNIX=socket.AF_UNIX, SOCK_STREAM=socket.SOCK_STREAM,
                        timeout=socket.timeout)
    network.socket.side_effect = make_socket
    with mock.patch.object(module, "socket", network):
        yield proxies


class SocketAliasTransportTests(unittest.TestCase):
    def call(self, *args):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = coven.main(list(map(str, args)))
        self.assertEqual(code, 0, output.getvalue())
        return json.loads(output.getvalue())

    def test_default_observation_uses_short_alias_and_reports_canonical_identity(self):
        with observation_fixtures.FakeSocket(
                observation_fixtures.sender(observation_fixtures.reply)) as server:
            with long_canonical_endpoint(server) as canonical:
                with intercepted_socket(observation, lambda: None) as streams:
                    report = observation.inspect(server.path)
                self.assertEqual(report["assessment"], "observed", report)
                info = Path(canonical).stat()
                self.assertEqual(report["endpoint"], {
                    "selected": server.path, "resolved": canonical,
                    "device": info.st_dev, "inode": info.st_ino})
                self.assertEqual(len(streams), 1)
                streams[0].connect.assert_called_once_with(server.path)
                request, = server.requests
                self.assertEqual(request.count(b"\n"), 1)
                self.assertEqual(json.loads(request)["method"], "session.snapshot")

    def test_process_shadow_observation_uses_short_alias_for_entire_bracket(self):
        with publication_fixtures.FakeServer(
                process_fixtures.pane_inventory(), process_fixtures.process_handler()) as server:
            with long_canonical_endpoint(server) as canonical:
                with intercepted_socket(observation, lambda: None) as streams:
                    report = process_observation.inspect(server.path, "w1:p1")
                self.assertEqual(report["assessment"], "observed", report)
                self.assertEqual(report["endpoint"]["resolved"], canonical)
                self.assertEqual(report["observations"][0]["recognition"], "djinn_executable_family")
                self.assertEqual([r["method"] for r in server.requests], process_fixtures.BRACKET)
                self.assertEqual(len(streams), 3)
                for stream in streams:
                    stream.connect.assert_called_once_with(server.path)
                self.assertFalse(report["dispatchable"])

    def test_process_group_add_and_publish_store_canonical_endpoint(self):
        with tempfile.TemporaryDirectory(dir=ROOT, prefix=".alias-work-") as directory:
            workspace = Path(directory) / "workspace"
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(coven.main(["init", str(workspace)]), 0)
            self.call("group", "create", workspace, "team", "--name", "Review", "--json")
            with publication_fixtures.FakeServer(
                    process_fixtures.pane_inventory(), process_fixtures.process_handler()) as server:
                with long_canonical_endpoint(server) as canonical:
                    added = self.call("group", "add", workspace, "team", "--member", "one",
                                      "--role", "Reviewer", "--socket", server.path,
                                      "--pane", "w1:p1", "--process-evidence", "--json")
                    info = Path(canonical).stat()
                    endpoint = {"path": canonical, "device": info.st_dev, "inode": info.st_ino}
                    self.assertEqual(added["member"]["binding"]["presentation"]["endpoint"], endpoint)
                    clock = publication_fixtures.Clock()
                    with contextlib.ExitStack() as stack:
                        for module in (publisher, observation, process_observation):
                            stack.enter_context(mock.patch.object(module, "time", clock))
                        report = publisher.publish_group(workspace, "team", server.path,
                                                         "coven", duration=1)
                    self.assertEqual(report["outcome"], "completed", report)
                    self.assertEqual(report["acknowledged"], 1)
                    self.assertEqual([r["method"] for r in server.requests],
                                     process_fixtures.BRACKET + ["session.snapshot"]
                                     + process_fixtures.BRACKET + ["pane.annotate"])
                    state = groups.read_groups(workspace)["state"]
                    member, = state["groups"][0]["members"]
                    self.assertEqual(member["binding"]["presentation"]["endpoint"], endpoint)
                    annotation = server.requests[-1]["params"]
                    self.assertEqual(annotation["expected_terminal_id"], "terminal-1")
                    self.assertEqual(annotation["annotation_epoch"], "epoch-1")
                    self.assertEqual(annotation["revision"], 1)
                    self.assertEqual(annotation["ttl_ms"], 6000)
                    self.assertFalse(report["dispatchable"])

    def test_publication_snapshot_uses_short_alias_once_and_keeps_canonical_identity(self):
        with publication_fixtures.FakeServer() as server:
            with long_canonical_endpoint(server) as canonical:
                with intercepted_socket(publisher, lambda: None) as streams:
                    report = publisher.read_publication_snapshot(
                        server.path, deadline=time.monotonic() + 1)
                info = Path(canonical).stat()
                self.assertEqual(report["endpoint"], {
                    "path": canonical, "device": info.st_dev, "inode": info.st_ino})
                self.assertEqual(len(streams), 1)
                streams[0].connect.assert_called_once_with(server.path)
                self.assertEqual([r["method"] for r in server.requests], ["session.snapshot"])

    def test_long_caller_alias_uses_short_canonical_for_both_transports(self):
        with publication_fixtures.FakeServer() as server:
            nested = Path(server.directory.name) / ("a" * 100) / ("b" * 60)
            nested.mkdir(parents=True)
            alias = nested / "alias"
            alias.symlink_to(server.directory.name, target_is_directory=True)
            selected = str(alias / "s")
            canonical = os.path.realpath(server.path)
            self.assertGreater(len(os.fsencode(selected)), 160)
            for module in (observation, publisher):
                with self.subTest(module=module.__name__), intercepted_socket(module, lambda: None) as streams:
                    if module is observation:
                        report = observation.inspect(selected)
                        self.assertEqual(report["assessment"], "observed", report)
                        self.assertEqual(report["endpoint"]["resolved"], canonical)
                    else:
                        report = publisher.read_publication_snapshot(selected, deadline=time.monotonic() + 1)
                        self.assertEqual(report["endpoint"]["path"], canonical)
                    self.assertEqual(len(streams), 1)
                    streams[0].connect.assert_called_once_with(canonical)

    def test_retargeted_parent_alias_rejected_before_send_and_after_receipt(self):
        for module in (observation, publisher):
            for stage in ("connect", "eof"):
                with self.subTest(module=module.__name__, stage=stage), \
                        publication_fixtures.FakeServer() as original, \
                        publication_fixtures.FakeServer() as replacement:
                    alias = Path(original.directory.name) / "alias"
                    alias.symlink_to(original.directory.name, target_is_directory=True)
                    selected = str(alias / "s")
                    frozen = publisher._endpoint(selected)

                    def retarget():
                        alias.unlink()
                        alias.symlink_to(replacement.directory.name, target_is_directory=True)

                    with intercepted_socket(module, retarget, stage=stage) as streams:
                        if module is observation:
                            report = observation.inspect(selected)
                            self.assertEqual(report["assessment"], "unavailable", report)
                            self.assertEqual(report["diagnostics"], ["endpoint_replaced"])
                            self.assertEqual(report["agents"], [])
                        else:
                            with self.assertRaises(publisher.PublisherError) as caught:
                                publisher._request(selected, "pane.annotate", {},
                                                   deadline=time.monotonic() + 1,
                                                   frozen_endpoint=frozen)
                            self.assertEqual(caught.exception.code, "endpoint_replaced")
                            self.assertEqual(caught.exception.delivery,
                                             "unsent" if stage == "connect" else "uncertain")
                    self.assertEqual(publisher._endpoint(original.path), frozen)
                    self.assertEqual(len(streams), 1)
                    streams[0].connect.assert_called_once()
                    if stage == "connect":
                        streams[0].sendall.assert_not_called()
                        self.assertEqual(original.requests, [])
                    self.assertEqual(replacement.requests, [])

    def test_short_alias_does_not_bypass_leaf_or_permission_checks(self):
        for mode in ("leaf", "permissions"):
            with self.subTest(mode=mode), publication_fixtures.FakeServer() as server:
                with long_canonical_endpoint(server):
                    selected = server.path
                    if mode == "leaf":
                        leaf = Path(server.directory.name) / "leaf"
                        leaf.symlink_to("s")
                        selected = str(leaf)
                    else:
                        os.chmod(server.path, 0o660)
                    report = observation.inspect(selected)
                    self.assertEqual(report["diagnostics"], ["unsafe_endpoint"])
                    with self.assertRaises(publisher.PublisherError) as caught:
                        publisher.read_publication_snapshot(selected, deadline=time.monotonic() + 1)
                    self.assertEqual(caught.exception.code, "unsafe_endpoint")
                    self.assertEqual(server.requests, [])

    def test_alias_read_transport_still_requires_eof_and_one_deadline(self):
        def keep_open(server, stream, request):
            stream.sendall(observation_fixtures.reply(request))
            server.stop.wait(1)

        for module in (observation, publisher):
            with self.subTest(module=module.__name__), \
                    observation_fixtures.FakeSocket(keep_open) as server:
                with long_canonical_endpoint(server):
                    started = time.monotonic()
                    if module is observation:
                        report = observation.inspect(server.path, timeout=0.08)
                        self.assertEqual(report["diagnostics"], ["timeout"])
                        self.assertEqual(report["agents"], [])
                    else:
                        with self.assertRaises(publisher.PublisherError) as caught:
                            publisher.read_publication_snapshot(server.path, deadline=started + 0.08)
                        self.assertEqual(caught.exception.code, "timeout")
                    self.assertLess(time.monotonic() - started, 0.5)
                    self.assertEqual(len(server.requests), 1)

    def test_alias_read_transport_still_rejects_extra_frames(self):
        for module in (observation, publisher):
            handler = observation_fixtures.sender(
                lambda request: observation_fixtures.reply(request) + b"{}\n")
            with self.subTest(module=module.__name__), observation_fixtures.FakeSocket(handler) as server:
                with long_canonical_endpoint(server):
                    if module is observation:
                        report = observation.inspect(server.path)
                        self.assertEqual(report["diagnostics"], ["trailing_data"])
                    else:
                        with self.assertRaises(publisher.PublisherError) as caught:
                            publisher.read_publication_snapshot(server.path, deadline=time.monotonic() + 1)
                        self.assertEqual(caught.exception.code, "trailing_data")

    def test_method_allowlists_reject_native_control_before_connect(self):
        with publication_fixtures.FakeServer() as server:
            endpoint = publisher._endpoint(server.path)
            selected = {"selected": server.path, "resolved": endpoint["path"],
                        "device": endpoint["device"], "inode": endpoint["inode"]}
            for module in (observation, publisher):
                with self.subTest(module=module.__name__), \
                        mock.patch.object(module, "socket") as network, self.assertRaises(ValueError):
                    if module is observation:
                        observation._read_request(selected, "pane.send", {},
                                                  deadline=time.monotonic() + 1,
                                                  max_response_bytes=1024)
                    else:
                        publisher._request(server.path, "pane.send", {}, deadline=time.monotonic() + 1)
                network.socket.assert_not_called()


if __name__ == "__main__":
    unittest.main()
