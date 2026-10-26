import base64
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import kk_native
import kk_guest
from kk_guest import Guest


class GuestControlTests(unittest.TestCase):
    def test_room_invitation_export_rejects_arbitrary_or_key_paths_before_guest_calls(self):
        guest = Guest("fixture")
        guest.run = Mock()
        for source in ["/var/tmp/private-key.pem", "/var/tmp/kk-room-012345abcdef/key.pem",
                       "/var/tmp/kk-room-012345abcdef/../scoped-invitation.json"]:
            with self.assertRaises(ValueError):
                guest.room_invitation(source)
        guest.run.assert_not_called()

    def test_room_invitation_export_is_bounded_and_refuses_links(self):
        guest = Guest("fixture")
        path = "/var/tmp/kk-room-012345abcdef/scoped-invitation.json"
        guest.run = Mock(side_effect=[{"code": 1}, {"code": 0, "out": "opaque ephemeral fixture"}])
        self.assertEqual(guest.room_invitation(path), b"opaque ephemeral fixture")
        self.assertEqual(guest.run.call_args.args, ("/usr/bin/dd", "if=" + path, "bs=4097", "count=1", "status=none"))
        guest.run = Mock(return_value={"code": 0})
        with self.assertRaisesRegex(ValueError, "symlinks"):
            guest.room_invitation(path)
        guest.run = Mock(side_effect=[{"code": 1}, {"code": 0, "out": "x" * 4097}])
        with self.assertRaisesRegex(ValueError, "exceeds"):
            guest.room_invitation(path)
        guest.run = Mock(side_effect=[{"code": 1}, {"code": 1, "out": "must-not-be-logged", "err": "must-not-be-logged"}])
        with self.assertRaisesRegex(RuntimeError, r"^Private invitation transfer failed \(exit 1\)$"):
            guest.room_invitation(path)

    def test_fedora_uses_the_labelled_management_entrypoint(self):
        guest = Guest("fedora-44")
        guest.agent = Mock(return_value={"pid": 42})
        self.assertEqual(guest.start("/usr/bin/id"), 42)
        guest.agent.assert_called_once_with(
            "guest-exec", path="/usr/local/libexec/kk-lab-control", arg=["/usr/bin/id"],
            **{"capture-output": True},
        )

    def test_mint_preserves_empty_arguments_without_shell_command_evaluation(self):
        guest = Guest("mint-22.3")
        guest.agent = Mock(return_value={"pid": 7})
        guest.start("/usr/local/bin/kk", "send", "peer", "")
        self.assertEqual(guest.agent.call_args.kwargs["path"], "/usr/local/libexec/kk-lab-control")
        self.assertEqual(guest.agent.call_args.kwargs["arg"][-1], "")

    def test_binary_input_is_in_the_rpc_pipe_not_a_shell_argument(self):
        guest = Guest("mint-22.3")
        guest.agent = Mock(return_value={"pid": 7})
        payload = b"\x00\xff\n"
        guest.start("dd", "status=none", input_data=payload)
        self.assertEqual(base64.b64decode(guest.agent.call_args.kwargs["input-data"]), payload)

    def test_upload_chunks_are_bounded_and_complete(self):
        guest = Guest("mint-22.3")
        guest.run = Mock()
        guest.start = Mock(return_value=1)
        guest.wait = Mock(return_value={"code": 0})
        payload = b"x" * 96_001
        guest.upload("/var/tmp/public-fixture", payload)
        chunks = [call.kwargs["input_data"] for call in guest.start.call_args_list]
        self.assertEqual([len(chunk) for chunk in chunks], [48_000, 48_000, 1])
        self.assertEqual(b"".join(chunks), payload)

    def test_failed_guest_write_cannot_be_reported_as_success(self):
        guest = Guest("mint-22.3")
        guest.run = Mock()
        guest.start = Mock(return_value=1)
        guest.wait = Mock(return_value={"code": 1, "err": "disk full"})
        with self.assertRaises(OSError):
            guest.upload("/var/tmp/public-fixture", b"x")

    def test_isolation_restores_address_and_checks_both_default_routes(self):
        guest = Guest("fixture")
        peer = {"name": "fixture", "mac": "52:54:00:00:00:01", "ip": "192.0.2.10"}
        guest.interfaces = Mock(return_value=[{"name": "eth0", "hardware-address": peer["mac"]}])
        guest.run = Mock(return_value={"out": "[]"})
        guest.configure_isolated_network(peer)
        calls = [call.args for call in guest.run.call_args_list]
        self.assertEqual(calls[0], ("systemctl", "start", "NetworkManager"))
        self.assertEqual(calls[1], ("/usr/bin/nmcli", "device", "set", "eth0", "managed", "no"))
        self.assertIn(("ip", "address", "replace", "192.0.2.10/24", "dev", "eth0"), calls)
        for family in ["-4", "-6"]:
            self.assertIn(("ip", family, "-j", "route", "show", "default"), calls)
        self.assertEqual(peer["device"], "eth0")

    def test_isolation_refuses_a_remaining_internet_route(self):
        guest = Guest("fixture")
        guest.interfaces = Mock(return_value=[{"name": "eth0", "hardware-address": "fixture-mac"}])
        guest.run = Mock(return_value={"out": '[{"dst":"default"}]'})
        with self.assertRaisesRegex(RuntimeError, "default Internet route remains"):
            guest.configure_isolated_network({"name": "fixture", "mac": "fixture-mac", "ip": "192.0.2.10"})

    def test_isolation_refuses_an_unexpected_guest_before_network_changes(self):
        guest = Guest("fixture")
        guest.run = Mock()
        with self.assertRaises(ValueError):
            guest.configure_isolated_network({"name": "other"})
        guest.run.assert_not_called()

    def test_network_service_failure_does_not_continue_into_address_mutation(self):
        guest = Guest("fixture")
        guest.run = Mock(side_effect=RuntimeError("NetworkManager failed to start"))
        guest.interfaces = Mock()
        with self.assertRaisesRegex(RuntimeError, "failed to start"):
            guest.configure_isolated_network({"name": "fixture", "mac": "fixture-mac", "ip": "192.0.2.10"})
        guest.run.assert_called_once_with("systemctl", "start", "NetworkManager")
        guest.interfaces.assert_not_called()

    def test_shutdown_rpc_disconnect_still_requires_confirmed_domain_stop(self):
        command = Mock(side_effect=["running", RuntimeError("agent disconnected"), "running", "shut off"])
        with (
            patch.object(kk_guest, "virsh", command),
            patch.object(kk_guest.time, "sleep"),
            patch("builtins.print") as output,
        ):
            kk_guest.shutdown_guests(["fixture"])
        self.assertIn("confirmed domain shut-off", output.call_args.args[0])
        self.assertEqual(command.call_args.args, ("domstate", "fixture"))

    def test_shutdown_rpc_failure_does_not_prevent_other_guest_requests(self):
        command = Mock(side_effect=["running", RuntimeError("agent disconnected"), "running", "requested", "shut off", "shut off"])
        with patch.object(kk_guest, "virsh", command), patch("builtins.print"):
            kk_guest.shutdown_guests(["first", "second"])
        self.assertIn(("shutdown", "second", "--mode", "agent"), [call.args for call in command.call_args_list])

    def test_shutdown_failure_cannot_claim_a_running_domain_stopped(self):
        command = Mock(side_effect=["running", RuntimeError("agent disconnected"), "running"])
        with (
            patch.object(kk_guest, "virsh", command),
            patch.object(kk_guest.time, "monotonic", side_effect=[0, 61]),
        ):
            with self.assertRaisesRegex(TimeoutError, "agent disconnected"):
                kk_guest.shutdown_guests(["fixture"])

    def test_native_cli_environment_keeps_trash_safe_path_and_is_scoped(self):
        original = str(Path(sys.executable).parent.resolve())
        safe_bin = str(Path.home() / ".local/bin")
        with (
            tempfile.TemporaryDirectory() as temporary,
            patch.object(kk_native, "ROOT", Path(temporary)),
            patch.dict(os.environ, {"PATH": os.pathsep.join([original, safe_bin, "/usr/bin"])}),
        ):
            result = kk_native.environment()
            self.assertEqual(result["PATH"], os.pathsep.join([safe_bin, "/usr/bin"]))
            self.assertIn(original, os.environ["PATH"])
            self.assertTrue(Path(result["LIBGUESTFS_CACHEDIR"]).is_dir())


if __name__ == "__main__":
    unittest.main()
