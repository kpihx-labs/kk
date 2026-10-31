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

| Budget | Code authority |
| --- | --- |
| Incoming tracks/subscriptions | `MEDIA_MAX_STREAMS`, derived from remote members and source kinds |
| Local publications | `MEDIA_MAX_PUBLICATIONS`, separate from incoming reservations |
| Combined sessions | `MEDIA_MAX_USERS` |
| Queued RTP payload | `MEDIA_MAX_STREAMS * MEDIA_PACKET_QUEUE * MEDIA_PACKET_BYTES` |

Local publication reservations cannot consume the incoming reservation budget,
or vice versa. Each receiver explicitly subscribes to an exact peer and kind;
there is only one live listener and one live source for that pair. A duplicate
offer, including the same track ID on a different connection, is rejected before
it can replace an established source. Colliding IDs and RTP SSRCs from different
peers remain isolated. Capacity does not grant consent or start capture.

Queued-payload bounds exclude track metadata, allocator overhead, independent
QUIC buffers, encoded-frame assembly and decoder resources; they are not total
process-memory limits. Packet/frame sizes and queue age remain code-owned. Full
queues drop packets rather than waiting; consent expiry, policy refresh, idle
timeout and bounded session lifetime still apply.

Deterministic hub regressions fill remote kind slots and queues, reserve local
publications, and verify excess/duplicate
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
