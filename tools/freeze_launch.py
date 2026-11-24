#!/usr/bin/env python3
"""Capture only from an explicitly supplied historical Python reference tree.

The unit input table below is authored from src/launch/tests.rs at 24d73fe,
not from native outputs. Original-method inputs are observed while running the
unchanged original assertions. No launch implementation is copied here.
Run with Python 3.14.8 -I -B; no runtime commands or real delays are used.
"""
import argparse
import ast
import copy
import hashlib
import json
from pathlib import Path
import runpy
import subprocess
import sys
import unittest

PIN = "24d73fede7d8e305cc2572c623a0eb93b440e806"
BASE = "e389a08669204d4e51a49ffd13cbe01b90b5d8ba"
METHODS = [
    "strict_recomputed_schema",
    "happy_transport_literal_payload_and_exact_call_contract",
    "collisions_and_query_errors_have_no_startup_effects",
    "every_required_command_failure_stops_without_cleanup",
    "untrusted_json_and_handles_fail_cleanly",
    "sleep_failure_does_not_send_starter_or_clean_up",
    "ordered_roster_and_delay_for_each_agent",
    "stop_exact_target_absence_and_no_legacy_adoption",
    "stop_ambiguity_untrusted_identity_and_failure_never_retry",
    "tmux_query_failure_is_not_evidence_of_absence",
    "timed_out_stop_records_attempt_but_not_success",
    "orchestrator_role_uses_a_distinct_agent_window",
    "invalid_plan_stop_still_has_complete_report_schema",
]


