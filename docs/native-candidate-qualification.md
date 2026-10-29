# Native Candidate Contention Evidence

This is a private-display lab gate, not physical desktop/camera/microphone capture.
The unsigned `a60aab6` CI executable and its digest are bound in the
[installation evidence](install-qualification.md); the stable installation is unchanged.

| Checkpoint | Actual result |
| --- | --- |
| Three-guest public CLI | The same candidate passes 25 scenarios, including creator VM shutdown and verified replica retrieval; report `lab/tmp/public-room-cli-fe3ecdeb2b0b.json` |
| Initial native attempt | `lab/tmp/native-media-4a483780fec0.json` fails before capture: host room readiness times out; its retained `host-room.log` reports `unsafe local session ancestor`. Guest/network cleanup is confirmed; no native playback pass follows |
| Fixture correction | The host profile was below potentially group-writable lab ancestors. A fresh physical 0700 OS temporary workspace now contains its private state/logs. Two red/green tests enforce private ownership, canonical paths, unchanged shared-parent permissions and actual driver wiring; all 35 lab cases pass |
| Corrected native replay | In progress with the unchanged candidate bytes and original deadlines; results and cleanup must be inspected before recording a pass |

The failed workspace remains retained. No existing profile was relocated or
recreated, no ancestor permissions were broadened, and no product protection was
relaxed. Reports remain in ignored `lab/tmp`; private workspaces retain their
own reported path under the OS temporary root for inspection.

The native gate must observe fresh decoded private-display pixels while a bulk
file is still active and foreground messages receive acknowledgements, then verify
recipient bytes and restore the exact previous live bandwidth policy. Merely
starting a publisher or exchanging RTP does not satisfy those assertions.

Neither successful functional execution nor archive integrity supplies signed
release provenance, installer qualification, physical-network traversal evidence
or full native Windows/macOS support.
