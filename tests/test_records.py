"""Pure v1 record contracts preserve valid legacy data and extension fields."""

import unittest

from runner import records


class RecordTests(unittest.TestCase):
    def test_task_schema_rejects_bad_shapes_and_known_fields(self):
        valid = {"id": "T001", "title": "Legacy task", "status": "custom wait", "owner": None}
        invalid = [None, {}, [None], [{}], [valid, valid]]
        for field, value in (("id", "../escape"), ("title", []), ("status", False),
                             ("owner", 3), ("notes", {}), ("created_at", False), ("updated_at", None)):
            invalid.append([{**valid, field: value}])
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(ValueError):
                records.tasks(value)

    def test_agent_and_lead_schemas_reject_bad_shapes(self):
        agent = {"id": "_", "status": "unknown", "role": "Legacy role"}
        for value in (None, {}, [False], [{}], [agent, agent], [{**agent, "role": None}], [{**agent, "status": 5}]):
            with self.subTest(value=value), self.assertRaises(ValueError):
                records.agents(value)
        for value in ({}, [], False, "", 0, {"agent": None}, {"agent": "../bad"},
                      {"agent": "Jamal", "set_at": 3}, {"agent": "_", "until": []}, {"agent": "_", "instructions": {}}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                records.lead(value)
        self.assertIsNone(records.lead(None))

    def test_metadata_contract_rejects_unsupported_versions_runtime_and_labels(self):
        valid = {"name": "Legacy", "agents": ["Jamal", "_"]}
        invalid = [None, [], {"name": "Legacy"}]
        for field, value in (("name", "!!!"), ("name", []), ("version", True), ("version", 2),
                             ("version", "1"), ("agents", []), ("agents", ["Jamal", "jamal"]),
                             ("agents", ["agent", "-agent-"]), ("multiplexer", "unsupported"),
                             ("opencode_command", " "), ("created_at", 2), ("source_of_truth", []),
                             ("example", False), ("display_names", []), ("display_names", {"../bad": "Name"}),
                             ("display_names", {"Jamal": None}), ("display_names", {"Jamal": " "}),
                             ("display_names", {"Jamal": chr(0xD800)})):
            invalid.append({**valid, field: value})
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(ValueError):
                records.team(value)

    def test_valid_legacy_records_and_unknown_fields_are_not_normalized(self):
        extension = {"nested": ["custom", {"key": True}]}
        values = (
            (records.team, {"name": "Legacy", "agents": ["Jamal", "_"], "extension": extension,
                            "display_names": {"Jamal": "Shared name", "_": "Shared name", "retired": "Old teammate"}}),
            (records.tasks, [{"id": "T001", "title": "", "status": "custom wait", "owner": "Retired",
                              "created_at": "old", "extension": extension}]),
            (records.agents, [{"id": "retired", "role": "", "status": "custom status", "extension": extension}]),
            (records.lead, {"agent": "_", "set_at": "old", "extension": extension}),
        )
        for validator, value in values:
            with self.subTest(validator=validator.__name__):
                self.assertIs(validator(value), value)
