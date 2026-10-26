"""Private VM adapter regression; no guest/network effects."""
import importlib.util
import json
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location("kk_private_room_lab", Path(__file__).resolve().parents[1] / "scripts/09-room-test.py")
ROOM = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ROOM)


class AtomicCommandPublication(unittest.TestCase):
    def test_only_finished_staging_input_is_renamed_into_the_request_namespace(self):
        guest = Mock()
        with patch.object(ROOM.CONTROL, "upload") as upload, patch.object(ROOM.uuid, "uuid4", return_value=Mock(hex="abc")):
            result = ROOM.enqueue(guest, "/private/requests", [1] * 16, {"action": "stop"})
        self.assertEqual(result, "/private/requests/result-abc.json")
        self.assertEqual(upload.call_args.args[:2], (guest, "/private/requests/pending-abc.json"))
        self.assertEqual(json.loads(upload.call_args.args[2]), {"instance": [1] * 16, "command": {"action": "stop"}})
        guest.run.assert_called_once_with("mv", "-n", "/private/requests/pending-abc.json", "/private/requests/request-abc.json")
