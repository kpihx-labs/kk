"""The guest-agent boot gate must match Guest.agent's real error contract."""
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location(
    "kk_control_scenario", Path(__file__).resolve().parents[1] / "scripts/08-control-test.py"
)
CONTROL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTROL)
ROSTER = [{"name": "disposable-guest", "ip": "192.0.2.10"}]
READY = [{"ip-addresses": [{"ip-address": "192.0.2.10"}]}]


class PowerGateTests(unittest.TestCase):
    def test_wrapped_agent_startup_failure_is_retried_until_connected(self):
        guest = Mock()
        guest.agent.side_effect = [RuntimeError("virtio not connected yet"), {}]
        guest.interfaces.return_value = READY
        with (
            patch.object(CONTROL, "declared_peers", return_value=ROSTER),
            patch.object(CONTROL, "Guest", return_value=guest),
            patch.object(CONTROL.subprocess, "run", return_value=Mock(stdout="running\n")),
            patch.object(CONTROL.time, "sleep"),
        ):
            CONTROL.power("disposable-guest", "start")
        self.assertEqual(guest.agent.call_count, 2)
        guest.configure_isolated_network.assert_called_once_with(ROSTER[0])

    def test_disconnected_agent_still_has_a_bounded_startup_deadline(self):
        guest = Mock()
        guest.agent.side_effect = RuntimeError("virtio never connected")
        with (
            patch.object(CONTROL, "declared_peers", return_value=ROSTER),
            patch.object(CONTROL, "Guest", return_value=guest),
            patch.object(CONTROL.subprocess, "run", return_value=Mock(stdout="running\n")),
            patch.object(CONTROL.time, "monotonic", side_effect=[0, 0, 46]),
            patch.object(CONTROL.time, "sleep"),
        ):
            with self.assertRaises(TimeoutError):
                CONTROL.power("disposable-guest", "start")

    def test_agent_ready_before_network_does_not_close_boot_gate(self):
        guest = Mock()
        guest.interfaces.side_effect = [[], [{"ip-addresses": [{"ip-address": "127.0.0.1"}]}], READY]
        with (
            patch.object(CONTROL, "declared_peers", return_value=ROSTER),
            patch.object(CONTROL, "Guest", return_value=guest),
            patch.object(CONTROL.subprocess, "run", return_value=Mock(stdout="running\n")),
            patch.object(CONTROL.time, "sleep"),
        ):
            CONTROL.power("disposable-guest", "start")
        self.assertEqual(guest.interfaces.call_count, 3)

    def test_teardown_attempts_every_process_and_retains_all_errors(self):
        stop = Mock(side_effect=[RuntimeError("first"), None, RuntimeError("third")])
        errors = CONTROL.cleanup({0: 100, 1: 101, 2: 102}, stop)
        self.assertEqual([str(error) for error in errors], ["first", "third"])
        self.assertEqual([call.args[0] for call in stop.call_args_list], [0, 1, 2])

    def test_connected_agent_without_network_still_has_a_deadline(self):
        guest = Mock()
        guest.interfaces.return_value = []
        with (
            patch.object(CONTROL, "declared_peers", return_value=ROSTER),
            patch.object(CONTROL, "Guest", return_value=guest),
            patch.object(CONTROL.subprocess, "run", return_value=Mock(stdout="running\n")),
            patch.object(CONTROL.time, "monotonic", side_effect=[0, 0, 46]),
            patch.object(CONTROL.time, "sleep"),
        ):
            with self.assertRaisesRegex(TimeoutError, "declared network address"):
                CONTROL.power("disposable-guest", "start")
