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
