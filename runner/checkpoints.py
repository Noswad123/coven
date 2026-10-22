"""Pure checkpoint decisions over exact UTF-8 Markdown bytes.

Bindings are not authentication: edit-and-revert restores a scope hash, and
linked external targets are not covered. Only this small Markdown dialect is
structural; unknown prose, nested examples, and code remain covered bytes.
"""

from dataclasses import asdict, dataclass
import hashlib
import json
import re

if __package__:
    from . import records, storage
else:
    import records
    import storage


class CheckpointError(storage.StorageError):
    """A checkpoint cannot be interpreted or edited safely."""


@dataclass
class View:
    checkpoint: str
    revision: str
    scope_revision: str | None
    declared_status: str
    effective_status: str
    binding: str
    decision: dict | None
    reason: str | None
    content: str | None
    history: list

    def to_dict(self) -> dict:
        return asdict(self)


def _fail(message):
    raise CheckpointError("Cannot edit checkpoint: " + message)


def _identifier(checkpoint):
    try:
        return records.identifier(checkpoint, "checkpoint", "checkpoint")
    except ValueError as exc:
        _fail(str(exc))


def validate_revision(token: str | None):
    """Validate an optional expected snapshot, never a scope token."""
    if token is not None and (not isinstance(token, str) or not re.fullmatch(
            r"checkpoint-(?:snapshot-v1:[0-9a-f]{64}|missing-v1:[A-Za-z0-9_-]+)", token)):
        _fail("invalid expected checkpoint revision")
    return token


def _hash(kind, checkpoint, data):
    # Frame the concatenated covered bytes once, not each exclusion segment.
    digest = hashlib.sha256()
    for part in (f"checkpoint-{kind}-v1".encode(), checkpoint.encode(), data):
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return f"checkpoint-{kind}-v1:" + digest.hexdigest()


def _heading(line):
    match = re.fullmatch(rb" {0,3}(#{1,6})[ \t]+(.+?)[ \t]*", line)
    if match:
        return len(match[1]), re.sub(rb"[ \t]+#+[ \t]*$", b"", match[2]).lower()
    return None


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            _fail("duplicate JSON key")
        result[key] = value
    return result


_FIELDS = {"version", "checkpoint", "decision_id", "action", "scope_revision",
           "reviewed_revision", "decided_at", "source"}


def _record(value, checkpoint, history=False):
    if not isinstance(value, dict) or set(value) != _FIELDS | ({"notes"} if history else set()):
        _fail("invalid decision record fields")
    if type(value["version"]) is not int or value["version"] != 1:
        _fail("unsupported decision version")
    _identifier(value["checkpoint"])
    if value["checkpoint"] != checkpoint:
        _fail("decision belongs to another checkpoint")
    if not isinstance(value["decision_id"], str) or not re.fullmatch(r"[0-9a-f]{32}", value["decision_id"]):
        _fail("invalid decision identity")
    if value["action"] not in ("approved", "changes_requested"):
        _fail("invalid decision action")
    if not isinstance(value["scope_revision"], str) or not re.fullmatch(
            r"checkpoint-scope-v1:[0-9a-f]{64}", value["scope_revision"]):
        _fail("invalid decision scope")
    validate_revision(value["reviewed_revision"])
    if value["reviewed_revision"] is None:
        _fail("missing reviewed revision")
    if value["reviewed_revision"].startswith("checkpoint-missing-v1:") and value["reviewed_revision"] != "checkpoint-missing-v1:" + checkpoint:
        _fail("reviewed missing revision belongs to another checkpoint")
    if value["source"] not in ("cli", "review", "console"):
        _fail("invalid decision source")
    try:
        records.text(value["decided_at"], "decided_at")
        if history:
            _notes(value["notes"])
    except ValueError as exc:
        _fail(str(exc))
    return value


