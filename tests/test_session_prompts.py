"""Private native protocol fixtures only; no UI, terminal input or model calls."""

import contextlib
import hashlib
import io
import json
import os
import subprocess
import uuid
from unittest import mock

from runner import actions, coven, session_prompts as prompts, storage
from test_coven import CovenTestCase, snapshot
from test_group_publisher import FakeServer

RUNTIME = "6cd93c2e-7146-4dca-8d2f-9c7afcfe85f0"
REQUEST = "832492cf-947d-4ba4-8fa6-9c43a347a81b"


def native_handler(change=None):
    def handle(server, stream, request):
        response = {"version": 1, "id": request["id"], "runtime_id": RUNTIME}
        if request["method"] == "capabilities":
            response["result"] = {"version": 1, "type": "capabilities", "contract": prompts.NATIVE_CONTRACT,
                                  "runtime_id": RUNTIME, "max_prompt_bytes": 65536}
        elif request["method"] == "submit":
            raw = request["params"]["text"].encode("utf-8")
            response["result"] = {"type": "prompt_accepted", "contract": prompts.NATIVE_CONTRACT,
                "session_id": request["params"]["session_id"], "message_id": "msg_fixture",
                "prompt_sha256": hashlib.sha256(raw).hexdigest(), "prompt_bytes": len(raw)}
        else:
            raise AssertionError("unexpected method")
        if change is not None:
            response = change(request, response)
        if response is not None:
            stream.sendall(json.dumps(response).encode()+b"\n")
        return True
    return handle


