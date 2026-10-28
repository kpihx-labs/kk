# Browser Source And Device Choice

Transport, native commands and capacity: [media](media.md).

Open **Share a native source**, choose the source kind, then submit **Choose
source and authorize capture**. Opening the dialog lists devices without calling
`getUserMedia` or opening a permission prompt. Capture only starts after submit;
duplicate publications, capacity exhaustion and stale room consent fail before
the capture call. Receiving a remote source opens playback only, never local
camera, microphone or display capture. Each source retains its own Stop control.

| Local choice | Capture request | What it guarantees |
| --- | --- | --- |
| Browser/OS chooser (any source) | `getDisplayMedia` without a surface preference | Browser/OS decides which sources to offer |
| Entire screen | Video `displaySurface: "monitor"` | Preference only |
| Window/application window | Video `displaySurface: "window"` | Preference for one chosen window, not every window of an application |
| Browser tab | Video `displaySurface: "browser"` | Preference only; tab capture may not be offered |
| Default device | `getUserMedia` without `deviceId` | Browser/OS chooses its default microphone or camera |
| Listed microphone or camera | Kind-matched `deviceId: { exact: selectedId }` | Only the explicitly selected device; failure does not silently substitute another |

Display requests use no mandatory `exact`/`min` surface constraints, custom window
enumeration or window previews. The browser/OS chooser and permission policy are
authoritative even when they ignore a hint. A local display tile reports the
actual video track's `getSettings().displaySurface`, or **Not reported by browser**
when unavailable or unrecognized. It never presents the requested preference as
an observed source. Screen audio is not requested.

Device listing processes at most the first 64 browser entries, includes only
audio/video inputs and bounds copied IDs and labels. Labels can be absent until
permission is explicitly granted. **Refresh devices** lists again without
requesting capture, and a successful explicit camera/microphone grant refreshes
labels automatically. Missing/failed enumeration leaves Default device usable;
unsupported exact device selection, disappeared devices and denied/cancelled
capture produce feedback instead of a fallback capture attempt. Video quality
offers the existing low-bandwidth, balanced and detailed encoding presets; it is
not a promise of measured network throughput or physical-device resolution.

Device IDs are kept only in the open dialog's memory, not profile storage or
global hardware identity. Browser origins, ports and permissions can change them;
open a fresh dialog and select again rather than reusing an old ID. Device IDs,
labels and window titles are not added to transport URLs or RTP metadata. The
shared pixels/audio can themselves contain private information, so inspect the
browser/OS choice before granting capture.

`node --test tests/ui_media.mjs` provides synthetic API/constraint and lifecycle
regressions, not physical device, permission-dialog, chooser or rendered-layout
qualification. Actual browser/OS hint behavior, device availability/labels,
permission denial/retry and foreground UI accessibility remain manual/integration
qualification tasks. These controls do not claim application-wide multi-window
capture, recording, remote capture initiation or custom previews.
