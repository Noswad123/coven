# Pure filtered session views

`runner/session_views.py` provides two pure functions:

```python
validate_selection(*, group_key=None, member_key=None, recognized_only=False)
select_report(report, *, group_key=None, member_key=None, recognized_only=False)
```

Validation returns `None` or raises `ValueError`. Group/member selectors use the
existing `[a-z0-9][a-z0-9_-]{0,63}` human-selector grammar; member requires group.
`recognized_only` must be a literal bool. The coordinator must validate before
performing API observation; selection also validates defensively.

The input is one original, validated `coven.session-inspection.v1` report from an
entire bounded, fenced observation, not an already filtered view. This module
does no report decoding, observation, I/O, clock reads, caching or native work.
It returns a deep copy, including when no selectors are active; in that case the
output equals the input exactly and no metadata is added.

Active selection adds `view` with `group_key`, `member_key`, `recognized_only`,
`observed_target_count`, `observed_recognized_target_count`, and
`declaration_count_scope: full_observation`. Observed counts are the original
report's counts (null for failed observation), not native catalog counts.

On observed reports, group/member selectors retain only targets with matching
explicit associations and only those associations within retained targets.
They also filter `declarations.unmatched`; binding qualifiers, historical native
references and declared live-attachment booleans remain unchanged. Matching a
selector never revalidates a changed or legacy declaration. Recognition selection
retains only `recognition == djinn_executable_family`, including unassociated
targets when no declaration selector is given. It intersects with group/member
selection, but never filters unmatched declarations by inferred recognition.
Target/association order is preserved, and selected target counts are recomputed.

Aggregate `other_endpoint_count` and `outside_inspection_count` remain counts for
the full observation, even with declaration selectors; the view scope marker makes
this explicit. Scope/request, revisions, diagnostics, receipt timestamps,
freshness, limitations, authorization and all native qualifiers are preserved.
Empty selection is an observed view, not native absence or catalog completeness.

Unavailable/incompatible reports retain their failure assessment, empty targets,
null counts and declaration-failure semantics. Only requested view metadata is
added. No prior success is retained or substituted. Selection grants no permission,
activity, readiness, identity, lifecycle, worker acceptance or completion claim.

Focused offline validation:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p test_session_views.py -v
```

These tests use in-memory fixtures, not live APIs, native sessions or models.
CLI integration, scanner bounds, collection and watch behavior are outside this
module's scope.