class SessionPromptTests(CovenTestCase):
    def setUp(self):
        super().setUp()
        self.invoke("init", self.workspace, "--blank", "--agents", "operator")
        with coven.legacy_runtime_transport(self.workspace):
            pass
        self.prompt = self.root / "task.txt"
        self.prompt.write_text("Literal λ 🦊\n$(not a command); @not-an-attachment\n", encoding="utf-8")
        for name in ("run", "Popen"):
            patcher = mock.patch.object(subprocess, name, side_effect=AssertionError("native process"))
            guard = patcher.start()
            self.addCleanup(patcher.stop)
            self.addCleanup(guard.assert_not_called)

    def args(self, address, *, execute=True):
        args = ["session", "prompt", str(self.workspace), "--native-socket", address,
                "--expected-runtime", RUNTIME, "--session", "ses_fixture", "--request-id", REQUEST,
                "--prompt-file", str(self.prompt), "--json"]
        return args+["--execute"] if execute else args

    def call(self, address, *, execute=True, expected=0, args=None):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(coven.main(self.args(address, execute=execute) if args is None else args), expected)
        return json.loads(output.getvalue())

    def plan(self, address):
        return prompts.plan(str(self.workspace), address, RUNTIME, "ses_fixture", REQUEST, str(self.prompt))

    def test_preview_is_pure_and_does_not_read_prompt_or_workspace(self):
        before = snapshot(self.root)
        self.prompt.unlink()
        args = coven.parse_args(self.args("/missing/socket", execute=False))
        with mock.patch.object(prompts.os, "open", side_effect=AssertionError("open")), \
             mock.patch.object(prompts.os, "lstat", side_effect=AssertionError("stat")), \
             mock.patch.object(prompts.socket, "socket", side_effect=AssertionError("network")):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(coven.command_session_prompt(args), 0)
        report = json.loads(output.getvalue())
        self.assertEqual(report["delivery"], "not_attempted")
        self.assertEqual(report["authorization"], "none")
        self.assertIsNone(report["prompt_sha256"])
        after = snapshot(self.root)
        before.pop("task.txt")
        self.assertEqual(after, before)

    def test_invalid_arguments_fail_before_io(self):
        for flag, value in (("--native-socket", "relative"), ("--expected-runtime", "w1:p1"),
                            ("--request-id", "not-a-uuid"), ("--session", "folder"),
                            ("--prompt-file", "/x/../y"), ("--timeout", "nan")):
            args = self.args("/missing/socket")
            if flag in args:
                args[args.index(flag)+1] = value
            else:
                args.extend([flag, value])
            with self.subTest(flag=flag), mock.patch.object(prompts.socket, "socket", side_effect=AssertionError("network")), \
                 contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                coven.main(args)

    def test_frozen_plan_rejects_extra_controls_and_boolean_version(self):
        value = self.plan("/socket")
        for mutation in ({"schema_version": True}, {"model": "override"}, {"text": "not approved"}):
            changed = {**value, **mutation}
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                prompts.preview(changed)

    def test_exact_native_admission_no_terminal_calls_and_no_workspace_writes(self):
        before = snapshot(self.workspace)
        with FakeServer({}, native_handler()) as server:
            report = self.call(server.path)
            self.assertEqual([r["method"] for r in server.requests], ["capabilities", "submit"])
            self.assertEqual(server.requests[-1]["params"]["text"], self.prompt.read_text())
            self.assertEqual(server.requests[-1]["id"], REQUEST)
        self.assertEqual(report["delivery"], "accepted")
        self.assertEqual(report["native_prompt_acceptance"], "accepted")
        self.assertEqual(report["native_message_id"], "msg_fixture")
        self.assertEqual(report["native_worker_acceptance"], "unestablished")
        self.assertEqual(report["job_completion"], "unestablished")
        self.assertNotIn("not a command", json.dumps(report))
        self.assertEqual(snapshot(self.workspace), before)

    def test_capabilities_query_is_metadata_only(self):
        with FakeServer({}, native_handler()) as server:
            report = self.call(server.path, args=["session", "prompt-capabilities", "--native-socket", server.path, "--json"])
            self.assertEqual([r["method"] for r in server.requests], ["capabilities"])
        self.assertEqual(report["runtime_id"], RUNTIME)
        self.assertEqual(report["authorization"], "none")

    def test_changed_runtime_refuses_before_submission(self):
        def change(request, response):
            response["runtime_id"] = response["result"]["runtime_id"] = str(uuid.uuid4())
            return response
        with FakeServer({}, native_handler(change)) as server:
            report = self.call(server.path, expected=1)
            self.assertEqual([r["method"] for r in server.requests], ["capabilities"])
        self.assertEqual(report["delivery"], "unsent")
        self.assertEqual(report["diagnostics"], ["native_runtime_mismatch"])

    def test_unsupported_capability_never_probes_with_a_write(self):
        for field, value in (("version", True), ("contract", "other"), ("max_prompt_bytes", True)):
            def change(request, response):
                response["result"][field] = value
                return response
            with self.subTest(field=field), FakeServer({}, native_handler(change)) as server:
                report = self.call(server.path, expected=1)
                self.assertEqual(len(server.requests), 1)
            self.assertEqual(report["delivery"], "unsent")

    def test_missing_ack_is_uncertain_and_never_retried(self):
        def change(request, response):
            return None if request["method"] == "submit" else response
        with FakeServer({}, native_handler(change)) as server:
            report = self.call(server.path, expected=2)
            self.assertEqual([r["method"] for r in server.requests], ["capabilities", "submit"])
        self.assertEqual(report["delivery"], "uncertain")
        self.assertEqual(report["native_prompt_acceptance"], "unestablished")

    def test_malformed_or_mismatched_ack_is_uncertain(self):
        changes = [lambda r: r.update(id=str(uuid.uuid4())), lambda r: r.update(version=True),
                   lambda r: r.update(runtime_id=str(uuid.uuid4())),
                   lambda r: r["result"].update(session_id="ses_other"),
                   lambda r: r["result"].update(prompt_sha256="0"*64),
                   lambda r: r["result"].update(prompt_bytes=True),
                   lambda r: r["result"].update(message_id="w1:p1"),
                   lambda r: r.update(error={"code": [], "delivery": []})]
        for mutate in changes:
            def change(request, response):
                if request["method"] == "submit":
                    mutate(response)
                return response
            with self.subTest(mutate=mutate), FakeServer({}, native_handler(change)) as server:
                self.assertEqual(self.call(server.path, expected=2)["delivery"], "uncertain")
                self.assertEqual(len(server.requests), 2)

    def test_typed_rejection_and_duplicate_uncertainty(self):
        for code, delivery, exit_code in (("session_unavailable", "rejected", 1),
                                         ("request_already_seen", "uncertain", 2),
                                         ("admission_uncertain", "uncertain", 2)):
            def change(request, response):
                if request["method"] == "submit":
                    response.pop("result")
                    response["error"] = {"code": code, "delivery": delivery}
                return response
            with self.subTest(code=code), FakeServer({}, native_handler(change)) as server:
                report = self.call(server.path, expected=exit_code)
                self.assertEqual(report["delivery"], delivery)
                self.assertEqual(len(server.requests), 2)

    def test_unsafe_parent_and_socket_permissions_fail_without_contact(self):
        with FakeServer({}, native_handler()) as server:
            os.chmod(os.path.dirname(server.path), 0o755)
            self.assertEqual(self.call(server.path, expected=1)["delivery"], "unsent")
            os.chmod(os.path.dirname(server.path), 0o700)
            os.chmod(server.path, 0o666)
            self.assertEqual(self.call(server.path, expected=1)["delivery"], "unsent")
            self.assertEqual(server.requests, [])

    def test_prompt_file_bounds_utf8_and_symlink_rejection(self):
        with FakeServer({}, native_handler()) as server:
            for raw in (b"", b" \t\n", b"x"*65537, b"\xff", b"secret\x00text"):
                self.prompt.write_bytes(raw)
                self.assertEqual(self.call(server.path, expected=1)["delivery"], "unsent")
            self.prompt.unlink()
            self.prompt.symlink_to(self.workspace / "goal.md")
            self.assertEqual(self.call(server.path, expected=1)["delivery"], "unsent")
            self.assertEqual(server.requests, [])

    def test_missing_or_wrong_gate_cannot_invoke_endpoint(self):
        with self.assertRaises(storage.StorageError):
            prompts.execute(self.plan("/socket"), gate=None)
        other = self.root / "other"
        other.mkdir()
        with storage.transport_gate(other, other/"state/.runtime-transport.lock") as gate, \
             self.assertRaises(storage.StorageError):
            prompts.execute(self.plan("/socket"), gate=gate)

    def test_approved_launch_mode_blocks_before_io(self):
        self.invoke("action", "enable-runtime-launch", self.workspace, "--json")
        with mock.patch.object(prompts.socket, "socket", side_effect=AssertionError("network")):
            self.invoke(*self.args("/socket"), error="Approved runtime-launch mode")
            with storage.transport_gate(self.workspace, self.workspace/"state/.runtime-transport.lock") as gate, \
                 self.assertRaises(actions.ActionError):
                prompts.execute(self.plan("/socket"), gate=gate)

    def test_orphaned_authority_blocks_before_io(self):
        directory = self.workspace / "state/actions"
        directory.mkdir(exist_ok=True)
        (directory / "orphan.json").write_text("{}\n")
        with mock.patch.object(prompts.socket, "socket", side_effect=AssertionError("network")), \
             contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            coven.main(self.args("/socket"))

    def test_maximum_prompt_size_is_accepted_without_text_in_report(self):
        self.prompt.write_bytes(b"x"*65536)
        with FakeServer({}, native_handler()) as server:
            report = self.call(server.path)
            self.assertEqual(report["prompt_bytes"], 65536)
            self.assertEqual(len(server.requests[-1]["params"]["text"]), 65536)
        self.assertNotIn("x"*100, json.dumps(report))

    def test_endpoint_replacement_after_capabilities_is_unsent(self):
        original = prompts._exchange
        with FakeServer({}, native_handler()) as server, FakeServer({}, native_handler()) as replacement:
            def exchange(endpoint, method, *args):
                result = original(endpoint, method, *args)
                if method == "capabilities":
                    os.rename(server.path, server.path+".old")
                    os.rename(replacement.path, server.path)
                return result
            with mock.patch.object(prompts, "_exchange", side_effect=exchange):
                report = self.call(server.path, expected=1)
            self.assertEqual(report["delivery"], "unsent")
            self.assertEqual(len(server.requests), 1)
            self.assertEqual(replacement.requests, [])

    def test_response_byte_overflow_after_mutation_is_uncertain(self):
        base = native_handler()
        def handler(server, stream, request):
            if request["method"] == "submit":
                stream.sendall(b"x"*(prompts.RESPONSE_BYTES+1))
                return True
            return base(server, stream, request)
        with FakeServer({}, handler) as server:
            report = self.call(server.path, expected=2)
            self.assertEqual(report["delivery"], "uncertain")
            self.assertEqual(len(server.requests), 2)

    def test_transport_gate_held_but_global_workspace_lock_free_during_native_io(self):
        checked = []
        def change(request, response):
            if request["method"] == "submit":
                with self.assertRaises(storage.LockBusy):
                    with storage.transport_gate(self.workspace, self.workspace/"state/.runtime-transport.lock", exclusive=True, timeout=0):
                        pass
                with storage.workspace_lock(self.workspace, self.workspace/"state/.workspace.lock", timeout=0):
                    checked.append(True)
            return response
        with FakeServer({}, native_handler(change)) as server:
            self.call(server.path)
        self.assertEqual(checked, [True])

    def test_interrupt_before_or_during_mutation_has_no_retry(self):
        original = prompts._exchange
        for operation, expected_delivery in (("capabilities", "unsent"), ("submit", "uncertain")):
            def exchange(endpoint, method, *args):
                if method == operation:
                    raise prompts.PromptError("interrupted", expected_delivery, interrupted=True)
                return original(endpoint, method, *args)
            with self.subTest(operation=operation), FakeServer({}, native_handler()) as server, \
                 mock.patch.object(prompts, "_exchange", side_effect=exchange):
                report = self.call(server.path, expected=130)
                self.assertEqual(report["delivery"], expected_delivery)

    def test_interruption_after_recorded_ack_retains_accepted_fact(self):
        original = prompts.preview
        class Interrupted(dict):
            fired = False
            def update(self, *args, **kwargs):
                super().update(*args, **kwargs)
                if kwargs.get("outcome") == "accepted" and not self.fired:
                    self.fired = True
                    raise KeyboardInterrupt
        with FakeServer({}, native_handler()) as server, \
             mock.patch.object(prompts, "preview", side_effect=lambda value: Interrupted(original(value))):
            report = self.call(server.path, expected=130)
        self.assertEqual(report["delivery"], "accepted")
        self.assertEqual(report["native_prompt_acceptance"], "accepted")
