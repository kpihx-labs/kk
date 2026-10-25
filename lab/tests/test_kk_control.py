import base64
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import kk_native
from kk_guest import Guest


class GuestControlTests(unittest.TestCase):
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
