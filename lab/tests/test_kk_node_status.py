"""Machine status is explicit; a successful offline observation is not readiness."""

import ast
import inspect
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
with mock.patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
    import kk_node_cases as NODES


class NodeStatusTests(unittest.TestCase):
    def test_online_observation_extracts_the_actual_node_status(self):
        status = {"received_operations": 2, "sent_operations": 1}
        reply = {"code": 0, "out": json.dumps({"profile": {}, "node": {"state": "online", "status": status}})}
        self.assertEqual(NODES.observed_node_status(reply), status)

    def test_successful_offline_unavailable_and_error_observations_are_not_ready(self):
        for state in ("offline", "unavailable", "error"):
            with self.subTest(state=state):
                reply = {"code": 0, "out": json.dumps({"node": {"state": state}})}
                self.assertIsNone(NODES.observed_node_status(reply))

    def test_nonzero_exit_is_not_parsed_as_a_successful_observation(self):
        self.assertIsNone(NODES.observed_node_status({"code": 1, "out": "not JSON"}))

    def test_human_legacy_malformed_and_unknown_shapes_are_not_silently_adopted(self):
        for value in ("human output", "[]", "{}", '{"received_operations": 2}',
                      '{"node": {"state": "online"}}', '{"node": {"state": "unknown"}}'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                NODES.observed_node_status({"code": 0, "out": value})

    def test_every_automated_status_call_explicitly_requests_json(self):
        calls = [node for node in ast.walk(ast.parse(inspect.getsource(NODES.node_cases)))
                 if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                 and node.func.id == "call" and len(node.args) > 1
                 and isinstance(node.args[1], ast.Constant) and node.args[1].value == "status"]
        self.assertEqual(len(calls), 2)
        for call in calls:
            self.assertGreater(len(call.args), 2, "automated status must request --json")
            self.assertEqual(call.args[2].value, "--json")


if __name__ == "__main__":
    unittest.main()
