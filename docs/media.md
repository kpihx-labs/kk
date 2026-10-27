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
