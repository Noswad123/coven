"""Pure post-observation views of validated session-inspection.v1 reports.

Selection never narrows scanner bounds, revalidates declarations, or establishes
native activity, identity, readiness, absence, completeness or permissions.
"""

from copy import deepcopy
import re


def validate_selection(*, group_key=None, member_key=None, recognized_only=False):
    """Validate selectors without I/O; callers must do this before observation."""
    for name, value in (("group_key", group_key), ("member_key", member_key)):
        if value is not None and not (
            isinstance(value, str) and re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,63}", value)
        ):
            raise ValueError(name + " must be a lowercase human selector of 1..64 characters")
    if member_key is not None and group_key is None:
        raise ValueError("member_key requires group_key")
    if type(recognized_only) is not bool:
        raise ValueError("recognized_only must be a literal bool")


def select_report(report, *, group_key=None, member_key=None, recognized_only=False):
    """Return an independent view of one validated, fully fenced inspection report.

    The input is the original inspection report, not a previously selected view.
    Validation of that report and bounded observation remain the caller's job.
    Unavailable/incompatible reports are copied without changing failure semantics.
    """
    validate_selection(group_key=group_key, member_key=member_key,
                       recognized_only=recognized_only)
    result = deepcopy(report)
    if group_key is None and not recognized_only:
        return result

    result["view"] = {
        "group_key": group_key,
        "member_key": member_key,
        "recognized_only": recognized_only,
        "observed_target_count": report["target_count"],
        "observed_recognized_target_count": report["recognized_target_count"],
        "declaration_count_scope": "full_observation",
    }
    if report["assessment"] != "observed":
        return result

    def matches(association):
        return (association["group_key"] == group_key
                and (member_key is None or association["member_key"] == member_key))

    targets = []
    for target in result["targets"]:
        if recognized_only and target["recognition"] != "djinn_executable_family":
            continue
        if group_key is not None:
            target["associations"] = [a for a in target["associations"] if matches(a)]
            if not target["associations"]:
                continue
        targets.append(target)
    result["targets"] = targets
    result["target_count"] = len(targets)
    result["recognized_target_count"] = sum(
        target["recognition"] == "djinn_executable_family" for target in targets
    )
    if group_key is not None:
        result["declarations"]["unmatched"] = [
            a for a in result["declarations"]["unmatched"] if matches(a)
        ]
    return result
