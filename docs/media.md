# Media

Capture is a local action. Joining a room or receiving network traffic cannot
start the microphone, camera or screen on another device.

```text
explicit local source -> encode once -> bounded RTP -> authorized peer/kind
                                                             |
                                                    explicit local playback
```

## Choose A Path

| Path | Sources/sinks | Qualification |
| --- | --- | --- |
| Native Linux CLI | Pulse-compatible audio, V4L2 camera, explicit X11 display | Codecs and owned virtual-display path verified; hardware is separate |
| Browser workspace | Browser capture, WebCodecs/AudioWorklet, selected playback | Software/synthetic flows verified; browser/OS permission still required |
| Wayland native portal | Not implemented | X11 is not a portal bypass |
| Windows/macOS native | Not qualified | No native full-feature claim |

## Browser Source And Device Choice

See [browser capture controls](browser-capture.md) for display-surface hints,
explicit device choices, privacy boundaries and exact qualification status.

## Commands

```sh
kk media doctor
kk media preview microphone --device EXPLICIT_PULSE_DEVICE --consent
kk media preview camera --device /dev/video0 --consent
kk media preview screen-x11 --device :PRIVATE_DISPLAY --window WINDOW_ID --consent
```

`doctor` probes plugins without opening devices. Preview is local capture and
playback, not a network conference. Choose only a device/display you intend to
capture; microphone playback can feed back without headphones.
`PRIVATE_DISPLAY` and `WINDOW_ID` are placeholders for the owned local display
number and numeric window ID; they are not literal device names.

Room publication and reception select exact peers and kinds:

```sh
kk room media team publish screen --device :PRIVATE_DISPLAY --to PEER_DIGEST --consent
kk room media team listen screen --from PEER_DIGEST --x11 --consent
```

`--inspect` is transport-only, not playback. `--x11` selects native X11 video output
and is not an audio option. See each subcommand's `--help` for source selectors.

## Bounds And Teardown

### Full-Room Capacity

[Central media limits](../src/config.rs) are authoritative. The budget is per
room, per local endpoint, not a hardware-independent participant claim:

| Budget | Equation | Current value |
| --- | --- | --- |
| Remote members | `ROOM_MAX_MEMBERS - 1` | 7 |
| Source kinds per peer | `MEDIA_SOURCE_KINDS` (audio, camera, screen) | 3 |
| Incoming tracks and subscriptions | `MEDIA_MAX_STREAMS = (ROOM_MAX_MEMBERS - 1) * MEDIA_SOURCE_KINDS` | 21 each |
| Local publications | `MEDIA_MAX_PUBLICATIONS = MEDIA_SOURCE_KINDS` | 3 |
| Combined session reservations | `MEDIA_MAX_USERS = MEDIA_MAX_STREAMS + MEDIA_MAX_PUBLICATIONS` | 24 |

Local publication reservations cannot consume the incoming reservation budget,
or vice versa. Each receiver explicitly subscribes to an exact peer and kind;
there is only one live listener and one live source for that pair. A duplicate
offer, including the same track ID on a different connection, is rejected before
it can replace an established source. Colliding IDs and RTP SSRCs from different
peers remain isolated. Capacity does not grant consent or start capture.

The hub's aggregate queued RTP payload bound is
`MEDIA_MAX_STREAMS * MEDIA_PACKET_QUEUE * MEDIA_PACKET_BYTES`:
`21 * 8 * 1024 = 172032 bytes` (168 KiB, 168 packets) per endpoint per room.
Across a full eight-endpoint room this is at most 1376256 bytes (1344 KiB,
1344 packets) of hub-queued RTP payload. These calculations exclude queue/track
metadata, allocator overhead, transport buffers, encoded-frame assembly and
native/browser decoder resources; they are not total process memory limits.
The independent QUIC buffer remains `MEDIA_DATAGRAM_BUFFER_BYTES` (66560 bytes
per configured send/receive buffer), not a multiplied or unbounded queue.
Packets remain capped at 1024 RTP bytes, encoded frames at 256 KiB and queue
age at 250 ms. Full queues drop packets without waiting on a consumer; policy
refresh, consent expiry, source idle timeout and bounded session lifetime remain.

Deterministic hub regressions fill all seven real remote peers' three kind slots
and all queues, reserve three local publications, and verify excess/duplicate
rejection and source isolation. A separate actual three-peer loopback QUIC test
publishes all three synthetic kinds from every peer, receives six tracks at each
peer concurrently and completes a control RPC during media traffic. Its policy
leases are explicit synthetic fixture consent, not room-actor or UI consent
qualification. Neither test measures sustained eight-peer network load, codec
CPU/GPU cost, native hardware, browser playback or synchronized A/V. Those need
separate load, native and foreground UI qualification before performance claims.

- Opus/VP8 use standard RTP with bounded RFC4571 pipe framing.
- Queues and native resources are bounded; stale buffers can be dropped.
- Native children receive an allowlisted environment, not provider credentials or
  arbitrary shell/pipeline text. GStreamer and Linux util-linux tools are explicit
  runtime prerequisites, not an isolation sandbox.
- Graceful publication stop finishes the active framed write before FIN.
- Stop, session loss and owner death terminate owned tracks/processes; decoder
  failure is not reported as intentional success.

Queued datagrams do not prove delivery, playback or synchronized A/V. Bitrates and
queue settings are bounded initial policies, not measured congestion adaptation.
See [platforms](platforms.md), [lab](lab.md) and [verification](progress.md).