def encoded(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")) + "\n").encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def inputs(backend, count):
    return dict(root="/not/read/../workspace with spaces//", namespace="coven-aA1-B2",
                multiplexer=backend,
                agents=[dict(id=f"agent_{i}", command=" exec worker --prompt /mutable/prompt.md ",
                             starter=f"- literal 'quotes' \"double\" $HOME $(touch nope)\n\u96ea {i}")
                        for i in range(count)],
                environment=dict(PATH="/usr/bin:/bin", HOME="/frozen", LANG="C.UTF-8",
                                 TERM="", SHELL="/bin/zsh", LC_ALL=" "))


def unit_cases():
    """Bounded input table mirroring every compare() family, including duplicates."""
    families = {}
    cases = families["builder"] = []
    for backend in ("tmux", "herdr"):
        for key, value in [("root", "relative"), ("root", "/bad\0path"),
                           ("namespace", "coven-human"), ("namespace", "coven-\uff11-ab"),
                           ("multiplexer", "screen"), ("agents", []), ("agents", None),
                           ("environment", []), ("environment", {"TOKEN": "secret"}),
                           ("environment", {"PATH": "a\0b"}),
                           ("environment", {"PATH": "x" * 32769})]:
            value_input = inputs(backend, 1)
            value_input[key] = value
            cases.append(dict(inputs=value_input))
        for key, value in [("id", "alice:other"), ("id", "x" * 129),
                           ("command", "\x1c\xa0\u3000"), ("starter", "a\0b"), ("extra", "hidden")]:
            value_input = inputs(backend, 1)
            value_input["agents"][0][key] = value
            cases.append(dict(inputs=value_input))
        duplicate = inputs(backend, 2)
        duplicate["agents"][1] = copy.deepcopy(duplicate["agents"][0])
        cases.extend([dict(inputs=duplicate), dict(inputs=inputs(backend, 33))])
        for lang in ("\xe9" * 16384, "\xe9" * 16384 + "x"):
            value_input = inputs(backend, 1)
            value_input["environment"] = dict(LANG=lang)
            cases.append(dict(inputs=value_input))
    cases = families["roster_required_failures"] = []
    for backend in ("tmux", "herdr"):
        for count in (1, 2, 32):
            case = dict(inputs=inputs(backend, count))
            cases.append(copy.deepcopy(case))
            case["inputs"]["environment"] = {}
            cases.append(case)
        value_input = inputs(backend, 2)
        # Independent fixed input cardinalities, not a native plan query.
        commands = 8 if backend == "tmux" else 11
        for index in range(1, commands):
            cases.append(dict(inputs=value_input, overrides={str(index): dict(status=9, stderr="failed\n")}))
        for status in ("Timeout", "SpawnError"):
            for index in range(commands):
                cases.append(dict(inputs=value_input, overrides={str(index): dict(status=status)}))
        for extra in (dict(sleep_error=True), dict(sleep_failure_at=1), dict(invalid_plan=True),
                      dict(stopping=True, invalid_plan=True)):
            cases.append(dict(inputs=value_input, **extra))
    cases = families["tmux_presence_stop"] = []
    for stopping in (False, True):
        for code in (0, 1, 2):
            for detail in ("", "can't find session: coven-aA1-B2", "can't find session: =coven-aA1-B2",
                           "no server running on /tmp/tmux-test",
                           "ERROR connecting to /tmp/tmux-test (No such file or directory)",
                           "failed to connect to /tmp/tmux-test (No such file or directory)",
                           "error connecting to tmux socket (Permission denied)", "unexpected socket failure",
                           "\x1ccan't find session: coven-aA1-B2\x1f"):
                cases.append(dict(inputs=inputs("tmux", 1), stopping=stopping,
                                  overrides={"0": dict(status=code, stderr=detail)}))
    for backend in ("tmux", "herdr"):
        cases.append(dict(inputs=inputs(backend, 1), stopping=True))
        for status in (0, 7, "Timeout", "SpawnError"):
            case = dict(inputs=inputs(backend, 1), stopping=True,
                        workspaces=[dict(label="coven-aA1-B2", workspace_id="scoped-id"),
                                    dict(label="human", workspace_id="legacy")],
                        overrides={"1": dict(status=status)})
            if backend == "tmux":
                case["overrides"]["0"] = dict(status=0)
            cases.append(case)
    cases = families["untrusted_exact"] = []
    value_input = inputs("herdr", 1)
    for workspaces in ([dict(label="coven-aA1-B2")],
                       [dict(label="coven-aA1-B2", workspace_id="bad;id")],
                       [dict(label="coven-aA1-B2", workspace_id="one"), dict(label="coven-aA1-B2", workspace_id="two")],
                       [dict(label="human-name", workspace_id="legacy")]):
        for stopping in (False, True):
            cases.append(dict(inputs=value_input, stopping=stopping, workspaces=workspaces))
    for bad in (None, "", "$(evil)", "a/b", "x" * 129, "\u96ea", 123):
        for index in (1, 3):
            payload = {"result": {"workspace": {"workspace_id": bad}, "root_pane": {"pane_id": "root"}}} if index == 1 else {"result": {"root_pane": {"pane_id": bad}}}
            cases.append(dict(inputs=value_input, overrides={str(index): dict(stdout=json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")))}))
    for index, payload in ((1, {}), (1, {"result": {"workspace": {"workspace_id": "observed"}}}), (3, {"result": {}})):
        cases.append(dict(inputs=value_input, overrides={str(index): dict(stdout=json.dumps(payload, sort_keys=True, separators=(",", ":")))}))
    for stdout in ('{"result":{"workspaces":null}}', '{"result":{"workspaces":[1]}}',
                   '{"result":{"workspaces":[],"workspaces":[]}}', '{"result":{"workspaces":[]},"extra":NaN}',
                   '{"result":{"workspaces":[]},"extra":Infinity}', '{"result":{"workspaces":[]},"extra":1e999}',
                   '{"result":{"workspaces":[]},"extra":-Infinity}', '[]', 'null', '1', 'true', '1.2',
                   '"string"', '{"result":null}'):
        cases.append(dict(inputs=value_input, overrides={"0": dict(stdout=stdout)}))
    cases = families["untrusted_parser_wording_exempt"] = []
    for index in (0, 1, 3):
        for stdout in ('{', '', '[] trailing', '{"result":'):
            cases.append(dict(inputs=value_input, overrides={str(index): dict(stdout=stdout)}))
    families["diagnostic_unicode_truncation"] = [
        dict(inputs=inputs(backend, 1), overrides={"1": dict(status=7, stderr="\U0001f980" * 3000)})
        for backend in ("tmux", "herdr")]
    return families


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-root", type=Path, required=True)
    args = parser.parse_args()
    if sys.version_info[:3] != (3, 14, 8) or not sys.flags.isolated or not sys.dont_write_bytecode:
        parser.error("requires Python 3.14.8 -I -B")
    project = Path(__file__).resolve().parents[1]
    root = args.oracle_root.resolve()
    if root == project:
        parser.error("use an explicit historical export, not the working tree")
    paths = ["runner/launch.py", "runner/actions.py",
             "tests/test_launch.py", "src/launch/oracle.py", "src/launch/tests.rs"]
    source_hashes = {}
    for path in paths:
        pinned = subprocess.check_output(["git", "show", f"{PIN}:{path}"], cwd=project)
        if (root / path).read_bytes() != pinned:
            parser.error(f"reference differs from {PIN}: {path}")
        source_hashes[path] = digest(pinned)
        if path in ("runner/launch.py", "tests/test_launch.py"):
            assert pinned == subprocess.check_output(["git", "show", f"{BASE}:{path}"], cwd=project)
    sys.path.insert(0, str(root))
    original = runpy.run_path(str(root / "tests/test_launch.py"))
    launch = original["launch"]
    records = []
    method_counts = {}
    active_method = None
    real_execute, real_stop = launch.execute, launch.stop

    def capture(plan, runner=None, sleeper=None, *, stopping=False):
        result = real_stop(plan, runner) if stopping else real_execute(plan, runner, sleeper)
        # Strict schema's nine arbitrary mutations are covered directly in Rust.
        if active_method == "strict_recomputed_schema":
            return result
        case = dict(inputs=dict(root=plan["root"], namespace=plan["namespace"], multiplexer=plan["multiplexer"],
                                agents=plan["agents"], environment=plan["env"]))
        if stopping:
            case["stopping"] = True
        if not plan["steps"]:
            case["invalid_plan"] = True
        if runner.workspaces:
            case["workspaces"] = runner.workspaces
        if runner.overrides:
            overrides = {}
            for index, value in runner.overrides.items():
                if isinstance(value, subprocess.TimeoutExpired):
                    overrides[str(index)] = dict(status="Timeout")
                else:
                    overrides[str(index)] = dict(status=value.returncode, stdout=value.stdout, stderr=value.stderr)
            case["overrides"] = overrides
        if active_method == "sleep_failure_does_not_send_starter_or_clean_up":
            case["sleep_error"] = True
        index = method_counts.get(active_method, 0)
        method_counts[active_method] = index + 1
        calls = [dict(argv=argv, **{key: kwargs[key] for key in ("cwd", "env", "timeout")})
                 for argv, kwargs in runner.calls]
        delays = [call.args[0] for call in sleeper.call_args_list] if hasattr(sleeper, "call_args_list") else None
        records.append(dict(id=f"original/{active_method}/{index:02}", method=active_method, case=copy.deepcopy(case),
                            observed_report=copy.deepcopy(result), observed_calls=copy.deepcopy(calls), observed_delays=delays))
        return result

    launch.execute = lambda p, runner=None, sleeper=None: capture(p, runner, sleeper)
    launch.stop = lambda p, runner=None: capture(p, runner, stopping=True)
    for method in METHODS:
        active_method = method
        result = unittest.TestResult()
        original["LaunchTests"]("test_" + method).run(result)
        if not result.wasSuccessful():
            raise RuntimeError(result.errors + result.failures)
    for family, cases in unit_cases().items():
        records.extend(dict(id=f"unit/{family}/{i:03}", family=family, case=case,
                            exact_diagnostics=family != "untrusted_parser_wording_exempt") for i, case in enumerate(cases))
    oracle = subprocess.run([sys.executable, "-I", "-B", str(root / "src/launch/oracle.py"), str(root)],
                            input=encoded([r["case"] for r in records]), capture_output=True, check=True)
    expected = json.loads(oracle.stdout)
    assert len(expected) == len(records)
    for record, value in zip(records, expected, strict=True):
        if "observed_report" in record:
            assert record.pop("observed_report") == value["report"], record["id"]
            assert record.pop("observed_calls") == value["calls"], record["id"]
            delays = record.pop("observed_delays")
            if delays is not None:
                assert delays == value["delays"], record["id"]
        record["expected"] = value
        record.setdefault("exact_diagnostics", not any(d["error"] == "JSONDecodeError" for d in value.get("report", {}).get("diagnostics", [])))
    corpus = encoded(records)
    target = project / "tests/retirement/fixtures"
    (target / "launch.json").write_bytes(corpus)
    source = (root / "tests/test_launch.py").read_text()
    methods = {}
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.FunctionDef) and node.name.removeprefix("test_") in METHODS:
            methods[node.name] = dict(sha256=digest(ast.get_source_segment(source, node).encode()),
                                      lines=[node.lineno, node.end_lineno],
                                      captured_cases=method_counts.get(node.name.removeprefix("test_"), 0))
    provenance = dict(schema_version=1, oracle_commit=PIN, unchanged_launch_since=BASE,
                      source_sha256=source_hashes, python=sys.version.split()[0], flags=["-I", "-B"],
                      capture_tool="tools/freeze_launch.py", capture_tool_sha256=digest(Path(__file__).read_bytes()),
                      corpus_sha256=digest(corpus), corpus_bytes=len(corpus), cases=len(records),
                      unit_families={k: len(v) for k, v in unit_cases().items()}, original_methods=methods,
                      original_methods_run=13,
                      inputs_provenance="Unit input table authored from pinned Rust tests; original inputs captured from unchanged passing Python methods. Expectations only from pinned Python reference, never Rust.",
                      strict_schema="Nine mutations run in original Python assertions; native B evidence, not oracle corpus records.")
    (target / "launch_provenance.json").write_bytes(encoded(provenance))
    print(json.dumps(provenance, indent=2))


if __name__ == "__main__":
    main()