def _scan(data, checkpoint):
    """Return visible lines and validated machine spans, all in byte offsets."""
    lines = data.splitlines(keepends=True)
    visible, excluded, history, history_spans = [], [], [], []
    decision = None
    fence = None
    comment = False
    list_indent = None
    offset = 0
    index = 0
    first_h2 = False
    while index < len(lines):
        raw = lines[index]
        line = raw.rstrip(b"\r\n")
        end = offset + len(raw)
        marker = re.fullmatch(rb" {0,3}(`{3,}|~{3,})(.*)", line)
        indentation = len(line) - len(line.lstrip(b" "))
        nested = list_indent is not None and bool(line.strip()) and (
            line.startswith(b"\t") or indentation >= list_indent)
        if not fence and not comment and line.strip() and not nested:
            item = re.match(rb" {0,3}(?:[-+*]|[0-9]+[.)])[ \t]+", line)
            list_indent = len(item[0]) if item else None
        if fence:
            if marker and marker[1][:1] == fence[0] and len(marker[1]) >= fence[1] and not marker[2].strip():
                fence = None
        elif nested or line.startswith((b"    ", b"\t")):
            pass
        elif marker and not comment:
            fence = marker[1][:1], len(marker[1])
        elif b"coven-checkpoint-" in line:
            match = re.fullmatch(rb"<!-- coven-checkpoint-(decision|history):v1", line)
            if comment or not match or index + 2 >= len(lines):
                _fail("malformed or unsupported reserved machinery")
            kind = match[1]
            payload, closing = lines[index + 1:index + 3]
            if closing.rstrip(b"\r\n") != b"-->" or b"-->" in payload or b"<!--" in payload:
                _fail("malformed reserved block")
            try:
                value = json.loads(payload, object_pairs_hook=_pairs,
                                   parse_constant=lambda value: _fail("nonfinite JSON value"))
            except (ValueError, UnicodeError) as exc:
                _fail("invalid decision JSON: " + str(exc))
            _record(value, checkpoint, kind == b"history")
            stop = end + len(payload) + len(closing)
            start = offset
            if kind == b"decision":
                if decision is not None or first_h2:
                    _fail("duplicate or non-header decision block")
                decision = value
            else:
                if offset == 0 or data[offset - 1:offset] != b"\n":
                    _fail("history block lacks its separator newline")
                start -= 1
                if any(item["decision_id"] == value["decision_id"] for item in history):
                    _fail("duplicate history identity")
                history.append(value)
                history_spans.append((start, stop))
            excluded.append((start, stop))
            index += 3
            offset = stop
            continue
        elif comment:
            if b"-->" in line:
                comment = False
        elif b"<!--" in line:
            comment = b"-->" not in line.split(b"<!--", 1)[1]
        else:
            visible.append((offset, end, line))
            if (_heading(line) or (None,))[0] == 2:
                first_h2 = True
        offset = end
        index += 1
    if fence:
        _fail("unterminated fenced code block")
    return visible, excluded, decision, history, history_spans


def _notes(notes):
    try:
        records.text(notes, "notes")
    except ValueError as exc:
        _fail(str(exc))
    if "coven-checkpoint-" in notes or "-->" in notes:
        _fail("reserved machinery in notes")
    visible, _, _, _, _ = _scan(notes.encode("utf-8"), "notes")
    if any(_heading(line) == (2, b"user decision") for _, _, line in visible):
        _fail("User decision section in notes")


