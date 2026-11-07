"""Test-only oracle. prepare() constructs bindings, not approval permission.

Emit JSONL: id, checkpoint, rawbyteshex (or null), accepted, expected view.
Explicit resource fixtures additionally carry the exact expectednative_error.
No runtime/CLI/workspace setup; the production Rust module never invokes this.
"""
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from runner import checkpoints as cp


TEMPLATE = (b"# Plan\n\nStatus: pending\n\n## Purpose\nShip the plan.\n\n"
            b"## User decision\n\n- [ ] Approved\n- [ ] Rejected / needs changes\n")


def prepare(data, action="approved", number=1, notes="Review rationale", **kwargs):
    return cp.prepare("plan", data, action, notes, decision_id=f"{number:032x}",
                      decided_at="arbitrary scalar text, NOT an ISO timestamp",
                      source="cli", **kwargs)


def replace_record(data, kind, transform):
    marker = f"<!-- coven-checkpoint-{kind}:v1\n".encode()
    start = data.index(marker) + len(marker)
    end = data.index(b"\n-->", start)
    value = json.loads(data[start:end])
    transformed = transform(value)
    payload = (transformed if isinstance(transformed, bytes) else
               json.dumps(transformed, ensure_ascii=True).encode())
    return data[:start] + payload + data[end:]


