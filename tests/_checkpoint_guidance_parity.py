"""Literal next CLI oracle with disposable fixtures and zero-effect snapshots.

Profiles are native bounded-input/opaque-argv refusals, not claims of Python invalidity.
Opaque negative argv may fail native lexing before --; after -- it remains a
byte-exact path subject to native UTF-8 policy. This is not opaque-path parity.
Pure timestamp/parser unit tests belong to their implementation owners.
"""
import argparse
from collections import Counter
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from _checkpoint_workspace_parity import (
    A, B, MIB, TEMPLATE, actions, changed, checkpoints, coven, invoke,
    snapshot, storage, task_ledger,
)


def main(binary=None, oracle_only=False):
    binary = str(Path(binary).resolve()) if binary else None
    counts, names = Counter(), set()
    with tempfile.TemporaryDirectory(prefix="checkpoint-guidance-") as temporary:
        root = Path(temporary).resolve()
        env = {**os.environ, "HOME": str(root), "PATH": str(root / "no-tools"),
               "PYTHONPATH": str(root / "no-python"), "COVEN_MULTIPLEXER": "",
               "PYTHONDONTWRITEBYTECODE": "1"}

        def fixture(name, *, team_name=None):
            workspace = root / name
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                assert coven.main(["init", str(workspace), "--blank", "--agents", "operator",
                                   *(["--name", team_name] if team_name else [])]) == 0
            for log in ("messages", "events"):
                (workspace / f"logs/{log}.jsonl").write_bytes(b"")
            return workspace

        w = fixture("workspace with spaces \u96ea")
        cp = w / "checkpoints/plan-approval.md"
        cp.write_bytes(TEMPLATE)

        def records(messages=(), events=()):
            stack = ExitStack()
            for log, values in (("messages", messages), ("events", events)):
                raw = b"".join((json.dumps(value, ensure_ascii=True) + "\n").encode() for value in values)
                stack.enter_context(changed(w / f"logs/{log}.jsonl", raw))
            return stack

        def request(checkpoint="plan-approval", ts="2026-01-01", kind="checkpoint.request"):
            return {"checkpoint": checkpoint, "ts": ts, "type": kind}

        def literal(workspace, cwd, candidate):
            prefix = "" if workspace.resolve() == cwd.resolve() else str(workspace.resolve()) + " "
            if candidate is None:
                return f"Next action: inspect status/dashboard\nSuggested: coven status{(' ' + prefix.rstrip()) if prefix else ''}\n"
            return (f"Next action: approve checkpoint `{candidate}`\n"
                    "Reason: one or more agents are blocked waiting for user approval.\n"
                    f"Review file: {workspace.resolve()}/checkpoints/{candidate}.md\n"
                    f"Suggested: coven approve {prefix}{candidate}\n")

        def check(name, candidate=None, *, workspace=None, cwd=None, args=None,
                  ok=True, parser=False, error=None, native_error=None, profile=None,
                  help_output=False, no_internal=False):
            assert name not in names, name
            names.add(name)
            workspace = workspace or w
            cwd = cwd or workspace
            args = args if args is not None else ["next", str(workspace)]
            before = snapshot(root)
            expected = invoke(args, cwd, env, help_only=help_output)
            if not profile:
                assert (expected[0] == 0) == ok, (name, expected)
            if expected[0] == 0:
                assert expected[2] == "", (name, expected)
                if help_output:
                    assert "usage:" in expected[1], (name, expected)
                elif not profile:
                    assert expected[1] == literal(workspace, cwd, candidate), (name, expected)
            else:
                assert expected[1] == "", (name, "partial Python stdout", expected)
                if error:
                    assert error in expected[2], (name, expected)
            if help_output or no_internal:
                assert "__coven_checkpoint_operand_" not in expected[1] + expected[2], (name, expected)
            assert snapshot(root) == before, (name, "Python effects")
            if not oracle_only:
                result = subprocess.run([binary, *map(str, args)], cwd=cwd, env=env,
                                        capture_output=True, timeout=15)
                if profile or not ok:
                    assert result.returncode == (2 if parser else 1), (name, result)
                    assert result.stdout == b"" and result.stderr, (name, "partial native stdout", result)
                    if parser:
                        assert b"error:" in result.stderr, (name, result)
                    elif profile or native_error:
                        assert result.stderr == f"[ERROR] {profile or native_error}\n".encode(), (name, result)
                elif help_output:
                    assert result.returncode == 0 and result.stderr == b"" and b"Usage: coven next" in result.stdout, (name, result)
                else:
                    assert (result.returncode, result.stdout, result.stderr) == (
                        0, expected[1].encode("utf-8"), b""), (name, expected, result)
                if help_output or no_internal:
                    assert b"__coven_checkpoint_operand_" not in result.stdout + result.stderr, (name, result)
                assert snapshot(root) == before, (name, "native effects")
            counts["profile_refusals" if profile else "help_success" if help_output else "parser_refusals" if parser else
                   "parity_success" if ok else "parity_fatal"] += 1
            if profile and parser:
                counts["profile_parser_refusals"] += 1

        check("default-cwd", args=["next"])
        check("explicit-current")
        check("explicit-from-outside", cwd=root)
        alias = root / "alias"
        alias.symlink_to(w, target_is_directory=True)
        check("canonical-alias", args=["next", str(alias)])
        check("terminator", args=["next", "--", str(w)])
        for name, args in (("no-json", ["next", "--json"]),
                           ("no-abbreviation", ["next", "--j"]),
                           ("unknown", ["next", "--unknown"]),
                           ("excess", ["next", str(w), "extra"]),
                           ("negative-option", ["next", "-bad"])):
            check(name, args=args, ok=False, parser=True)
        check("explicit-unmarked-is-workspace", args=["next", "missing"], ok=False)
        check("no-ancestor-search", args=["next"], cwd=w / "work", ok=False)
        # Distinct real roots make numeric coercion or leaked lexer placeholders
        # observable in the exact review path and suggested command.
        for spelling in ("-.5", "-1e+2", "-1anything", "-\u0661", "-.\u0661", "-1_000",
                         "__coven_checkpoint_operand_0"):
            selected = fixture(spelling, team_name="operand-fixture")
            (selected / "logs/messages.jsonl").write_text(
                json.dumps(request()) + "\n", encoding="utf-8")
            check("exact-negative-root-" + spelling, "plan-approval", workspace=selected,
                  cwd=root, args=["next", spelling])
            check("exact-negative-root-terminator-" + spelling, "plan-approval", workspace=selected,
                  cwd=root, args=["next", "--", spelling])
            check("exact-negative-root-current-" + spelling, "plan-approval", workspace=selected,
                  args=["next"])
        check("literal-token-and-numeric-excess", cwd=root,
              args=["next", "__coven_checkpoint_operand_0", "-42"], ok=False, parser=True)
        check("extra-negative-after-existing-workspace", args=["next", str(w), "-1anything"],
              ok=False, parser=True, no_internal=True)
        check("two-adapted-negative-operands", args=["next", "-1anything", "-.2"],
              ok=False, parser=True, no_internal=True)
        check("empty-literal-workspace", args=["next", ""])
        for spelling in ("-inf", "-nan", "-\u2165", "-.\u2165"):
            selected = fixture(spelling, team_name="operand-fixture")
            check("nonnumeric-root-option-" + spelling, workspace=selected, cwd=root,
                  args=["next", spelling], ok=False, parser=True)
            check("nonnumeric-root-terminator-" + spelling, workspace=selected, cwd=root,
                  args=["next", "--", spelling])
        with changed(w / "coven.toml", b"invalid TOML!"):
            for label, operands in (("plain", ["--help"]),
                                    ("negative-before-help", ["-1anything", "--help"]),
                                    ("help-before-negative", ["--help", "-.2"])):
                check("help-early-" + label, args=["next", *operands], help_output=True)
        literal_help = fixture("--help", team_name="literal-help")
        check("terminator-help-is-root", workspace=literal_help, cwd=root,
              args=["next", "--", "--help"], no_internal=True)
        check("terminator-help-is-missing-path", args=["next", "--", "--help"],
              ok=False, no_internal=True)
        opaque_bytes = b"-1\xff"
        opaque_name = os.fsdecode(opaque_bytes)
        assert os.fsencode(opaque_name) == opaque_bytes
        opaque = root / opaque_name
        shutil.copytree(w, opaque)
        decoy = fixture(opaque_bytes.decode("utf-8", "replace"), team_name="lossy-decoy")
        (decoy / "coven.toml").write_bytes(b"invalid TOML!")
        check("opaque-negative-root-before-terminator", workspace=opaque, cwd=root,
              args=["next", opaque_name], parser=True,
              profile="opaque_negative_operand_parser_profile", no_internal=True)
        check("opaque-negative-root-after-terminator", workspace=opaque, cwd=root,
              args=["next", "--", opaque_name], profile="non_unicode_path_unsupported", no_internal=True)
        check("opaque-negative-missing-after-terminator", args=["next", "--", opaque_name],
              profile="invalid_checkpoint_workspace", no_internal=True)
        for log in ("messages", "events"):
            with changed(w / f"logs/{log}.jsonl", None, "missing"):
                check("missing-log-" + log)
        with changed(w / "checkpoints", None, "missing"):
            check("missing-checkpoint-directory")

        for status in ("pending", "changes_requested", "unknown", "approved_unbound"):
            raw = TEMPLATE.replace(b"pending", b"approved" if status == "approved_unbound" else status.encode())
            with changed(cp, raw):
                check("file-alone-" + status, "plan-approval" if status == "approved_unbound" else None)
                with records([request()]):
                    check("requested-" + status, "plan-approval")
        approved, _ = checkpoints.prepare("plan-approval", TEMPLATE, "approved", "review \u96ea",
                                           decision_id=A, decided_at="frozen", source="cli")
        rejected, _ = checkpoints.prepare("plan-approval", approved, "changes_requested", "again",
                                           decision_id=B, decided_at="later", source="review")
        for name, raw, candidate in (
                ("approved", approved, None), ("stale", approved + b"\nchanged\n", "plan-approval"),
                ("invalid", approved.replace(b"Status: approved", b"Status: pending"), "plan-approval"),
                ("rejected", rejected, "plan-approval")):
            with changed(cp, raw), records([request()]):
                check("bound-" + name, candidate)
        with changed(cp, None, "missing"), records([request()]):
            check("requested-missing", "plan-approval")

        for kind in ("request", "pre-request-post", "changes.requested", ["request"],
                     {"request": False}, {"nested": ["request"]}, [False, {"x": "request"}]):
            with records([request(kind=kind)]):
                check("request-type-" + repr(kind), "plan-approval")
        for kind in ("Request", "REQUEST", "approve", None, False, 1, 1.5, [], {}):
            with records([request(kind=kind)]):
                check("nonrequest-type-" + repr(kind))

        # Both pending files exist; a second request makes stable ordering observable.
        other = w / "checkpoints/other.md"
        with changed(other, TEMPLATE):
            with records([request(), request("other"), request()]):
                check("duplicates-retained", "plan-approval")
            with records([request()], [request("other")]):
                check("message-before-event-tie", "other")
            with records([request(ts="2027-01-01")], [request("other")]):
                check("chronology-not-log-order", "plan-approval")
            timestamps = ["20260102", "2026-W01-5", "2026W015", "2026-01-02",
                          "2026-01-02\u96ea01:02:03", "2026-01-02T01", "2026-01-02T0102",
                          "2026-01-02T01:02:03.123456789", "2026-01-02T01:02:03,5",
                          "2026-01-02T01:02:03Z", "2026-01-02T01:02:03+01:30:12.5",
                          "2026-01-02T01:02:03-0130", "0001-01-01T00:00:00+01:00",
                          "9999-12-31T23:59:59-01:00", "invalid", "", None, True, 20260102,
                          20260102.0, -0.0, [], {}]
            from datetime import datetime, timezone
            for index, ts in enumerate(timestamps):
                try:
                    value = datetime.fromisoformat(str(ts))
                    key = value.replace(tzinfo=timezone.utc) if value.tzinfo is None else value.astimezone(timezone.utc)
                except (ValueError, OverflowError):
                    key = datetime.min.replace(tzinfo=timezone.utc)
                baseline = datetime(2026, 1, 1, tzinfo=timezone.utc)
                with records([request(ts=ts)], [request("other")]):
                    check(f"timestamp-selection-{index}", "plan-approval" if key > baseline else "other")
            with changed(cp, b"<!-- coven-checkpoint-invalid:v99\n"), \
                    records([request(), request("other", "2027-01-01")]):
                check("inspect-every-request-not-just-last", ok=False)

        for name in ("a-first", "z-last"):
            (w / f"checkpoints/{name}.md").write_bytes(TEMPLATE.replace(b"pending", b"approved"))
        check("fallback-last-not-first", "z-last")
        (w / "checkpoints/a-first.md").unlink()
        (w / "checkpoints/z-last.md").unlink()
        with changed(w / "checkpoints/Upper.MD", TEMPLATE.replace(b"pending", b"approved")):
            check("uppercase-not-fallback")
        with changed(w / "checkpoints/Upper.MD", b"\xff"):
            check("uppercase-still-preflight", ok=False)
        for name, raw in (("stale", approved + b"\nchanged\n"),
                          ("invalid", approved.replace(b"Status: approved", b"Status: pending"))):
            with changed(cp, raw):
                check("fallback-" + name, "plan-approval")
        with changed(cp, approved), \
                changed(w / "checkpoints/z-last.md", TEMPLATE.replace(b"pending", b"approved")), \
                records([request()]):
            check("all-requested-approved-then-fallback", "z-last")
        with changed(w / "checkpoints/z-last.md", TEMPLATE.replace(b"pending", b"approved")), \
                records([request()]):
            check("pending-request-suppresses-fallback", "plan-approval")

        for field in (None, False, 0, "", [], {}, "../bad", "bad.id", "\u96ea"):
            for kind in ("request", "unrelated"):
                with records([{"checkpoint": field, "type": kind}]):
                    check("every-reference-" + repr((field, kind)), ok=False,
                          native_error="invalid_checkpoint_identifier" if isinstance(field, str)
                          else "invalid_checkpoint_history_reference")
        for log in ("messages", "events"):
            for name, tail in (("syntax", b"{\n"), ("utf8", b"\xff\n"),
                               ("array", b"[]\n"), ("scalar", b"null\n")):
                raw = (json.dumps(request()) + "\n").encode() + tail
                with changed(w / f"logs/{log}.jsonl", raw):
                    check(f"fatal-tail-{log}-{name}", ok=False,
                          native_error="invalid_jsonl_utf8" if name == "utf8" else "invalid_jsonl_record")
        with records([{"checkpoint": "../bad"}], [{"checkpoint": None}]):
            check("messages-reference-before-events", ok=False, error="../bad",
                  native_error="invalid_checkpoint_identifier")
        # Nonrequest references are still read. A suffixless checkpoint reference
        # targets lowercase .md, never a similarly spelled uppercase file.
        with changed(w / "checkpoints/referenced.md", b"\xff"), \
                records([request("referenced", kind="unrelated")]):
            check("nonrequest-reference-utf8", ok=False)
        with changed(w / "checkpoints/referenced.MD", TEMPLATE), \
                records([request("referenced")]):
            check("reference-does-not-select-uppercase", "referenced")
        with changed(w / "logs/messages.jsonl", b'{"checkpoint":"../bad"}\n{\n'):
            check("whole-message-syntax-before-reference", ok=False, error="line 2",
                  native_error="invalid_jsonl_record")
        with changed(w / "logs/messages.jsonl", (json.dumps(request()) + "\n").encode()), \
                changed(w / "coven.toml", b"invalid TOML!"), \
                changed(w / "logs/events.jsonl", b"{\n"):
            check("config-before-history", ok=False, error="Invalid workspace configuration",
                  native_error="invalid_workspace_toml")
        with changed(w / "logs/messages.jsonl", (json.dumps(request()) + "\n").encode()), \
                changed(w / "logs/events.jsonl", b"{\n"), \
                changed(cp, b"<!-- coven-checkpoint-invalid:v99\n"):
            check("whole-history-before-status", ok=False, error="line 1",
                  native_error="invalid_jsonl_record")
        with changed(w / "coven.json", b"{}"), \
                changed(w / "logs/messages.jsonl", b"{\n"):
            check("team-before-history", ok=False, native_error="missing_workspace_record_field")
        for relative in ("state/agents.json", "state/tasks.json", "state/lead.json"):
            with changed(w / relative, b"null" if "lead" not in relative else b"{}"), \
                    records([request()]):
                check("full-workspace-record-" + relative, ok=False)

        # Holding actual existing locks must not block either implementation.
        with storage.workspace_lock(w, w / "state/.workspace.lock"):
            check("held-workspace-lock")
        with changed(w / "state/task-ledger", None, "directory"):
            (w / "state/task-ledger/opaque").write_bytes(b"not authority")
            check("pending-ledger-snapshot")
        active = fixture("active-ledger")
        store = task_ledger.LedgerStore(active)
        with storage.workspace_lock(active, active / "state/.workspace.lock") as lock:
            view = store.import_tasks(storage.snapshot_revision((active / "state/tasks.json").read_bytes()), lock)
            store.publish_projection(view, lock)
        with changed(active / "state/tasks.json", b"\xff"):
            check("active-bad-projection", workspace=active)
        runtime = fixture("runtime-control")
        with ExitStack() as locks:
            gate = locks.enter_context(storage.transport_gate(runtime, runtime / "state/.runtime-transport.lock", exclusive=True))
            lock = locks.enter_context(storage.workspace_lock(runtime, runtime / "state/.workspace.lock"))
            actions.ActionStore(runtime).enable(lock, gate=gate)
            check("valid-runtime-held-locks", workspace=runtime)

        with changed(cp, b" " * MIB):
            check("text-at-limit")
        with changed(cp, b" " * (MIB + 1)):
            check("text-over-limit", profile="unsupported_checkpoint_workspace_input_profile")
        for log in ("messages", "events"):
            with changed(w / f"logs/{log}.jsonl", b" " * (8 * MIB - 1) + b"\n"):
                check("log-at-limit-" + log)
            with changed(w / f"logs/{log}.jsonl", b" " * (8 * MIB + 1)):
                check("log-over-limit-" + log, profile="unsupported_event_log_size_profile")
            with changed(w / f"logs/{log}.jsonl", b"\n" * 65536):
                check("lines-at-limit-" + log)
            with changed(w / f"logs/{log}.jsonl", b"\n" * 65537):
                check("lines-over-limit-" + log, profile="unsupported_event_log_line_profile")
            for name, raw, code in (
                    ("nonfinite", b'{"type":NaN}\n', "invalid_jsonl_record"),
                    ("surrogate", b'{"type":"\\ud800"}\n', "invalid_jsonl_record"),
                    ("depth", b'{"x":' + b'[' * 65 + b'0' + b']' * 65 + b'}\n', "unsupported_json_depth_profile")):
                with changed(w / f"logs/{log}.jsonl", raw):
                    check(f"finite-profile-{log}-{name}", profile=code)
        # Scan all entries, including ignored suffixes, not only selected checkpoints.
        directory = w / "checkpoints"
        existing = len(list(directory.iterdir()))
        for index in range(4096 - existing):
            (directory / f"ignored-{index}.bin").write_bytes(b"")
        check("directory-at-limit")
        with changed(directory / "one-more.bin", b""):
            check("directory-over-limit", profile="unsupported_checkpoint_scan_profile")
    print(json.dumps({"cases": len(names), "oracle_only": oracle_only,
                      "counts": dict(sorted(counts.items()))}, sort_keys=True))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary")
    parser.add_argument("--oracle-only", action="store_true")
    arguments = parser.parse_args()
    if sys.platform != "linux":
        parser.error("this filesystem oracle is Linux-only")
    if not arguments.oracle_only and not arguments.binary:
        parser.error("--binary is required unless --oracle-only")
    main(arguments.binary, arguments.oracle_only)
