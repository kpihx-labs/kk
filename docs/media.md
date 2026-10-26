# Native Media Boundary

This is an unreleased native-process/codec slice, not completed room conferencing.
No physical microphone, camera or host desktop capture is claimed as test evidence.

## Available Commands

```sh
kk media doctor
kk media preview microphone --device EXPLICIT_PULSE_DEVICE --consent
kk media preview camera --device /dev/video0 --consent
kk media preview screen-x11 --device :PRIVATE_DISPLAY --window WINDOW_ID --consent
```

Doctor probes plugin presence without opening capture devices. Preview performs
actual local native capture, encoding and decoding, only after explicit command
consent. Microphone preview plays locally; headphones avoid acoustic feedback.
Display zero/window zero deliberately selects that local X display's root.
There is no default display or remote capture URI and no free-form pipeline input.
Linux PulseAudio-compatible audio, V4L2 raw camera and explicit X11 capture builders
are implemented. They have not yet completed the device/platform acceptance gates.

X11 capture is NOT a Wayland portal bypass. Portal screen sharing remains to be
implemented with CreateSession, SelectSources, Start, OpenPipeWireRemote and
session-close/revocation handling. Version-six portal stream targeting must prefer
the non-reused PipeWire serial, not an old numeric node ID after device changes.
Windows/macOS bounded capture/playback are not yet implemented.

## Ownership And Bounds

`media::Source` selects native capture and `media::Sink` selects native playback.
No constructor reads devices until capture/preview/playback explicitly starts.
The source enum is not a network-controlled executable or serialized pipeline.
User identifiers are bounded and reject whitespace, shell/pipeline syntax and
remote X displays. Arguments are passed directly, never through a shell.

GStreamer uses Opus and VP8 with standard RTP and RFC4571 pipe framing. Packets are
bounded before allocation, validate RTP structure/type and retain content in
zeroizing buffers without Debug/automatic history. Capture queues hold at most two
raw buffers and discard stale buffers; video is initially 640x360 at 15 fps with
an 800 kbit/s target, audio at 64 kbit/s. These are bounded initial settings, not
measured network congestion adaptation or audiovisual synchronization guarantees.

The native child has one supervisor. Cancellation requests termination and joins
the child; stdin/stdout close under the same owner. Stderr is continuously drained
while retaining at most 8 KiB, rather than stopping at the limit and blocking the
encoder. Only explicit native-session environment keys are inherited, not provider
credentials or arbitrary GStreamer debug/plugin overrides. Linux children inherit
hard address-space (2 GiB) and data-space (256 MiB) limits through `prlimit`.
These limits are NOT an isolation sandbox or a claim of independently audited
native parsers. GStreamer and `prlimit` are explicit system runtime dependencies.

## Evidence

Four targeted host tests pass, including explicit native GStreamer Opus/VP8
encoding into bounded RTP packets, native decoding, process cancellation/reaping,
input validation and packet framing. The native codec test uses synthetic sources
and a decoding fakesink: it proves codecs/processes, not microphone/camera capture
or speaker/display output. CLI tests require consent and reject pipeline injection
before opening a capture process. All fourteen probed plugins are present locally.

Explicit native gates are ignored by the default Rust suite because they require
declared host media/display dependencies. Run them deliberately; an ignored test
is not passing evidence. A private Xwayland virtual-display capture gate is being
verified separately. Room media authorization/transport, pacing under contention,
native sinks, device/portal/platform acceptance and final security review remain
open. No raw RTP pipe is advertised as authenticated room media.

## Upstream References

- [RTP stream payloading, RFC4571](https://gstreamer.freedesktop.org/documentation/rtp/rtpstreampay.html)
- [RTP stream depayloading](https://gstreamer.freedesktop.org/documentation/rtp/rtpstreamdepay.html)
- [X11 native capture and XInitThreads](https://gstreamer.freedesktop.org/documentation/ximagesrc/index.html)
- [PulseAudio native source](https://gstreamer.freedesktop.org/documentation/pulseaudio/pulsesrc.html)
- [V4L2 native source](https://gstreamer.freedesktop.org/documentation/video4linux2/v4l2src.html)
- [ScreenCast portal lifecycle](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [GStreamer licensing](https://gstreamer.freedesktop.org/documentation/licensing.html)

Inspected installed GStreamer 1.28.2 RTP, Opus and VP8 plugin metadata. GStreamer
core/base/good plugins report LGPL; packaging must still retain upstream notices,
provide the applicable corresponding-source/relinking conditions and audit the
actual bundled dependency set. This does not relicense upstream code under BUSL.

## Integrated Room And Explicit Display Update

The later room adapter publishes only after exact peer/kind consent, and uses
fresh room/file/media policy owners rather than capture instructions from the
network. Browser VP8/Opus and the CLI RTP interoperability gates are recorded in
`progress.md`; these supersede the earlier pre-room snapshot above.

```sh
kk room media team publish screen --device :OWNED_PRIVATE_DISPLAY --to PEER --consent
kk room media team listen screen --from PEER --x11 --consent
```

`--x11` selects the local DISPLAY server explicitly instead of automatic video
sink discovery; it conflicts with transport-only `--inspect` and is refused for
audio. It does not choose a display remotely or bypass an OS capture permission.
Native decoder diagnostics remain bounded and are surfaced on shutdown. A natural
decoder failure is not classified as a successful requested stop.

The paced SDK relay now accepts a graceful stop future. It stops at a packet
boundary, completes an active bounded IPC write and only then allows finish/FIN.
The CLI routes Ctrl-C through that owner rather than dropping a write mid-frame.
A constrained-socket regression requests stop after reading one frame byte and
verifies every delivered RTP frame and the final reply remain complete.

The current native cross-guest gate has proved actual fresh decoded pixels before
and during bulk traffic, but initially exposed a Ctrl-C framing failure. The
framing fix passes its regression and is being replayed through the whole native
scenario. That replay, warnings review and final full validation are not implied
by the unit test or by packet counts alone.
