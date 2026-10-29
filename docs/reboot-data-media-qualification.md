# After-reboot file and synthetic media qualification

The same two profiles and Edge tabs from [the reboot UI run](reboot-ui-qualification.md)
subsequently exercised these additional cases. These observations extend the
development-binary evidence; they do not attest a release archive or replace
native hardware qualification.

| Case | Result | Evidence |
| --- | --- | --- |
| File consent and binary integrity | Pass | A generated 65,536-byte pattern and an empty file arrived only after incoming-file consent. Authenticated retrieval matched their SHA-256 values, respectively `7daca2095d0438260fa849183dfc67faa459fdf4936e1bc91eec6b281b27e4c2` and `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. |
| Consent withdrawal | Pass | Incoming-file consent was explicitly set off and observed off before a fresh three-byte probe. The sender retained an editable form with the transfer-not-allowed explanation; the receiver's stored-file count stayed unchanged at three. Earlier toggle attempts are not evidence of withdrawal. |
| Simultaneous synthetic screen streams | Pass | Both profiles held one publication and one subscription concurrently. Alex decoded 1,422 blue-source frames with a center pixel of `[26,76,214,255]`; Morgan decoded 351 red-source frames with `[205,9,52,255]`. These are lossy VP8 outputs from generated canvases, not desktop captures. |
| Control traffic during both streams | Pass | With both publications and subscriptions live, a new message reached Alex. Morgan displayed event-queue acknowledgement, explicitly not human reading. Both receiving tiles continued reporting encrypted live media. |
| Independent source stop | Pass | Stopping Alex's publication removed Morgan's corresponding receiving tile but retained Morgan's own publication. Alex's opposite subscription continued decoding, advancing to 131 frames after its counter snapshot. |
| Synthetic-capture cleanup | Pass | Both pages stopped all media sessions and all retained synthetic tracks were observed ended. Fixture timers were cleared and the saved display-capture methods were restored. |

The media fixture supplied only detached, generated red/blue canvases through a
temporary page-local display-capture replacement. Physical desktop, camera and
microphone capture were not requested. Each source had a bounded automatic stop;
some initial attempts ended before consent or during fixture replacement. Those
attempts do not count as successful continuous-media runs. The passing concurrent
run used explicit receiving consent before publication.

A further generated 65,536-byte XOR-pattern transfer completed after the media
cleanup, with digest
`86092f4829ca304327e8a15d248501343d8514a82ee5621169d18a154eddd18a`.
The pages had zero media tiles then: this is not evidence of file transfer under
simultaneous-media load. Native file-picker interaction, completed browser disk
saving, physical source selection and final process restart remain separate gates.

## Concurrent-file Continuation, 2026-10-29

Main relaunched both unchanged profiles in foreground panes `%122` and `%125` of
`kpihx-labs:15`, using explicit profile paths rather than the OS catalog. Both
retained their exact device identities, two members and epoch one. The development
binary at source `a60aab6` reports `0.9.0-dev.2`; executable SHA-256 is
`a81d7dd7b4254611ac5a0baa7b4f882814d12d197e5279efbf26d806dae1f67b`.
The same owned Edge window/two tabs were reused; no new browser context was made.

| Case | Observed evidence, UTC |
| --- | --- |
| Concurrent synthetic sources | At 18:53:28-29 each page held one screen publication and one subscription. Generated red/blue canvases replaced display capture only in those pages, with a 300-second automatic stop; no physical source was requested |
| File during active media | Morgan submitted a generated 4,194,304-byte file at 18:53:48, after Alex's explicit incoming-file opt-in. At 18:54:03 the modal had closed with `Verified durable recipient storage.`; both pages still held both media tiles |
| Independent bytes | Browser SHA-256 and recipient filesystem SHA-256 both equal `1dc3290cfda4caed83dd2c6f9298c2f13ee3260a801df977c000c4ac78315df5`; filesystem size is exactly 4,194,304 bytes. Alex's inventory subsequently lists the 4096-KiB file |
| Media/control progress | Morgan's decoded counter grows from 256 before file submission to 272 after completion and 622 after new chat. Alex grows from 689 before submission to 855 after completion and 889 afterwards. Both retain publication/subscription tiles. A new chat reaches Morgan; sender reports recipient-event-queue acknowledgement, not human reading |
| Cleanup | At 18:55:21-23 both Stop my media actions leave zero tiles; retained synthetic tracks report ended and the original display-capture functions/timers are restored. Alex's file consent is restored off. Both authenticated UI shutdowns succeed and both panes return to their shells; both QA pages are navigated to blank |

This closes the earlier absence of a file sent while both synthetic media sessions
were active. Counter snapshots bracket the transfer, not every packet: they do not
prove a frame was decoded during each individual file-write interval, sustained
bandwidth/latency, shaped-network contention, native file picking, browser disk-save
completion, physical capture or new-release artifact qualification. The generated
file remains in the disposable profile; no existing received file was deleted.
Closing the extra blank Morgan tab returned `APPROVAL_TIMEOUT`; it remains blank,
not proven closed. No raw-CDP alternative bypassed that approval boundary.
