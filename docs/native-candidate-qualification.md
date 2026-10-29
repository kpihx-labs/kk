# Native Candidate Contention Evidence

This is a private-display lab gate, not physical desktop/camera/microphone capture.
The unsigned `a60aab6` CI executable and its digest are bound in the
[installation evidence](install-qualification.md); the stable installation is unchanged.

| Checkpoint | Actual result |
| --- | --- |
| Three-guest public CLI | The same candidate passes 25 scenarios, including creator VM shutdown and verified replica retrieval; report `lab/tmp/public-room-cli-fe3ecdeb2b0b.json` |
| Initial native attempt | `lab/tmp/native-media-4a483780fec0.json` fails before capture: host room readiness times out; its retained `host-room.log` reports `unsafe local session ancestor`. Guest/network cleanup is confirmed; no native playback pass follows |
| Fixture correction | The host profile was below potentially group-writable lab ancestors. A fresh physical 0700 OS temporary workspace now contains its private state/logs. Two red/green tests enforce private ownership, canonical paths, unchanged shared-parent permissions and actual driver wiring; all 35 lab cases pass |
| Corrected native replay | Formerly pending; `lab/tmp/native-media-ce4a303f739f.json` now passes with unchanged candidate bytes/deadlines. Fresh private-display pixels decode during the active bulk file; native receiver reports 1,478 packets and playback=true, recorded=false |
| Contention observations | At 125 KiB/s live inbound shaping, nine text acknowledgements overlap bulk transfer; maximum observed acknowledgement is 1.760454 seconds. Bulk completion is observed at 21.763589 seconds and recipient SHA-256 matches the expected bytes. These are observations, not an SLA |
| Cleanup | Report failure is null and cleanup_errors is empty; exact prior bandwidth policy is restored. Post-run libvirt checks confirm three guests shut off and the dedicated network inactive, persistent and non-autostarted |

The failed workspace remains retained. No existing profile was relocated or
recreated, no ancestor permissions were broadened, and no product protection was
relaxed. Reports remain in ignored `lab/tmp`; private workspaces retain their
own reported path under the OS temporary root for inspection.

The native gate must observe fresh decoded private-display pixels while a bulk
file is still active and foreground messages receive acknowledgements, then verify
recipient bytes and restore the exact previous live bandwidth policy. Merely
starting a publisher or exchanging RTP does not satisfy those assertions.

Mint's room process exits 0 but retains `bounded close grace elapsed` with Quinn
drivers in its stderr. This known best-effort drain diagnostic is not suppressed
or counted as a bounded-grace pass. Final guest/network shutdown is verified;
these observations alone do not prove every intermediate socket-release timing.

Neither successful functional execution nor archive integrity supplies signed
release provenance, installer qualification, physical-network traversal evidence
or full native Windows/macOS support.

## Production-fix Checkpoint, `1f846ef`

The [production-fix executable binding](file-duplex-qualification.md#executed-production-fix-checkpoint-1f846ef)
is distinct from the earlier `a60aab6` binary. Report
`lab/tmp/native-media-452c36fbbda2.json` records 1,459 packets and actual playback,
fresh decoded private-display pixels during bulk, and nine text acknowledgements
at 125 KiB/s. Maximum observed ACK is 1.719293 seconds; bulk completion is observed
at 21.500533 seconds, with the expected recipient digest. Failure is null and
cleanup_errors is empty; the previous bandwidth policy is restored exactly.

Both Fedora and Mint room processes exit 0 while retaining the known close-grace
diagnostic; the retained host-room log also contains it. The report does not persist
every host-process exit code, so no all-host-exit-code claim is inferred. After this
run, all three guests are confirmed shut off and the
dedicated network inactive/persistent/non-autostarted. This does not qualify a
separate build or erase the earlier diagnostics and failed workspace attempt.

## Corrected Fixture Checkpoint, `cfa8145`

The [corrected executable binding](install-qualification.md#corrected-fixture-checkpoint-cfa8145)
is independently replayed, not inferred byte-identical to `1f846ef`.
`lab/tmp/public-room-cli-c648942a321c.json` passes all 25 CLI scenarios.
`lab/tmp/native-media-585869681e93.json` records 1,507 packets with playback=true,
recorded=false, and fresh decoded private-display pixels during the bulk file.
At 125 KiB/s, nine text acknowledgements overlap bulk; maximum observed ACK is
1.644308 seconds and bulk completion is observed at 21.652393 seconds. Recipient
digest matches. Both reports have failure=null and cleanup_errors=[].

The prior live bandwidth policy is restored exactly. All three guests and the
dedicated network are confirmed off afterwards; definitions/disks are retained.
Mint and the host room log retain the known close-grace diagnostic; Fedora exits
0 without it. Publisher/bulk logs report success; pattern/Xwayland logs are empty.
These are this candidate's observations, not signed publication or an SLA.

## Listener And Executable Binding Guards

The native invitation listener now binds TCP port zero, rather than a UDP-derived
neighbour port. Its source guard fails on the old expression and passes after
the correction. Actual replay `lab/tmp/native-media-7cb86df2295f.json` passes with
the same corrected executable: 1,491 packets, nine overlapping ACK samples, maximum
observed ACK 1.670950 seconds at 125 KiB/s, and verified recipient bytes.

Earlier reports recorded the selected host digest and trusted guest-agent upload;
they did not persist independently measured guest executable digests. The new shared
guard checks only the public deployed CLI with guest-side SHA-256, before chmod or
execution, and again at each public/native gate. Invalid expected hashes, mismatched
digests, missing output and wrong reported paths fail closed. Four red/green tests
bring the lab suite to 40 passing cases, without exporting private guest files.

Before redeployment, direct probes confirm the retained executable on all three
guests matches the corrected digest. The strengthened replay passes all 25 public
CLI scenarios in `lab/tmp/public-room-cli-6e9d4f6ce3fe.json`; its
`guest_binary_sha256` records all three independent measurements. Native report
`lab/tmp/native-media-d90222529fd0.json` records both participating guest digests,
1,466 packets and nine ACK samples during bulk; maximum observed ACK is 1.684260
seconds, bulk completion 21.928632 seconds. Both reports have null failure and no
cleanup errors; the prior QoS policy and guest/network shut-off are verified.

## Broader Baseline Compatibility

The first broader baseline invocation retains partial report
`lab/tmp/kk-results-2371f5ec13c1-1793312670632847890.json` but fails in the legacy
node harness: default status is now human-readable, and successful JSON status may
legitimately describe an offline node. That partial report is not a baseline pass.
The adapter now explicitly requests `status --json`, extracts only an online node's
status, and rejects malformed/legacy/unknown shapes. Five red/green regression
cases bring lab coverage to 45; readiness deadlines and application behavior are
unchanged. The initially pending corrected replay now passes all 66 multi-guest
scenarios; report `lab/tmp/kk-results-ce2745ed2663-1793313646433414997.json` records
the corrected digest on the host and independently on each guest. It includes
clean/limited/difficult/blackhole networking, direct/contact files and interruption/
resume, sensitive input, pairing/revocation, live-node bulk/control progress and
crash/restart/socket release. Capture stays within the declared guests. All three
guests and the dedicated network are confirmed off after the run; disks retained.