def corpus():
    yield "missing", None
    yield "empty", b""
    yield "template", TEMPLATE
    for ending in (b"\n", b"\r\n", b"\r"):
        for raw in (b"", b"# Plan", TEMPLATE, TEMPLATE + "\nUnicode: 雪 café  \n".encode()):
            yield f"line-ending-{ending.hex()}-{raw.hex()}", raw.replace(b"\n", ending)
    for status in ("APPROVED", "Changes_Requested", "custom", "", " \t ",
                   "\x1c\u00a0ΟΣ\x1f", "ΟΣΑ", "ΟΣ\u0301", "ΟΣ'", "İ", "\u0085Custom\u2028"):
        yield "status-" + repr(status), TEMPLATE.replace(b"pending", status.encode())
    approved, view = prepare(TEMPLATE)
    rejected, _ = prepare(TEMPLATE, "changes_requested")
    chain = approved
    for number, action in enumerate(("changes_requested", "approved", "changes_requested"), 2):
        chain, _ = prepare(chain, action, number)
        yield f"history-chain-{number}", chain
    yield "approved", approved
    yield "rejected", rejected
    yield "missing-created", prepare(None, initial_content=TEMPLATE)[0]
    for raw in (b"", b"# Plan", b"# Plan\nCustom prose.\n", b"Status: pending",
                b"## User decision\n- [ ] Approved"):
        yield "sparse-" + raw.hex(), prepare(raw)[0]

    # Flip every bit of the original semantic spans inside real prepared files.
    # This covers covered prose/heading bytes AND ignored status/check bytes.
    for name, data in (("approve", approved), ("reject", rejected)):
        start = data.index(b"# Plan")
        stop = data.index(b"\n<!-- coven-checkpoint-history")
        for at in range(start, stop):
            for bit in range(8):
                changed = data[:at] + bytes([data[at] ^ (1 << bit)]) + data[at + 1:]
                yield f"span-bit-{name}-{at-start}-{bit}", changed
        for old, new in ((b"Ship the plan.", b"Ship the plan!"),
                         (b"Status: " + (b"approved" if name == "approve" else b"changes_requested"), b"Status: pending"),
                         (b"Status: approved\n", b""),
                         (b"Status: changes_requested\n", b""),
                         (b"- [x] Approved", b"- [ ] Approved"),
                         (b"\n", b"\r\n")):
            yield f"presentation-{name}-{old.hex()}", data.replace(old, new)
        yield "contradiction-before-stale-" + name, data.replace(b"Ship the plan.", b"Different").replace(b"Status:", b"Status: wrong ")

    examples = [
        b"<!--\n## User decision\nStatus: hidden\n-->\n",
        b"inline <!-- hidden --> suffix\nStatus: pending\n",
        b"<!--\nreserved coven-checkpoint-any\n-->\n",
        b"inline coven-checkpoint-decision:v1\n",
        b"<!-- coven-checkpoint-decision:v99\n",
        b"<!-- coven-checkpoint-history:v1\n{}\n-->\n",
        b"```md\nStatus: example\n## User decision\n- [x] Approved\n```\n",
        b"~~~~\n```\n~~~\n~~~~~ \t\n",
        b"````\n```\n~~~~\n````\n",
        b"```\n```tail\n", b"~~~\n", b"<!-- unfinished\n",
        b"## User decision\n", b"Status: approved\nStatus: pending\n",
        b"\vStatus: pending\f\x1c\xc2\x85\xe2\x80\xa8\n",
    ]
    for i, example in enumerate(examples):
        for position in ("before", "after"):
            raw = example + TEMPLATE if position == "before" else TEMPLATE + example
            yield f"example-{i}-{position}", raw
            try:
                bound, _ = prepare(raw)
            except cp.CheckpointError:
                continue
            yield f"example-bound-{i}-{position}", bound
    for marker in (b"-", b"+", b"*", b"1.", b"12)", b"0001."):
        for indent in range(6):
            nested = b" " * indent + marker + b" List\n"
            width = indent + len(marker) + 1
            for padding in (b" " * (width - 1), b" " * width, b"\t", b"    "):
                raw = (nested + b"\n" + padding + b"Status: approved\n" + padding +
                       b"## User decision\n" + padding + b"- [x] Approved\n" +
                       padding + b"<!-- coven-checkpoint-any:v99\n" + TEMPLATE)
                yield f"list-{marker.hex()}-{indent}-{padding.hex()}", raw
    for char in (b"`", b"~"):
        for opening in (3, 4, 6):
            for closing in (2, 3, 4, 6, 7):
                for tail in (b"", b" \t", b"x", b"\v"):
                    raw = (char * opening + b"markdown\n<!-- coven-checkpoint-any\n" +
                           char * closing + tail + b"\n" + TEMPLATE)
                    yield f"fence-{char.hex()}-{opening}-{closing}-{tail.hex()}", raw
    for level in range(1, 9):
        for indent in range(5):
            for tail in (b"", b" ### \t", b"###", b"\t#"):
                raw = (b"Status: approved\n" + b" " * indent + b"#" * level +
                       b" User decision" + tail + b"\n- [x] Approved\n" +
                       b"# boundary\n- [x] Approved\n")
                yield f"heading-{level}-{indent}-{tail.hex()}", raw
    for line in (b"- [x] Approved", b"-\t[X]\tAPPROVED \t", b"+ [x] Approved",
                 b"- [x] Approved extra", b"- [x] Rejected / needs changes",
                 b"- [x] Rejected  / needs changes", b"- [x]Approved", b"- [x] Approved #"):
        for indent in range(5):
            yield f"checkbox-{indent}-{line.hex()}", b"## User decision\n" + b" " * indent + line + b"\n"

    history = b"\n" + cp._block("history", view.history[0])
    without = approved.replace(history, b"", 1)
    for separator in (b"", b"\n", b"\r", b"\r\n", b"\n\r"):
        yield "history-separator-" + separator.hex(), without + separator + history[1:]
    for at in (0, without.index(b"# Plan"), without.index(b"Purpose") + 3,
               without.index(b"Ship") + 2, len(without)):
        yield f"history-move-{at}", without[:at] + history + without[at:]
    for suffix in (b"prose", b"\n", b"\r", b"Status: pending\n", b"## End\n"):
        yield "history-appended-" + suffix.hex(), approved + suffix
    yield "duplicate-history", approved + history
    yield "duplicate-decision", cp._block("decision", view.decision) + approved
    yield "nonheader-decision", TEMPLATE + cp._block("decision", view.decision)
    yield "decision-id-equals-history-is-valid", approved
    for kind in ("decision", "history"):
        fields = dict(view.decision)
        if kind == "history":
            fields["notes"] = "rationale"
        invalids = {
            "version": (True, False, 0, 2, 1.0, "1", None, [], {}),
            "checkpoint": ("other", "../plan", "", "雪", 1, None),
            "decision_id": ("A" * 32, "a" * 31, "g" * 32, 1, None),
            "action": ("pending", [], {}, True, None),
            "scope_revision": (view.revision, "checkpoint-scope-v1:" + "A" * 64, None, 1),
            "reviewed_revision": (None, view.scope_revision, "checkpoint-missing-v1:other", "checkpoint-snapshot-v1:" + "a" * 63, False),
            "source": ("api", {}, [], 1, None),
            "decided_at": (None, 1, False, [], {}, "\ud800"),
        }
        if kind == "history":
            invalids["notes"] = (None, 1, [], "\udfff", "## User decision\n", "```\nunclosed", "raw -->", "coven-checkpoint-any")
        for field, variants in invalids.items():
            for i, value in enumerate(variants):
                yield f"record-{kind}-{field}-{i}", replace_record(approved, kind, lambda v, f=field, x=value: {**v, f: x})
        for field in fields:
            yield f"record-missing-{kind}-{field}", replace_record(approved, kind, lambda v, f=field: {k: x for k, x in v.items() if k != f})
        for value in (None, [], 1, "text", {**fields, "extra": True}):
            yield f"record-type-{kind}-{value!r}", replace_record(approved, kind, lambda _, v=value: v)
        for text in ("", "later", " \n\t", "雪\x00", "2026-not-a-date"):
            yield f"scalar-time-{kind}-{text!r}", replace_record(approved, kind, lambda v, t=text: {**v, "decided_at": t})
        payload = json.dumps(fields).encode()
        for encoding in ("utf-8-sig", "utf-16-be", "utf-16-le", "utf-32-be", "utf-32-le"):
            # BE payloads can be UTF-8 checkpoint bytes (ASCII with NULs), and
            # json.loads(bytes) recognizes their encoding independently.
            encoded = (json.dumps(fields) + "\n").encode(encoding)
            yield f"json-encoding-{kind}-{encoding}", replace_record(approved, kind, lambda _, p=encoded: p[:-1])
        for key in fields:
            duplicate = b'{"' + key.encode() + b'":null,' + payload[1:]
            yield f"duplicate-key-{kind}-{key}", replace_record(approved, kind, lambda _, p=duplicate: p)
        for bad in (b'{"extra":NaN,', b'{"extra":Infinity,', b'{"extra":-Infinity,',
                    b'{"extra":1e9999,', b'{"extra":"\\ud800",', b'{"extra":{"a":1,"a":2},'):
            yield f"json-bad-{kind}-{bad!r}", replace_record(approved, kind, lambda _, p=bad: p + payload[1:])
        for raw in (b"{", b"{} trailing", b"\xef\xbb\xbf" + payload,
                    payload + b" {}", payload.replace(b"1,", b"1.0,", 1),
                    payload.replace(b"1,", b"1e0,", 1)):
            yield f"json-syntax-{kind}-{raw.hex()}", replace_record(approved, kind, lambda _, p=raw: p)
        resources = (
            ("depth", b"[" * 65 + b"0" + b"]" * 65, "unsupported_json_depth_profile"),
            ("field-depth", b'{"extra":' + b"[" * 65 + b"0" + b"]" * 65 + b"," + payload[1:], "unsupported_json_depth_profile"),
            ("integer", b'{"version":' + b"1" * 4301 + b"," + payload.split(b",", 1)[1], "unsupported_integer_profile"),
            ("number-token", b"0." + b"0" * 8192, "unsupported_number_token_profile"),
            ("array", b"[" + b"0," * 65536 + b"0]", "unsupported_json_array_profile"),
            ("object", b"{" + b",".join(f'"{i}":0'.encode() for i in range(16385)) + b"}", "unsupported_json_object_profile"),
        )
        for name, raw, error in resources:
            data = replace_record(approved, kind, lambda _, p=raw: p)
            assert len(data) < 1_048_576
            yield f"resource-{kind}-{name}", data, error
        marker = f"<!-- coven-checkpoint-{kind}:v1".encode()
        for changed in (marker + b" ", marker[:-1] + b"2", marker.upper(), b" " + marker):
            yield f"marker-{kind}-{changed.hex()}", approved.replace(marker, changed)
        for closing in (b"--x", b" -->", b"--> suffix"):
            yield f"closing-{kind}-{closing.hex()}", approved.replace(b"\n-->\n", b"\n" + closing + b"\n")
    for notes in ("雪 café <tag>\n```md\n## User decision\n```\n- Prior\n  - [x] Approved",
                  "<!-- hidden\n## User decision\n", "- List\n  ## User decision\n"):
        yield "notes-" + repr(notes), prepare(TEMPLATE, notes=notes)[0]
    for raw in (b"\xff", b"\xc0\xaf", b"\xed\xa0\x80", b"\xf4\x90\x80\x80", b"Status: \xe2\x82"):
        yield "utf8-" + raw.hex(), raw


