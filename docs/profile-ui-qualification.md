# Interactive Profile Presentation Evidence

Companion to [UI qualification](ui-qualification.md), not native/mobile proof.
The two disposable profiles used separate foreground processes and existing owned
Edge tabs. No operator profile, key material or browser identity was changed.

Executable SHA-256:
`87e2d41e2a27b68b04637fc9ef22450381376c304454c3a94c8f05b6e6a3f3e4`.

| Interactive case | Observed result |
| --- | --- |
| Same visible name on both devices | Morgan temporarily renamed itself Alex and explicitly enabled sharing; both views eventually showed two Alex names with two distinct full IDs |
| Identity presentation | This-device/role and self-announcement provenance remain visible; fingerprints start collapsed |
| Bidirectional messages with duplicate names | Controller-to-member message and member-to-controller reply both arrived; names did not become routing keys |
| Local nickname | Controller assigned Morgan (local) to the exact other member; presentation reports local_override |
| Opt-out after local override | The local nickname remains; it is not a remote announcement or trust assertion |
| Long Settings conflict | CLI on/off changes invalidate the open form; conflict is visible, draft name retained, sharing reloaded off |
| Confirmation containment | Save and Close remain inside the live viewport while form contents scroll, including after conflict text appears |
| Explicit retry | Save succeeds without silently enabling sharing; disposable profile restored to Morgan with sharing off |

Reports include `page-evaluate_20261028_095402_912723.json` (two visible names,
two identities), `page-evaluate_20261028_095439_652387.json` (both remote names),
`page-evaluate_20261028_095515_780501.json` (local override), and
`page-evaluate_20261028_095607_731424.json` (contained conflict/draft controls),
under `/tmp/browser-proxy-autosave`.

Remote-name refresh is asynchronous: an early intermediate read had not yet
received both names. This run waited for actual presentation state instead of
claiming the first click or cached view established completion. Three-party
duplicate-name recipient selectors remain a distinct case from this two-party run.