def _parse(checkpoint, data):
    try:
        content = data.decode("utf-8")
    except UnicodeError as exc:
        _fail("checkpoint is not UTF-8: " + str(exc))
    visible, excluded, decision, history, history_spans = _scan(data, checkpoint)
    header_end = next((start for start, _, line in visible if (_heading(line) or (None,))[0] == 2), len(data))
    statuses = [(start, end, line) for start, end, line in visible
                if start < header_end and re.match(rb"^ {0,3}status:", line, re.I)]
    sections = [(start, end) for start, end, line in visible if _heading(line) == (2, b"user decision")]
    if len(statuses) > 1 or len(sections) > 1:
        _fail("ambiguous current status or User decision sections")
    checks = []
    if sections:
        section_start = sections[0][1]
        section_end = next((start for start, _, line in visible if start >= section_start and _heading(line)), len(data))
        for label in (b"Approved", b"Rejected / needs changes"):
            matches = []
            for start, end, line in visible:
                match = re.fullmatch(rb" {0,3}-[ \t]+\[([ xX])\][ \t]+" + re.escape(label) + rb"[ \t]*", line, re.I)
                if section_start <= start < section_end and match:
                    matches.append((start, end, line, label, match[1].lower() == b"x"))
            if len(matches) > 1:
                _fail("ambiguous current decision checkbox")
            checks.extend(matches)
    excluded.extend((start, end) for start, end, _ in statuses)
    excluded.extend((start, end) for start, end, *_ in checks)
    # Only a machine-history TRAILER is exempt. Moving a block into reviewed
    # prose/headings changes Markdown structure even if removing it would leave
    # the same concatenated text. Internal history (and any new covered prose
    # appended after old history) is therefore conservatively covered in full.
    cursor, last_covered = 0, 0
    for start, end in sorted(excluded):
        if start > cursor:
            last_covered = start
        cursor = max(cursor, end)
    if cursor < len(data):
        last_covered = len(data)
    internal_history = {span for span in history_spans if span[1] <= last_covered}
    excluded = [span for span in excluded if span not in internal_history]
    remaining = []
    cursor = 0
    for start, end in sorted(excluded):
        remaining.append(data[cursor:max(cursor, start)])
        cursor = max(cursor, end)
    remaining.append(data[cursor:])
    scope = _hash("scope", checkpoint, b"".join(remaining))
    declared = statuses[0][2].split(b":", 1)[1].decode().strip().lower() or "unknown" if statuses else "unknown"
    binding, effective, reason = "none", declared, None
    if decision:
        binding = "bound"
        action = decision["action"]
        if declared != action or any(checked != ((label == b"Approved") == (action == "approved")) for _, _, _, label, checked in checks):
            binding, effective, reason = "inconsistent", "decision_invalid", "Current presentation contradicts the bound decision"
        elif scope != decision["scope_revision"]:
            effective = "approval_stale" if action == "approved" else action
            reason = "Covered checkpoint bytes differ from the decision scope"
        else:
            effective = action
    elif declared == "approved":
        binding, effective = "legacy_unbound", "approved_unbound"
    view = View(checkpoint, _hash("snapshot", checkpoint, data), scope, declared,
                effective, binding, decision, reason, content, history)
    return view, statuses, checks, excluded


def inspect(checkpoint: str, data: bytes | None) -> View:
    _identifier(checkpoint)
    if data is None:
        return View(checkpoint, "checkpoint-missing-v1:" + checkpoint, None,
                    "missing", "missing", "none", None, None, None, [])
    return _parse(checkpoint, data)[0]


def _block(kind, value):
    payload = json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    payload = payload.replace("<", "\\u003c").replace(">", "\\u003e")
    return (f"<!-- coven-checkpoint-{kind}:v1\n" + payload + "\n-->\n").encode("ascii")


def prepare(checkpoint, data, action, notes, *, decision_id: str,
            decided_at: str, source: str, initial_content: bytes | None = None):
    before = inspect(checkpoint, data)
    _notes(notes)
    if data is None:
        if initial_content is None:
            _fail("missing checkpoint requires initial_content")
        original = initial_content
    else:
        original = data
    base, statuses, checks, _ = _parse(checkpoint, original)
    decision = dict(version=1, checkpoint=checkpoint, decision_id=decision_id,
                    action=action, scope_revision=base.scope_revision,
                    reviewed_revision=before.revision, decided_at=decided_at, source=source)
    _record(decision, checkpoint)
    if any(item["decision_id"] == decision_id for item in base.history):
        _fail("duplicate history identity")
    replacements = []
    if base.decision:
        # Find the already validated live metadata span, not a fenced example.
        _, spans, _, _, _ = _scan(original, checkpoint)
        start, end = next((start, end) for start, end in spans
                          if original[start:end].startswith(b"<!-- coven-checkpoint-decision:v1"))
        replacements.append((start, end, b""))
    for start, end, line in statuses:
        ending = original[start + len(line):end]
        replacements.append((start, end, b"Status: " + action.encode() + ending))
    for start, end, line, label, _ in checks:
        checked = (label == b"Approved") == (action == "approved")
        # Change only the checkbox character, preserving all other formatting.
        changed = re.sub(rb"\[[ xX]\]", b"[x]" if checked else b"[ ]", line, count=1)
        replacements.append((start, end, changed + original[start + len(line):end]))
    result = original
    for start, end, replacement in sorted(replacements, reverse=True):
        result = result[:start] + replacement + result[end:]
    prefix = _block("decision", decision)
    if not statuses:
        prefix += b"Status: " + action.encode() + b"\n"
    result = prefix + result + b"\n" + _block("history", {**decision, "notes": notes})
    final = inspect(checkpoint, result)
    if final.scope_revision != base.scope_revision or (data is not None and final.scope_revision != before.scope_revision):
        _fail("decision would change covered checkpoint bytes")
    if final.decision != decision or final.effective_status != action:
        _fail("prepared decision does not match requested action")
    return result, final