def fixtures():
    for case in corpus():
        name, raw = case[:2]
        result = dict(id=name, checkpoint="plan", rawbyteshex=None if raw is None else raw.hex())
        if len(case) == 3:
            result["expectednative_error"] = case[2]
        try:
            result.update(accepted=True, expected=cp.inspect("plan", raw).to_dict())
        except cp.CheckpointError:
            result.update(accepted=False)
        yield result
    for checkpoint in ("other", "_-", "0", "", "../plan", "雪", "a b"):
        for raw in (None, b"", TEMPLATE):
            result = dict(id=f"identifier-{checkpoint!r}-{raw!r}", checkpoint=checkpoint,
                          rawbyteshex=None if raw is None else raw.hex())
            try:
                result.update(accepted=True, expected=cp.inspect(checkpoint, raw).to_dict())
            except cp.CheckpointError:
                result.update(accepted=False)
            yield result


if __name__ == "__main__":
    counts = {"accepted": 0, "rejected": 0, "resource_refusals": 0}
    for fixture in fixtures():
        counts["accepted" if fixture["accepted"] else "rejected"] += 1
        counts["resource_refusals"] += "expectednative_error" in fixture
        if "--summary" not in sys.argv:
            print(json.dumps(fixture, ensure_ascii=True, separators=(",", ":")))
    if "--summary" in sys.argv:
        print(json.dumps(counts, sort_keys=True))
