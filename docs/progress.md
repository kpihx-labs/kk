# Completion Progress

## Frozen 0.8.0 Linux Regression (2026-10-26)

The release-version software freeze passes the complete final VM batch on binary
SHA-256 `2a626cbd294d1e0049e2543a9349938ff22256562611b36882201b57841fbc35`:

- Public CLI/copies: 25 scenarios, `lab/tmp/public-room-cli-1f124e3e8e8f.json`.
- Complete baseline: 66 scenarios, `lab/tmp/kk-results-9d2dea7c4adc-1793053715758106841.json`.
- Native capture/guest decode/shaped-link contention: `lab/tmp/native-media-b8740c541060.json`.

The native run receives 1,302 actual native packets, proves a changed decoded image
while bulk data is active, verifies the 1,094,160-byte file and records nine
concurrent text acknowledgements with maximum latency 1.5414 seconds at 125 KiB/s.
The private guest X server now exits with no keymap warnings using its explicit
private compatible library path. Recorded forced QUIC-drain notices take the
reviewed bounded terminate/join path. QoS is restored exactly; all dedicated
guests and the private network are stopped, with no cleanup error.

The exact local `make check` also passes 155 Rust, six JavaScript and seven archive
tests; the nine explicit media tests and 21 lab adapter tests pass separately.
All 474 locked dependency license declarations/notices are inventoried without a
missing notice, and offline Cargo vendoring succeeds. The workflow passes local
actionlint. Current remaining delivery gates are committed-source bundles,
required remote CI/ARM64 qualification, public provenance, installation and tag
publication. Native Windows/macOS and physical-device limits remain explicit in
`platforms.md`, not silently waived by this Linux result.

## Explicit Replicas Passed The VM And UI Paths (2026-10-26)

Public CLI gate: 25 scenarios, `lab/tmp/public-room-cli-943e5eff1a70.json`, binary
SHA-256 `d3a99b5e1bc55727696c54c72747cb7a514d0605283c0bd4b6f62db8d5387bf6`.
This includes actual signed voter-address movement/cold resume and explicit file
offers, selected-audience lookup, withdrawal, membership-epoch retirement,
verified pull with the original VM powered off, prefix reverification and
refused withdrawn pulls preserving the existing copy. Guest/network cleanup passed.

Later source makes registry epoch advancement monotonic and retains the watermark
after withdrawal, preventing delayed older publications from removing newer
offers. Local tests also prove publisher restart does not silently reoffer a file
and a changed advertised source cannot replace the recipient's verified copy.
The browser adapters use the same bounded file lane and strict inbox identifiers.
Configured-profile GUI evidence:
`/tmp/browser-proxy-autosave/page-evaluate_20261026_191555_830610.json` (one exact
authorized hint), `page-evaluate_20261026_191639_017528.json` (verified stored copy,
neutral native background) and `page-evaluate_20261026_191655_700447.json` (withdrawn
fetch refused, existing stored copy retained).

An isolated regression reproduced 16 abandoned read-only status replies being
misreported as operation failures. Only a completed Status reply's closed-client
BrokenPipe/ConnectionReset is now normal observation cancellation. All other
command delivery failures remain visible because effects may already commit.
The deterministic cancellation regression and the extended HTTP integration pass
without the earlier spurious broken-pipe diagnostic.

Native shaped-link capture/decode/contention is the current gate. No final native
contention, deep audit, portable-platform or signed-publication success is claimed.

## Browser Workflows And Neutral Layout Verified (2026-10-26)

The operator explicitly selected the existing configured Edge profile and one
dedicated owned window. This supersedes the historical smoke-profile wording.
Typed browser actions and bounded opaque test fixtures replace raw/HITL calls.
The opener reads the CLI's complete private output envelope, not its truncated
console presentation. Test invitation delivery verifies the actual page handler
completed; an action's success report alone is not file-attachment evidence.

Browser evidence: both CONFIRM values `9029c8a18db5` matched before approvals;
the two independently stored profiles resumed at epoch 1 with two data members
and one voter. Literal `<script>` text arrived with zero injected script nodes.
An explicitly accepted file appeared in the recipient inbox. Live synthetic VP8
decoded 588 frames and reproduced RGBA `[112,112,112,255]`; synthetic Opus decoded
748 blocks with receiver playback muted. Both sides removed all media tiles and
the test source/playback overrides were restored. No physical capture occurred.

Actual browser results are retained under `/tmp/browser-proxy-autosave/`, including
`page-evaluate_20261026_171801_999887.json` (VP8 pixels),
`page-evaluate_20261026_171908_141255.json` (Opus) and
`page-evaluate_20261026_174158_351670.json` (final two-theme 390 x 844 layout).
The last layout fixture includes three retained operation errors and proves no
horizontal overflow, a visible composer and an unobstructed Send button. Native
theme surfaces are neutral, including the actual dialog sibling's backdrop.
App-local `darkreader-lock` prevents extension double coloring without changing
the operator's browser configuration. Static layout fixtures do not qualify a
physical mobile device, keyboard or permission portal.

The first full host check exposed ENXIO when opening `/dev/stdout`: this agent's
stdout is a Unix socket, which cannot be reopened as a pipe. The bounded I/O
owners now duplicate socket descriptors and use per-call DONTWAIT send/receive
without changing shared flags. Subprocess tests cover socket input/output and
cancelled stalled reads/saturated writes. The subsequent exact `make check`
passes 147 Rust tests, four intentionally separate native tests ignored, and all
five JavaScript tests, with warning-free Clippy and documentation.

Next: remaining native contention, replica discovery/availability and fresh VM
integration evidence, then the operator-required deep audit, platform packaging
and dual-remote publication. No final product or portable release is claimed.

## Foreground UI And Recovery Integration (2026-10-26)

The public CLI VM gate passes SEVENTEEN scenarios:
`lab/tmp/public-room-cli-61c9093ce0c9.json`, CLI SHA-256
`7d08b08e1b7187e093ae3cb4298c220c66c43ee015686cce79d08eba28981c62`.
Its three media additions are explicitly synthetic RFC4571/RTP transport and
consent checks, NOT physical capture/decoding. Dedicated guests/network stopped.
Separately, all six explicit host native media tests pass, including real capture
of an owned private Xwayland display, VP8/Opus encode/decode, owned shutdown and
actual native-child exit after owner SIGKILL. No host desktop/mic/camera was read.

Later source persists verified join context before private key generation,
tracks membership initialization atomically with SDK checkpoints and handles
cancellation inside the enrollment owner. An actual recipient CLI is SIGKILLed
after replicated admission but before its stage reply; `room serve` activates
from the committed Welcome with identical device/MLS identities and exchanges
text. That test and the marker/cancellation/forwarding regressions pass. Missing
keys or uncertain/lost membership evidence refuse without regeneration; an
uncommitted bootstrap is not silently replaced by a new identity.

Signed node metadata intents are now explicit v2 learner/self-address operations.
Configured-device ownership is independent of data rights and votes. Prior
configuration CAS and exact sender/context signatures are checked. Existing
metadata uses official SetNodes, NOT AddNodes (which preserves old metadata),
and receipts verify the actual committed metadata and unchanged voter sets.
CLI tests move a voting node's port, preserve identity/votes, continue data and
persist the new address. Cached connections bind their authenticated address
hint; active media refuses obsolete routes. Full VM replay of this later slice
remains pending. Mixed unreleased wire versions are not promised compatible.

The new light/dark foreground UI uses the same SDK with bounded event observers,
readiness and structured stop, not UI-owned trust rules. Three targeted HTTP tests
pass: independent-device confirmed pairing, real messages, explicit sensitive
reveal, consented files, downloads and forwarding; session/Host/Origin controls.
Storage shutdown now awaits actual SQLite/lease destruction through a last-field
release signal, fixing same-process create/resume without weakening locks.
UI snapshots wait for actual managed readiness rather than probing a bootstrap
socket and producing canceled-query broken pipes.

UI authentication is a private-file one-use HMAC bootstrap followed by bearer
headers in origin/port-scoped session storage; no localhost cookies, token argv
or token console output. Browser test launch delivers the file opaquely to the
test-only smoke profile. Assets are bundled offline. Browser rendering/live media
are being verified. Three JavaScript RTP tests and syntax checks pass. The last
full pre-UI host check passed 139 Rust tests; newer targeted successes do not
substitute for the required final full check/audit/platform/distribution gates.

## File Integration VM Gate And Native Media Start (2026-10-26)

The extended public-CLI VM gate PASSES all FOURTEEN scenarios:
`lab/tmp/public-room-cli-00150bf04863.json`, release CLI SHA-256
`fa6642e26861c959bdd5a0a299e767875d16c00d5a5a25a586ecfe61da2bd390`.
It adds actual recipient-consent refusal, verified file storage, complete-prefix
repeat, size refusal with responsive control and explicit forwarding of a verified
copy after the original creator VM is OFF. This is manual, consented copy/forward,
not automatic replica discovery or a durable availability guarantee. Failure is
null and cleanup errors are empty. The following 66-scenario paired-node replay
also passes with the SAME binary hash:
`lab/tmp/kk-results-cb670f32c074-1793007883203103868.json`. All guests/network stopped.

Subsequent source begins native media: fixed Pulse/V4L2/local-X11 source builders,
Opus/VP8 RTP and RFC4571 framing, bounded supervised GStreamer children, continuous
bounded diagnostic drain, Linux process memory limits and explicit local preview
consent. Four targeted media tests and CLI refusal tests pass; native codec tests
use synthetic sources and do not claim physical capture. `kk media doctor` reports
all fourteen required plugins present locally. A private virtual-display capture
gate is being verified. These new media files are NOT covered by the earlier VM
binary or the preceding full 133-test host run. Current details and primary sources
are in `media.md`. Authenticated room media/pacing, portals, physical devices,
Windows/macOS, GUI, recovery/address lifecycle and final publication remain open.

## Public CLI VM Gate And Room Files (2026-10-26)

All NINE public room CLI scenarios PASS on three independent guests:
`lab/tmp/public-room-cli-d14a1cb102f4.json`, release CLI SHA-256
`fe8f75c645aa7d2cd85917f80f28719d5f5cc1d6fdd464dc768d34f5993d1598`.
This proves actual user commands, not fixture operations: two network-only joins,
three broadcasts, sensitive stdin/output redaction, exclusive foreground ownership,
explicit voter selection, cold resume, separate creator vote/data retirement and
continued survivor communication while the actual creator VM is OFF. Failure is
null and cleanup errors are empty; dedicated guests/network were stopped.

The subsequent file slice reuses the existing verified-prefix stream and storage
protocol, including no-clobber, identity-bound partials and complete SHA-256 checks.
Room file acceptance is explicit per foreground serve/join. File recipients are
selected explicitly, not automatically broadcast to the whole room. Async guards
check quorum-current membership, publisher rights and local Rooms/Files vetoes at
the existing authorization boundaries. Shared inbox offer/budget accounting now
has one owner reused by paired nodes and rooms; file admission releases control
capacity before waiting for actor/quorum work.

The first integration run exposed a real framing deadlock: control frames waited
for FIN, but a file handshake intentionally continues with binary data. The reader
now separates bounded length-prefixed parsing from the required control-message
FIN check. Only the explicitly typed file channel continues past the prefix.
A later test incorrectly assumed complete output retained partial sensitivity
metadata; existing protocol semantics instead reverify completed content without
rewriting it. The test now checks actual semantics: sensitive receipt redaction
does not retroactively erase an earlier ordinary file. Both failed runs remain
session provenance, not passing evidence.

Full host make check now PASSES 133 Rust tests (68 library, 62 integration, three
fixture), formatting, warning-denied Clippy and rustdoc. The real CLI integration
includes no-consent refusal, binary room-file delivery, complete-prefix repeat,
sensitive receipts and content preservation. Reader file-publish denial and reader
file-receive permission are exercised by backend enrollment tests. All 21 static
lab checks pass. The extended public-CLI VM gate and 66-scenario paired-node replay
are running for this new slice; the older nine-case report is not its proof.

Native media research verified installed GStreamer RTP RFC4571 framing, Opus and
VP8 plugins and upstream portal lifecycle requirements. No user microphone, camera
or desktop was captured. Capture permissions, native devices, bounded live media,
GUI, platform releases and the final adversarial review remain required.

## Public Room Commands Integrated (2026-10-26)

User-facing `kk room` commands now invoke the existing SDK, control service and
scoped enrollment backend directly. The private profile manifest contains public
bootstrap data only; keys remain in the recipient's own MLS store. Creation and
join reserve new directories exclusively; resume never resets keys or authority.
The node's existing private socket/UID/length/lock checks are shared with room IPC.
Long command and single-invitation lanes are bounded independently of status/stop.

Two new real CLI-process tests PASS: three independent profiles create/serve,
enroll two administrative members without votes, broadcast to both, select voters,
stop/resume on the same port and continue; invalid names, existing state and missing
profile identity refuse without replacing retained history. Clippy with warnings
denied and all 21 static lab tests pass. `make lab-room-cli-test` is running for
this slice; no fixture gate is called public CLI evidence.

The previously pending wire-role hardening replay completed: fourteen scenarios
in `lab/tmp/network-enrollment-1c7bc19fb04b.json`, helper SHA-256
`fe7ddd8c5add29cf3eea3c8ceea7787137eb3e04dadf072ce26d40ca51216d39`;
all 66 baseline scenarios in
`lab/tmp/kk-results-cee650a3a082-1793003954650761105.json`, CLI SHA-256
`4c470eaf37c7eed9c47109be7a7dbf5909290324765012d7fd9187eee7e6177f`.
All dedicated guests/network were subsequently stopped. These artifact versions
precede the public room CLI changes and are not their proof.

The operator reaffirmed end-to-end autonomous execution and requires a dedicated
deep adversarial review after functional integration, even for already passing
boundaries. This is recorded in TODO.md. Room files/replicas, native capture and
media, GUI, platforms/distribution, incomplete-enrollment recovery and authenticated
address changes remain open. No final publication or whole-product closure.

## Explicit Controller Approval And Creator Retirement (2026-10-26)

Controller selection is now an EXPLICIT signed operation, never an automatic
consequence of data admission or possession of a TLS certificate. It requires the
existing administrative ceiling (invite, remove AND delegate), a bounded nonempty
exact target set of admitted/configured peers, room binding, sender-key proof,
nonce and prior configuration log ID. The leader serializes authorization with
application writes. It uses official OpenRaft change_membership and completes the
joint-to-uniform transition. Identical target reconciliation is allowed; a stale
different transition is a conflict. Demoted voters remain metadata learners by
explicit policy; their data membership is a separate operation.

The three-guest gate passes all FOURTEEN scenarios on that slice:
`lab/tmp/network-enrollment-e74860bdd0e6.json`, helper SHA-256
`9b9cc2c55a7e5781264581365568a418fceeef61f6dbbdffda62168ae0b139bf`.
This includes nonleader administrative approval, participant refusal even after
receiving a vote, an actual creator VM OFF with surviving voting majority, explicit
creator vote retirement, delegated data removal and cold creator refusal. Primary
and cleanup errors are empty. The following 66-scenario paired-node regression
also passes: `lab/tmp/kk-results-56e64fcbfe26-1793001445207792655.json`, CLI SHA-256
`5a9a0381f8ec434e6c45338ed5ec195f8e113312c1190c6d80cd93eb2e94463e`.
Guests/network were stopped. These reports precede the following wire-role repair.

Official OpenRaft source inspection confirms it assumes controller trust; the
application transport must not pass every pinned DATA/learner peer's raw vote or
claimed-leader request to the engine. Incoming vote/append/snapshot dispatch now
checks actual voter eligibility (including joint configurations). Empty bootstrap
state accepts only independently approved controller seeds, which network
enrollment now restricts to actual voters, not arbitrary learners. Bootstrap votes
are needed before first configuration replication; an unconfigured node's response
is counted only if the actual candidate configuration selected it. A known data
certificate alone cannot inject high terms, committed-leader claims or snapshots.
The existing claim-to-be-another-candidate check remains in place.

Full host make check passes 131 Rust tests, formatting, Clippy and rustdoc; lab
checks pass 21 tests. A real pinned-TLS negative test checks all three forbidden
RPC kinds leave the controller term unchanged. Bootstrap/controller and scoped
enrollment tests pass. A new fourteen-case VM and full paired-node replay for this
wire-role change is running; earlier success is not claimed as its proof.

An offline node whose ONLY known controllers were all replaced may require explicit
verified rejoin rather than accepting an unknown leader by address. This fails
closed and is not a Byzantine-consensus guarantee. Room CLI/IPC, authenticated
address updates, remaining partition/concurrent/rejoin cases, full snapshot-budget
audit, replicas, native media, GUI/platform verification and publication remain
open. No commit, push or final product completion is claimed.

## Owned Transport Termination Verified In VMs (2026-10-26)

The nine-case three-guest enrollment gate now PASSES INCLUDING process teardown:
`lab/tmp/network-enrollment-553601b57fc8.json`, helper SHA-256
`510091685665bf9b37361611b8217b5a2e77b6f97a6d5e3b4e9de4bc075c15bf`.
Primary failure is null and cleanup errors are empty. The following lab shutdown
stopped all three guests/network. Earlier cleanup failures remain below as history.

The verified repair gives the node actual ownership of its transport children,
not merely a task counter. QUIC task creation sites/abort handles are tracked;
after protocol/storage/server work ends, peer notification has a separate bounded
one-second grace. The owner then terminates and joins remaining transport drivers,
including late spawns, without waiting indefinitely on a remote drain timer.
No application SQLite/MLS transaction is aborted by this transport scope.
This is not a larger timeout or a claim that remote effects can be rolled back.

Full host validation passes 128 Rust tests, Clippy, formatting and rustdoc, plus
21 lab checks. The old raw Raft transport test also handles explicit election
redirects with the exact signed operation inside its original deadline; observing
a leader ID is not treated as a leadership lease. No warnings remain in the last
full check. This closes the observed shutdown blocker on the recorded host/VM
slice, not every imaginable network schedule or the unfinished product gates.

Controller/voter authorization and lifecycle, room CLI/IPC/address handling,
concurrent/partition/rejoin cases, file replicas, media, GUI/platform verification
and final signed publication remain required. No commit/push/publication occurred.

## Detached Replication Client Ownership Repair (2026-10-26)

The first three-guest forwarded-enrollment replay completes all NINE behavioral
cases but FAILS its teardown: `lab/tmp/network-enrollment-f4e7c792e956.json`.
The failed cleanup is not waived or counted as a passed gate. The report preserves
the socket-driver timeout; the final lab shutdown stopped all guests/network.

Inspection of official OpenRaft/Quinn sources and a new regression identifies a
second lifetime boundary: OpenRaft may retain a detached/backoff network Client
after its core has shut down. Such clients now hold weak endpoint ownership;
bounded active RPCs temporarily borrow it, and a closing flag rejects new work.
They cannot keep the listener/private node alive merely by remaining in backoff.
The regression intentionally keeps a client alive after owner shutdown, awaits
the actual QUIC drivers and rebinds the exact same UDP address successfully.

Targeted forwarding/cold-scope tests and full make check pass: 127 Rust tests,
formatting/Clippy/rustdoc, with 21 lab checks. The actual three-guest nine-case gate
is running again, followed by unconditional lab shutdown. The VM cleanup outcome
is still unverified; host success alone does not close the failed VM teardown.
Voting authorization/lifecycle and the later room/product gates remain open.

## Signed Nonleader Learner Intents And Recoverable Transport Scope (2026-10-26)

Learner provisioning now routes from an admitted inviter through the elected
controller using an exact device-signed request. The signature binds room,
author, target, address hint, nonce and prior configuration log ID. The leader
checks the TLS sender, current invite rights, target admission and configuration
precondition. A matching already-configured learner can be reconciled; stale
different intent is a conflict, not silent replacement. No request grants a vote.

Signed application writes and learner intents share one leader-side serialization
gate; role/removal writes cannot race authorization before the configuration call.
Append/vote/snapshot RPCs never acquire this gate. Role/scope/signature/target
tampering and issuance by an unprivileged or removed member are refused in tests.
Existing official OpenRaft add_learner remains the sole replication mechanism.

Controller Node metadata now carries its bounded PUBLIC certificate pin, bound to
the complete node fingerprint. High-level initialization/provisioning fills it;
missing pins are allowed only in low-level/preapproved storage fixtures, never
promoted from an unknown address. Protected snapshots validate nonempty pins before
replacement. This makes approved metadata-only peers recoverable even after leaving
the DATA group and after log compaction, rather than relying on a second trust DB.

A bounded committed-state watcher derives room-scoped TLS roots after apply or
snapshot COMMIT, preserving independent data-member and metadata-node eligibility.
This never creates global contacts. Explicit local roster replacement remains an
override/veto and is not silently widened by the watcher. The transport union has
its own bounded peer count; application membership remains limited independently.

Cold-restart tests found two real lifecycle defects: cancelling an async scope
reader did not cancel its blocking SQLite work/lease, and draining connections did
not mean Quinn's background UDP endpoint driver had exited. Shutdown now signals
the scope worker and awaits its bounded work, drains server/stream children, closes
connections, drops endpoint owners and awaits the actual tracked QUIC drivers.
The safe adapter delegates Quinn's existing Tokio Runtime; no crypto implementation,
unsafe code, sleep/poll workaround or extra background service was introduced.

Full make check passes 126 Rust tests, formatting, Clippy and rustdoc; lab checks
pass 21 tests. A real loopback test enrolls a third peer through a NONLEADER
moderator, forwards the signed intent, confirms unchanged voters, receives private
data, removes that peer from data membership, and restarts the inviter on the SAME
UDP port with its ORIGINAL configuration. Committed metadata recovers the scoped
pin, metadata read remains possible and old group data is denied; valid current
data and zero global contact insertion are checked. Cached metric publication is
explicitly not treated as recipient application or freshness authority.

The network enrollment VM driver now uses all three guests: Root enrolls a
moderator as a learner, that nonleader enrolls a participant as another learner,
and its cold restart must recover trust without host-supplied new pins. This new
gate is RUNNING; no cross-VM success is claimed yet. Voting changes, explicit
controller-approval policy, room CLI/IPC, remaining fault/rejoin cases, replicas,
native media, GUI and signed platform distribution remain open. No commits/push.

## Network-Only Enrollment VM Gate Passed (2026-10-26)

The new two-guest enrollment gate passes all five scenarios:
`lab/tmp/network-enrollment-873e0dac70da.json`, helper SHA-256
`14f61a1f1cc745071ae222e8b148ee8abea853b86e254313abf33e1e4b67611d`.
The report has no primary or cleanup errors; all three lab guests/network stopped.
Only the intended one-time invitation was delivered out of band. Authority,
grants, signed seed hints, key package and control state travelled over the proved
network path; the recipient generated/reopened its own private state and exchanged
real empty/maximum/sensitive QUIC data without global contact insertion.

The source is one voter and the admitted peer a NON-VOTING learner. This is not
multi-voter reconfiguration/failover proof, nonleader admission forwarding or a
complete product room CLI. Host checks pass 124 Rust/21 lab tests. The older eleven
private and 66 paired-node reports predate this schema/session slice; their full
new-source regression remains to run. No source has been committed/published yet.

## Proved-Connection Enrollment And Non-Voting Learners (2026-10-26)

The prior scoped-bootstrap slice passes eleven actual private cross-guest
scenarios, including room-scoped PAKE/key proof and zero automatic global
contacts: `lab/tmp/cross-guest-private-room-956dfcf4a434.json`, helper SHA-256
`c36b8e77338dd561f7c3904e65d99071c8e30a0acc56e244b68af5188d3af167`.
Its following paired-node regression passes all 66 cases:
`lab/tmp/kk-results-4a2801816296-1792985091680363881.json`, CLI SHA-256
`c556ea8c59e7c7ac7414d959a389985f7f70aa0f9e9db8a6499f35f6bbe100b0`.
Cleanup completed. Those artifacts predate the following new enrollment code.

RoomSession now retains the SAME proved/pinned TLS connection after scoped
approval. Two ordered, role-bound stages carry authority (512 KiB) and key package
(1 MiB), with exact room/stage/length framing, original ceremony expiry and bounded
deadlines. Wrong role/order/room, extra stage, malformed payload, expiry or caller
cancellation fail closed; interrupted stages cannot silently retry. The original
OPAQUE 16 KiB/eight-frame limits and contact close/commit path are unchanged.
No second PAKE, TLS verifier or group encryption scheme was added.

Backend enrollment authority binds the approved room digest, original public
creator proof, current inviter credential/ceiling, exact recipient/accepted rights
and scoped controller hints. Existing device signatures bind the complete bounded
body digest, so replacing seed addresses/certificates is not accepted. The runtime
checks fresh private/public agreement and local profile veto before issuing it.
Only the recipient creates its private candidate/signing/init keys locally.

Official OpenRaft add_learner now provisions an ALREADY committed member, with a
current local leader/inviter gate, explicit address hint, bounds and updated scoped
TLS roots. It never grants a vote. Existing approved metadata nodes are retained
separately from data membership; no global contact entry is created. This primitive
is local-leader-side; authorized forwarding, voting changes and full lifecycle
reconciliation remain OPEN, not implicitly solved by adding a learner.

Public control state schema v2 retains immutable PUBLIC genesis origin through
creator departure/compaction. It exports no private signing/init/ratchet material.
Unknown schema, inconsistent applied position and invalid origin are rejected
before snapshot database replacement. Old unreleased control states without this
proof are NOT silently repaired/migrated; private Group state is not regenerated.

Library enrollment admit/join functions own protocol/policy orchestration, not UI
callbacks. A source result means committed/replicated admission, not claimed
remote activation or voting privilege. The recipient verifies its own protected
admission, commits private join, catches up and checks policy before returning a
caller-owned Room/control pair. Errors after private COMMIT require reopening
existing state; timeout/cancellation never imply atomic remote rollback.

Full make check passes 124 Rust tests, formatting, Clippy and rustdoc; lab checks
pass 21. Real loopback tests transfer root/grants/seeds/package over this channel,
add a previously uninitialized learner with updated TLS trust, join via its own
replicated record, preserve reader restrictions and keep global contacts empty.
Wrong room/issuer/subject/rights/package identity and tampered hints are refused.
Delegated grant verification after creator departure and read-only issuance refusal
are tested. These are NOT yet real cross-VM enrollment proof.

make lab-enrollment-test is running that new proof on two independent guests.
ONLY the intended one-time invitation goes out-of-band; genesis/grants/seeds/key
package/control bootstrap may not be supplied by the host. The recipient saves
only received/approved public configuration, explicitly reopens its own private
state, and must exchange real private QUIC data. The source has one initial vote;
the admitted participant remains a learner. This does not claim voter failover.
Full room CLI/IPC, voter policy/lifecycle, further faults, file replicas, media,
GUI/platform verification and final signed publication remain required.

## Purpose-Bound OPAQUE Bootstrap Added (2026-10-26)

Pairing now distinguishes `Scope::Contact` from an exact `Scope::Room(id)`.
The room ID is bound into the existing official OPAQUE context, derived-channel
transcript, encrypted metadata, confirmation and TLS exporter binding. Contact-v1
context/metadata/bundle encoding is preserved (no serialized null room field).
Room invitations use a distinct header and version-2 private bundle/QR carrying
the exact room ID. Unknown or mismatched purpose/version fails before commitment.

The existing network ceremony remains ONE implementation: OPAQUE, encrypted
metadata, explicit approval on both sides, pinned mutual-TLS private-key proof
and both completion receipts. Contact entrypoints accept only a Contact callback;
room entrypoints accept only a typed RoomPeer callback. RoomPeer proves device
possession and intended scope, NOT MLS membership, controller rights or global
contact trust. Room mode cannot carry contact permission grants. No limits were
inflated and no replacement crypto/bootstrap transport was introduced.

Full `make check` passes 117 Rust tests, Clippy, formatting and rustdoc. Tests cover
cross-room/contact code confusion, scoped channel derivation, denial/store failure,
room QR roundtrip, zero global contact insertion and an attacker holding the code
but not the claimed certificate key, for BOTH purposes. The old contact CLI tests
pass. Actual old-binary interoperability was not separately exercised.

The VM adapter adds scoped pairing commands with callbacks writing only scoped
approval artifacts. A first new VM run failed in its driver before completing
the ceremony: `lab/tmp/cross-guest-private-room-b7d3f6760b6b.json` called a
nonexistent Guest.download method. Cleanup completed. That wiring is fixed using
a PURPOSE-SPECIFIC, path-allowlisted, symlink-refusing, 4096-byte-bounded ephemeral
test-invitation transfer, not an arbitrary private-file/key reader. The invitation
is intended out-of-band input and never printed/logged; error output is redacted.
Lab checks now pass 21 tests including transfer bounds/path/error regressions.

A new actual eleven-scenario private VM gate (scoped ceremony plus the prior ten)
and full paired-node regression are running, followed by lab shutdown. No scoped
cross-VM success is claimed yet. Public authority/genesis/grants and the fixed
control roster remain explicitly preapproved in this fixture. Full enrollment,
post-proof root/grant/control-state delivery, learner/voter lifecycle and product
CLI/IPC still require implementation; purpose-bound authentication does not by
itself close those gates. Later replicas/media/GUI/distribution remain required.

## Transport Recovery Replay And Full Regression Passed (2026-10-26)

After selective cache retirement, the private VM gate again passes all TEN
scenarios, including post-reboot data, sender-generation continuity, old-ciphertext
refusal and removed-creator recovery. Evidence:
`lab/tmp/cross-guest-private-room-225dadeed72d.json`; helper SHA-256
`8cdf0ae961ee191490e4792e718791ea69c1905f43c129df6800bebca95d3699`.
Both primary failure and cleanup errors are empty.

The subsequent full paired-node regression passes all 66 scenarios on the same
working tree, including shaped/lost links, files/resume, sensitive I/O, pairing,
live revocation, private IPC and persistent-node crash/restart. Evidence:
`lab/tmp/kk-results-23a8686f7895-1792981551627974054.json`; CLI SHA-256
`6de959d0366e2f8f7fb32443e0dcea40e748fdec3cdde0de5ceaaa612ce0eace`.
All three guests/network were stopped successfully. No terminal job remains.

Host checks pass 111 Rust tests and 19 static/control-adapter tests, formatting,
Clippy and rustdoc. This verifies the current repair slice; the earlier timeout
report remains provenance, not the latest outcome. It does not prove every possible
reconnect/partition schedule or waive the remaining scoped invitation, voter
lifecycle, CLI/IPC, concurrent/rejoin, replicas, media, GUI/platform and publication
gates. No source checkpoint has been committed or published during this pass.

## Ten Private Scenarios And Cancellation/Cache Isolation (2026-10-26)

The initial private gate passes all eight scenarios in
`lab/tmp/cross-guest-private-room-4e2e00d53a3c.json` (helper SHA-256
`37d1f05a56234d39997bafa36e85c8677a9def3bce5fe768dea182c7017d06d9`).
The expanded gate passes TEN scenarios in
`lab/tmp/cross-guest-private-room-cb062ed6d2c2.json` (helper SHA-256
`2b24b4a8ecd60408583c6c63958bb70dc448b80ad892ed4e834359065d12c994`).
Both reports have empty primary/cleanup errors; guests/network were stopped.
The extra proof rejects an actual pre-crash ciphertext after receiver restart,
accepts a fresh frame on the same live authenticated link, and detects sender
generation reuse by sending again after restart to a receiver with durable state.
The fixture holds at most ONE explicitly captured encrypted frame in RAM; replay
is an explicit adversarial test command, never automatic sensitive-message retry.

Private actor status now uses one bounded watch snapshot of already-public durable
metadata rather than waiting behind quorum work. It is explicitly cached, never a
freshness/authorization claim. Startup readiness is separately awaited. Dropping
a caller cancels work at its next async boundary; it does NOT undo prior COMMIT
or imply no remote effect. A deterministic no-initialization/no-quorum test checks
20 ms cached-status access and cancellation completion within 500 ms instead of
waiting the original five-second operation deadline. These are test bounds, not
a universal performance claim. Private keys remain on the owning local task.

A transport regression demonstrated that cancelling one RPC closed its entire
shared peer connection. The RPC guard now resets only its streams. Actual peer
authentication/revocation still owns peer-wide closure. Three handshake sites
also reuse the existing central QUIC error classifier: timeout/reset are not
reported as bad certificates; TLS alert/pinned-key failures remain hard failures.

A later full replay on that transport slice timed out on the first message after
Mint reboot: `lab/tmp/cross-guest-private-room-953f6a32e1ed.json`. It did NOT pass;
the following full node regression was therefore not executed. A second targeted
transport regression confirmed that nonresponsive connections remained cached.
The cache owner now retires the matching suspect entry without closing independent
holders or evicting a newer connection, so later attempts can reauthenticate.
The specific VM timeout's complete cause is still under verification. Fixture
errors now report parse/encrypt/QUIC phases without plaintext to narrow any replay
failure. No test-only blind retry or enlarged application deadline was introduced.

Full `make check` passes 111 Rust tests, Clippy, rustdoc and formatting; lab checks
pass 19 tests. A new actual private gate followed by the full paired-node VM
regression is running, with unconditional lab shutdown afterward. Earlier passing
artifacts are not claimed as proof of these newer cache/transport changes.
Scoped PAKE, authorized learner/voter lifecycle, room CLI/IPC, concurrent/partition
cases, file replicas, native media, GUI and signed portable publication remain open.

## Six Private Cross-Guest Scenarios; Guest Service Boot Race (2026-10-26)

The atomic-mailbox replay reaches six actual private scenarios successfully:
replica-based join, private QUIC text, creator VM OFF while a surviving moderator
issues its own grant/admission, both survivor directions (including empty/maximum
text), profile veto until removal/new epoch, and minority refusal before new data.
Evidence: `lab/tmp/cross-guest-private-room-5807f08cff9c.json`.
It then FAILS during Mint's reboot, before private restart/cold-return verification:
the guest agent was ready while NetworkManager was not running. This is partial
evidence, not a passed eight-scenario private gate. Shutdown completed afterward.

The shared guest network owner now starts the EXISTING guest NetworkManager unit
and waits for service activation before selecting its MAC-matched device and
assigning the volatile isolated address. It neither installs/enables a service
nor changes host/global policy. Service failure stops before address mutation;
unexpected guest identities still fail before any service/network action. Lab
checks pass 19 tests, including the new ordering/failure regression. Rust checks
remain 108 tests plus formatting/Clippy/rustdoc from the unchanged Rust slice.
The full private gate has been relaunched, with automatic guest/network cleanup.

## Private VM Mailbox Publication Race Fixed (2026-10-26)

The first private VM run failed before completing a scenario:
`lab/tmp/cross-guest-private-room-099a233f9a87.json`. Admission and all graceful
stop commands reported `Protocol`; guest shutdown still completed. The driver
published request names with touch/chmod before uploading complete JSON, while
the foreground process used existence as its completion signal. Response files
had the inverse create/write visibility gap. This was a lab mailbox race, not a
successful admission/data proof, and the failed evidence is retained.

Requests now upload to an ignored staging name and become visible by same-directory
atomic rename. Responses publish complete temporary JSON with no-clobber atomic
publication; replacement remains explicit only for observational status. Tests
cover the valid request schema, old-instance refusal, complete immutable results,
explicit status replacement and the driver's staging/rename sequence.
Full `make check` passes 108 Rust tests with Clippy/rustdoc/formatting, and lab
checks pass 18 tests. The corrected actual private VM gate is running; its result
is not yet available. No private room gate is closed by these local regressions.

## First Actual Private Room VM Gate Launched (2026-10-26)

`make lab-room-test` now drives the real `serve-room` processes through
`lab/scripts/09-room-test.py`, with the same helper hash verified on each guest.
Public control startup, file IO/status and guest lifecycle helpers are reused;
there is no second crypto or TLS implementation in the driver.

Scenarios target committed-replica private join, actual sensitive QUIC text,
creator-VM loss, a surviving moderator issuing its own grant/admitting a third
member, both data directions including empty/maximum payload, profile veto until
removal/new epoch, minority refusal, private restart and a removed creator's cold
return. Key packages/anchors/grants use an explicitly preapproved bootstrap over
trusted virtio management. Welcome ciphertext arrives through control replication
and recipient-authenticated retrieval; application data goes over actual QUIC.
This fixture does NOT close scoped PAKE invitations or dynamic voter membership.

The first run is IN PROGRESS; no private cross-guest success is claimed yet.
Its command is followed by `make lab-stop` even on failure. Failed runs retain
their primary and teardown diagnostics. Further replay/generation-after-restart,
concurrent admission, partition/rejoin and status responsiveness proof remain
required beyond this initial scenario set. Local checks still pass 107 Rust and
17 lab tests; the optimized private helper builds successfully.

## Native Recovery Passed; Recipient Delivery And Private VM Runner (2026-10-26)

The routing/atomic-Welcome slice passes all seven public cross-guest scenarios:
`lab/tmp/cross-guest-control-945eb3169d72.json`, helper SHA-256
`0ba5e4b76959bfd8976d09890a24a262de963ac72d1d393a846b58f478c90359`.
It also passes 38 native room-library tests plus the isolated crash executable
on EACH of the three guests. Evidence:
`lab/tmp/native-crypto-1792973123878109108.json`; library SHA-256
`6d9c814c9c2dc9889f5cf526c9cf64235daaceb9007d9ca8312ae8b365a4d30e`,
crash SHA-256 `089a80de5cb66f754d738e49d21d20bf319a6aa3133ef78d4274fe6221a94d60`.
This closes the failed Mint replay for THAT slice, not new untested code. Cleanup
completed and all guests/network were stopped. No terminal test is still running.

Subsequent source adds recipient-authenticated Welcome RPC. The TLS sender alone
selects its artifact; no request-supplied recipient is accepted. A fresh quorum
read precedes lookup, and the receiver matches the remote hint/ciphertext against
its own fresh protected admission record before private join. Lookup works after
genesis eviction if the actual admission remains retained; a missing admission
bridge is an explicit recovery/rejoin error, never fabricated authority.
`Room::join` checks owner/room, approved original author and local room policy,
commits SDK join, then catches up and rechecks policy before reporting activation.
After a late error, reopen existing membership; never regenerate init-key state.

The lab example now has explicit `genesis-room` / `serve-room` modes, factored
public control startup/status helpers, and a separate `room_fixture.rs` adapter.
Its creator identity matches a persisted real private Group. Candidate and member
startup are explicit, with no restore-or-create fallback. Synthetic text is
generated inside the guest and sent through actual QUIC; receive evidence is
hash/length/sensitive flag/sender only. Bounded file commands require the current
random PUBLIC process-instance ID, preventing automatic replay of old pending
sends across PID reuse/restart. This remains internal test machinery, not a room
product command or a scoped invitation implementation.

Local `make check` passes 107 Rust tests including the example's genesis/restart
guards, formatting, Clippy and rustdoc. The private foreground VM adapter compiles,
but its actual multi-guest Python driver and fault replay are still to implement.
Do not substitute another per-OS-only run for that next gate.

Full document reread completed: CONTRACT, README, CHANGELOG, TODO, all ten docs
files and lab README. The current-versus-historical capability matrix is now at
the top of `backend-map.md`. A whole-project implementation/security audit is
still pending; document review is not that audit. Room CLI/IPC, scoped bootstrap,
voter lifecycle, replica consent, native media, GUI and final signed distribution
remain required. Cached private status/cancel responsiveness under quorum stalls
also needs explicit proof before frontend integration.

## Election Routing Fix And Atomic Encrypted Welcome Replication (2026-10-26)

Public inter-VM control again passes seven scenarios:
`lab/tmp/cross-guest-control-143417ca1df7.json`.
The subsequent native matrix did NOT pass: Fedora passed 37 library tests and
the crash test; Mint failed the private runtime's initial fresh-view operation
with `Network`. The runner stopped before testing the third guest. Evidence:
`lab/tmp/native-crypto-1792971976708890642.json`. Cleanup now completes correctly,
including the observed guest-shutdown RPC disconnect followed by confirmed stop.

The production route owner previously returned immediately for a temporary
no-leader/unavailable response. It now retries transient network/election states
inside the original deadline with backoff and bounded known-peer redirect chains.
The signed proposal/nonce never changes. Authentication, authorization and other
hard failures still propagate. A deterministic regression starts the fresh read
BEFORE Raft initialization, then initializes later; the request must wait and
succeed rather than relying on a larger VM startup timeout. Mint still needs its
new replay result before this native gate can be marked passed.

Encrypted MLS Welcome is now carried in the exact signed admission and committed
atomically into bounded replicated public state, not copied through a second
post-commit replication protocol. The unpublished control wire/domain is v2 and
binds commit, key package and encrypted Welcome together. No private MLS database,
plaintext epoch key or private signer is replicated. Existing low-level MLS
prepare/confirm primitives still withhold recipient delivery until acceptance.
Prepared envelopes are for authorized control replication, NOT join responses.

The public state retains at most one artifact per current member, bounded by
1 MiB aggregate raw ciphertext and the existing 4 MiB serialized snapshot cap.
Removal drops that recipient's artifact. Prospective state is size-checked before
mutating membership or delivery, so a quota failure cannot leave a partial add.
Applied records now bind proposal ID and admitted recipient; private join checks
the exact recipient in addition to epoch/audience/author. Tests cover ciphertext
tampering, signature binding, quota rollback, snapshot restore and private join
from committed ciphertext while the private producer is closed before confirming.
The three-member loopback test obtains Welcome from a real follower's public
replica, not the producer's local outbox.

Full `make check` passes 104 Rust tests, formatting, Clippy and rustdoc; lab checks
pass 17 tests. A new public-control/native VM replay is running. This does not yet
prove private admission/data across guests or creator-VM loss during delivery.
Next: authenticated current-recipient artifact delivery, actual private VM
foreground processes and fault scenarios; then scoped bootstrap, voter lifecycle,
files/replicas, media, GUI and final signed portable publication. None is waived.

## Thread-Confined Private Runtime And Quorum Freshness (2026-10-26)

The applied-record slice passes its actual cross-guest public-control replay:
`lab/tmp/cross-guest-control-14e77878c578.json`, seven scenarios, helper SHA-256
`4a930f401c1b3f4d78a17451d584baf46ffee5558a7333ddeb0907a76e26ec95`.
The native matrix passes 35 room-library tests plus the isolated crash executable
on each guest: `lab/tmp/native-crypto-1792970237422630803.json`.
Library SHA-256 `44b96bc79afa5cdc41a126529f073eded0d86576bd14bbbfc7d18a120deb1618`;
crash SHA-256 `47032ef4d6fca9497e64b1fa96d39d68210c1faedbc2f8fcb7b887816d9673fd`.

Teardown exposed a guest-shutdown RPC disconnect although all domains actually
stopped. Shared shutdown handling now requests every stop, waits for confirmed
domain state, reports resolved RPC errors and refuses forced destruction. It
does not classify an RPC error alone as either successful stop or failed stop.
All guests are stopped and `make lab-stop` subsequently completed. Lab checks
now pass 17 tests, including disconnect, continued cleanup and bounded failure.

The next working slice adds `rooms/runtime.rs` and its explicit local actor.
Private MLS stays !Send; Send handles use 64-command/64-event queues, absolute
expiry and cancelled-request suppression. Incoming events reserve capacity before
decrypting, so backpressure cannot consume replay state and discard the event.
There is no implicit daemon, plaintext history or automatic global contact import.

Fresh control uses OpenRaft's actual linearizable quorum read, authenticated
redirects and local application before reading the record/view pair atomically.
An isolated former leader fails the bounded freshness operation. The runtime
checks live local room/text policy for the entire outbound epoch audience, and
binds an inbound MLS sender to its direct TLS peer before private COMMIT.
The exact signed proposal/nonce is now persisted in the same transaction as its
pending MLS commit; restart reuses identical bytes, missing nonce state fails
closed, and real COMMIT failure releases neither a replacement nonce nor Welcome.

Actual QUIC room-text RPC now reaches the local private actor through a bounded
handle. An ACK hashes ciphertext after durable replay processing and event-queue
acceptance, not human reading. Data has separate admission (four RPCs) and releases
the control admission permit before waiting on private processing/quorum reads.
This avoids holding all control slots hostage to data operations needing control.

Full `make check` passes 103 Rust tests, Clippy, rustdoc and formatting. The local
runtime test covers three private members, joins/removal, group text, identity
mismatch, no global contact import, profile revocation, bounded actor queues and
actual loopback QUIC delivery/backpressure. These are NOT yet cross-VM PRIVATE
room admission/data, replicated Welcome delivery or the final room CLI.
The earlier VM/native reports predate this runtime/freshness/room-text slice.
Scoped bootstrap, replicated Welcome durability, dynamic voter lifecycle,
full private cross-guest fault scenarios, files/replicas, media, GUI and signed
portable publication remain open and explicitly authorized.

## Public VM Recovery And Private Applied Records (2026-10-26)

The actual three-VM public-control gate passes all seven scenarios, including
creator-VM loss, delegated control, minority refusal, restored quorum resolving
the exact uncertain operation, and cold creator catch-up. Evidence:
`lab/tmp/cross-guest-control-4a492d0d4560.json`, helper SHA-256
`7577443f218d9bcf0a35473e46deaf0d0a09ab5c8fa39045f2f616e255b0afaa`.
Both primary failure and teardown errors are empty. Guests/network were stopped.

The boot failure was a lifecycle bug, not simply a slow DHCP client: setup disabled
NetworkManager and assigned volatile addresses. Shared virtio-controlled network
configuration now restores that state after cold boots and verifies absence of
IPv4/IPv6 default routes. Static lab validation passes 14 tests.

The next unpublished slice adds a bounded protected LOCAL applied-record journal
(64 records / 8 MiB), complete log IDs and chained record hashes. It retains the
logical audience at each actual record, not the historical epoch in a replay
receipt. Public snapshots deliberately do not pretend to contain private MLS
history; an evicted/missing bridge fails closed and requires verified rejoin.

Private join, membership changes and applied position now share the crypto
transaction. Blank, rejected and replay entries advance only the cursor. Tests
cover skips/rewinds, historical receipts, reopen, real SQLite COMMIT failure,
withheld Welcome/outbox, rejected join rollback and original MLS author binding.
Full `make check` passes 101 Rust tests, Clippy, formatting and rustdoc.
The updated code is being replayed through public cross-VM and native per-OS
gates. The earlier successful report predates this applied-journal addition.

Remaining room work: thread-confined live private actor, authenticated scoped
bootstrap, durable Welcome replication/delivery, actual cross-VM private room
data, voter lifecycle, fresh-view/profile veto enforcement and complete fault
scenarios. Media, GUI and portable signed publication remain authorized and open.

## Cross-Guest Recovery Harness Correction (2026-10-25)

The first public-control VM run reached election, exact retry, creator loss,
delegated control and minority refusal, but failed at quorum restoration. The
restarted helper exited with `Cannot assign requested address`: guest-agent
connectivity alone did not establish readiness of its configured network address.
The timeout was subsequently masked by a teardown exception.

The boot gate now waits, with a deadline, for the address in the canonical lab
roster. Foreground startup detects early exits rather than waiting for a leader
that cannot appear. Teardown attempts every process, records all errors and
preserves the primary failure; evidence is written even when teardown fails.
`make lab-check` passes 11 tests including delayed address assignment and bounded
boot failure. Full `make check` passes 97 Rust tests, formatting, Clippy and rustdoc.
The cross-VM gate is being replayed; these local checks do not close it.

## Completed Native Service Replay (2026-10-25)

- Latest full `make check` passes 97 Rust tests. The expanded native matrix passes
  31 library room tests plus the isolated process-crash executable on EACH guest.
  This includes foreground public-service resume, real TLS redirects and exact
  historical receipts, without pretending these are private room/network readiness.
- Evidence: `lab/tmp/native-crypto-1792964978997141581.json` (format version 2).
  Library SHA-256 `124503602449fcfaeb7bc63d61bce819e22360a02564ba8140433b8c725c92c9`;
  crash SHA-256 `fabc960972e2222271092d3a678163be397846d7a523ddfc8901b5f8f49e7980`.
- The job has FINISHED; all three guests and the isolated network were stopped.
  No terminal job is pending. Next code must expose the already implemented SDK
  as actual foreground processes across the VM bridge and wire private applied
  records/scoped joins, not repeat per-OS-only proof indefinitely.

## Foreground Public Service And Exact-Retry Receipts (2026-10-25)

- `control/raft/service.rs` now owns a foreground ControlNode lifecycle with
  explicit create/resume, voter initialization, status, routed public submission
  and shutdown. Followers redirect to a pinned known leader; request nonce and
  signed bytes are preserved. Remote Submit uses actual QUIC/mutual TLS and binds
  the transport origin to the original proposal author.
- The public state machine retains bounded accepted-operation receipts. An exact
  retry after later changes/restart recovers the historical outcome and first log
  position with `replayed=true`, without reapplying membership. A changed signature
  sharing an ID cannot obtain that receipt. Historical acceptance is not a claim
  of current membership or private application readiness.
- Root `make check` passes 97 tests. A new replay of both native executables is
  running; the last completed two-executable matrix passed 28 library tests plus
  the isolated process-crash test on each guest, recorded in
  `lab/tmp/native-crypto-1792963598738714133.json` (format version 2).
- Public service remains an SDK, not a finished room CLI or cross-VM room test.
  Next: actual foreground multi-process VM service proof, private applied-record
  cursor/catch-up, authenticated scoped bootstrap and committed Welcome replication
  before producer-VM loss. A public submit result alone is not a completed join ACK.

## Local Welcome Outbox And Test-Process Isolation (2026-10-25)

- Private membership confirmation now stores its encrypted Welcome artifact and
  bounded outbox index in the SAME transaction as the crypto epoch/checkpoint.
  `committed_welcome` survives reopen; `acknowledge_welcome` removes it only after
  the caller verifies recipient delivery. Uncertain commit remains quarantined.
- Welcome blobs live in separate bounded local rows, not re-serialized with every
  chat checkpoint. This closes the local COMMIT-to-delivery crash gap. Replication
  to authorized control peers for producer-VM loss is still a runtime requirement.
- Full parallel testing exposed transient inherited file leases around fork/exec
  crash injection. The SIGKILL probe is now its own `tests/room_crash.rs` process;
  production ownership checks were not relaxed and the fault test was not skipped.
  The room library suite passed three consecutive stress runs after isolation.
- Root `make check` now passes 94 tests. The native runner builds/discovers/hashes
  both the library and isolated crash executable and runs them sequentially on
  every guest, preserving coverage without parallel-fork lease interference.
  Its expanded matrix is being replayed; no new native success is claimed yet.

## Public Control Backend Working Tree (2026-10-25)

- Unpublished `0.8.0-dev.2` adds bounded signed control proposals and a public-only
  membership/replay view. Original device signatures bind room, previous head,
  epoch, operation nonce and framed payload hashes. Public/private logical audience
  digests share one owner; mismatch rolls private changes back before release.
- Stable OpenRaft 0.9.25 is now integrated, not the 0.10 alpha. Its public SQLite log
  persists votes, entries, commit/purge cursors and atomic flush callbacks on blocking
  IO workers. The state machine and snapshots contain public data only. Aggregate
  snapshot reception is bounded, not just individual chunks. The upstream storage
  conformance suite passes against this implementation.
- Actual QUIC/mutual-TLS Raft RPC binds full certificate-fingerprint IDs, room
  context and RPC leader/candidate identity. Links are reused; dynamic control-scope
  revocation is checked per RPC. Encoders, streams, connections and time are bounded;
  snapshot chunks have lower scheduling priority than votes/control.
- Root `make check` passes 92 Rust tests, Clippy and rustdoc. Tests include an
  isolated minority refusing quorum success, majority election/heal, real loopback
  QUIC quorum, private MLS confirmation against the accepted public view and
  continued control after stopping the creator's control node.
- Twenty-seven native room tests pass on each of the three guest OSes. Evidence:
  `lab/tmp/native-crypto-1792961427219687265.json`, executable SHA-256
  `cb708e5160d70903affdf31d1c2ba6175c462777bd18ff6f6bd47ab219858560`.
  These tests use loopback/in-process cluster fixtures separately inside each OS,
  NOT an actual cross-VM room service. Guests/network are stopped at rest.
- Remaining: foreground control/room service integration, authenticated scoped
  room bootstrap, private applied-control cursor/catch-up, durable Welcome delivery
  outbox, actual cross-guest data/control scenarios and later media/UI/distribution.
  A committed Welcome lost between crypto COMMIT and delivery must not disappear:
  persist its encrypted deliverable until authenticated acknowledgement, and design
  authorized control replication so creator loss does not strand a new participant.

## Development Checkpoint Publication (2026-10-25)

- Crypto/private-storage development checkpoint `4f3a91e866eb0e167d2b2c84b4f4cb1c37ea6cd9`
  is committed and pushed to GitHub and GitLab. This is source publication of
  `0.8.0-dev.1`, not a completed room service or signed portable release.
- GitHub run `40224311054` succeeds, including Rust and lab-static checks. GitLab
  pipeline `2987002333`, job `17451088448`, also succeeds.
- The next implemented owner must establish authoritative PUBLIC control order
  and then route actual cross-guest room admission and state. Private MLS state is
  not a control snapshot. Existing crypto epoch arguments are not quorum proofs.

## Private MLS Persistence Checkpoint (2026-10-25)

- `rooms/store.rs` now implements the official OpenMLS SQLite provider through
  Libcrux with bundled SQLite. Private owned directories/files, exclusive leases,
  bounded storage, FULL synchronization, DELETE journals and secure deletion are
  enforced. Unknown/corrupt/unsafe existing state is refused, not repaired.
- `CryptoMember::create_private/open_private` persist and validate the private
  signer, public credential and package budget. `Group::restore` checks matching
  room, owner, crypto epoch, pending state and local replay checkpoint.
- `group/durable.rs` owns atomic SDK writes plus local checkpoints before return
  of ciphertext, received text or Welcome. Failed commits quarantine the handle;
  public epoch/membership retain the last durable immutable view. Rejected changes
  roll back and reload instead of retaining dirty in-memory ratchets.
- Root `make check` passes 80 tests, Clippy and rustdoc. Fifteen native room tests
  pass on each of Fedora 44, Mint 22.3 and kk-peer-3, including actual SIGKILL during
  an outstanding SQL transaction and real failed SQLite COMMIT constraints.
- Evidence: `lab/tmp/native-crypto-1792952844042400963.json`; native executable
  SHA-256 `a75cdf2c720792092bb660d00deb04b16828444e5f8b12f30ae6e8c89bec715c`.
  This is per-OS crypto/storage evidence, not cross-guest room networking.
- All guests/network have been stopped. Ordered public control, a durable applied
  consensus/log cursor, actual room admission/replication and the later gates are
  still open. Private databases must never become public replication snapshots.

## MLS Crypto Primitive Update (2026-10-25)

- Unpublished 0.8 now contains an actual OpenMLS group state machine in
  `src/rooms/group.rs`: create/join, prepared add/remove, withheld Welcome,
  exact-byte/epoch confirmation, explicit verified rejection, credential/proposal
  authorization, replay-protected text and epoch-key removal behavior.
- The complete local `make check` passes 71 Rust tests, Clippy and rustdoc.
- Six real crypto tests pass natively on each of Fedora 44, Mint 22.3 and the
  fresh Fedora peer. Raw evidence: `lab/tmp/native-crypto-1792948757956428627.json`;
  executable SHA-256 `53fdad26934cfe9c510e7e5c5ade6e6af7bff5105dc27be51dd0e8de2c26262c`.
  Each guest independently executes the crypto tests; this is NOT a cross-guest
  room transport, ordering or persistence gate.
- Tests include a valid MLS signature claiming the wrong certificate, secret
  masking, message replay/reordering, stale epoch refusal, a removed creator's old
  keys failing to decrypt, delegated admission, inactive-issuer refusal and valid
  but unauthorized MLS commits. Conflicting preparation refuses silent override.
- Crypto provider is official Libcrux 0.4.0 with distinct Ed25519 MLS keys and a
  device-signed room/subject/grant/key binding. Ring authenticates device records.
  The RustCrypto-provider trial conflicted with age's pinned prerelease KEM;
  selecting the official compatible provider restored age's tested dependencies
  without modifying cryptographic implementations or weakening verification.
- All guests/network are stopped at rest. Network room admission, authoritative
  ordered delivery, private durable state and replication are still open.
  Read `docs/rooms.md` and `docs/room-runtime.md` before wiring a runtime.

## Publication And Room-Authority Start (2026-10-25)

- Node checkpoint `5b5a2f630b96907b23c4190569342cf5631e383e` is pushed to both
  remotes. CI portability fix `e0493c75ac27ce54b1bbc1ba8e1b7a2008d612f5` is also pushed.
- GitHub run `40208256307` passes both Rust and lab-static jobs. GitLab pipeline
  `2986728389` succeeds. The first GitHub lab run exposed a hardcoded temporary
  directory; it is fixed, not ignored. Deprecated action runtimes, empty cache
  matching and moving runner labels were also replaced with explicit pinned choices.
- All three guests and the isolated network are stopped at rest.
- Unpublished `0.8.0-dev.1` starts room-scoped signed authority: `src/rooms.rs`,
  `src/signed.rs`, four passing local tests in `tests/rooms.rs`. Proofs bind exact
  creator/subject certificate keys, room, parent, rights and domain. Chains are
  bounded; wrong issuers, escalation, substitution, cross-room replay and cycles fail.
- These are offline authorization proofs only. No global contact is imported and
  no network room-admission or room-revocation gate is claimed by those tests.
- Group cryptography should reuse OpenMLS/RFC 9420, not a home-grown group cipher.
  Primary docs were checked for credential/key binding, epochs, concurrent commits,
  application proposal validation and storage. Ordered delivery and K&K policy are
  still application responsibilities; OpenMLS alone does not close those gates.

## Verified Node Checkpoint: 0.7.0 (2026-10-25)

- Final 0.7.0 binary and migrated canonical `lab/` pass all 66 scenarios after a
  cold start of the three independent guests. Rust checks pass 61 tests; lab static
  validation passes six mocked control tests, Ruff and ShellCheck.
- Raw evidence: `lab/tmp/kk-results-571c21e64207-1792941968896578628.json`.
  A compact public summary is versioned in `lab/evidence/0.7.0.json`.
- The earlier development cold run also passed 66 scenarios (`780a4755da65`);
  all old records remain in `../vm-lab/tmp/`, not copied or deleted.
- `make lab-test` now owns the suite. The older vm-lab Makefile forwards to it;
  media assets and original disks remain outside the source repository.
- Publication/CI monitoring follows this verification. Later sections are retained
  historical snapshots, including their then-current in-progress wording.
- Next: dynamic rooms and replicated state, then native media. No UI or complete
  product/distribution gate is closed by the persistent-node evidence.

## Cold Regression Update (2026-10-25)

- The 0.7 development binary passes all 65 VM scenarios after a cold restart of
  the three guests. `make check` passes 61 Rust tests, Clippy and rustdoc.
- Evidence: `vm-lab/tmp/kk-results-abeabe2f3319-1792940363567737238.json`.
  Installed binary SHA-256: `76011a4a5bc95b4164aceaf1fdcc9e1645ee487cf9fb1be2b6ea94fd86edfb68`.
- In this run, local status took 0.065 seconds including lab-control overhead while
  two file commands were active on the 4 Mbit/s, 50 ms shaped link. This is one
  measured fixture result, not a universal latency or throughput guarantee.
- Packet capture on the first guest contained no application-port traffic outside
  the three declared guest addresses. Every guest had no default Internet route.
- Additional active-file revocation/resume coverage is being checked. This remains
  an uncommitted development tree; the published checkpoint below is still 0.6.

## Working Tree: 0.7.0-dev.1 (2026-10-25)

- Message and file stream handlers are shared by one-shot and persistent lifetimes.
- Foreground `serve`, private same-UID Unix IPC, `status`, exact admitted TLS roots,
  reused authenticated links, live revocation and bounded event/operation queues.
- Long IPC work uses separate text/file lanes. Bulk saturation cannot occupy all
  status readers; excess work is refused, not queued without a bound.
- Uncertain operations retire their cached link. A later explicit operation can
  reconnect; the core does not automatically replay text or files.
- Three independent guests: Fedora 44, Mint 22.3 and a fresh Fedora Cloud peer.
  Private keys were generated separately and never copied between guests.
- Focused run `dbab1da297e8` passed 12 persistent-node scenarios: every directed
  text edge, file ring/hash checks, escaped/oversize sensitive input, two simultaneous
  throttled files with usable text/status, IPC ownership, revoke/restore, partition,
  first-node crash, same-identity restart and socket release.
- Evidence: `vm-lab/tmp/kk-results-dbab1da297e8-1792940065337103271.json`.
- Full regression after cold restart is running. This development tree is not yet
  a published checkpoint. Dynamic rooms, replicated state, media and UI remain open.

## Current Verified Checkpoint: 0.6.0 (2026-10-25)

- Network OPAQUE ceremony, encrypted certificate/grant metadata, two-sided approval,
  temporary strict mutual TLS with exporter binding, then private contact persistence.
- Private invitation/QR image roundtrip, default profiles, aliases, grants and
  fingerprint revocation enforced by library handlers, including active file streams.
- No silent key replacement/regeneration over trust data; CAS rejects stale pairing
  confirmation after a concurrent local permission change.
- 56 local tests pass; all 53 scenarios pass in cold VM run `2a684d63f667`.
- Evidence: `vm-lab/tmp/kk-results-2a684d63f667-1792933990374310154.json`.
- First development pass: 52 scenarios, run `cfb785cc0758`.
- Next implementation gate is persistent multi-peer sessions, followed by mesh/state
  and measured media. Read docs/sessions.md. No frontend or complete-product release yet.
- Historical snapshots below are retained, not current claims of missing 0.6 features.

Publication note: commit `e09bc779fb9b274c52328e2ecde9cc30f4e81ec1` reached both
remotes. The publication-time network timeout on SSH port 22 was resolved with the hosts'
official SSH port-443 endpoints, preserving strict known-host verification and
the existing host-key aliases. The Makefile exposes scoped per-remote SSH command
overrides; no global SSH or agent policy was edited.

## Verified Checkpoint: 0.5.0

- Principal agent only since the operator's no-subagent instruction.
- Unified positional send/receive, explicit file mode, stdin and protected local output.
- Standard age recipient/passphrase envelopes and explicit decryption.
- 45 local tests, 34 real two-guest scenarios, both foundation CIs successful.
- Commit `7296dfd208d90c47a65c985792b11f6467e1cae8` pushed to both SSH remotes.
- GitHub: https://github.com/kpihx-labs/kk
- GitLab: https://gitlab.com/kpihx-labs/kk
- Foundation CI is not a signed release or proof of complete backend functionality.

## Historical Working Tree: 0.6.0-dev.1

The PAKE and contact foundation is in progress and not yet published as complete.

- `src/pairing/pake.rs`: opaque-ke 4.0.1, Argon2id 19456 KiB/t2/p1,
  Ristretto255/TripleDH, six random dictionary words (66 bits), local registration,
  single consumed attempt, expiry/context tests, directional HKDF/AEAD metadata channel.
- `src/contacts.rs`: private local registry, incoming/outgoing grants, changed-key
  refusal, revocation and archived prior states. Not yet connected to networking/CLI.
- `make check` passes 49 local tests. These are not network pairing evidence.
- Real PTY regression run `fdc15a255887` passed all 36 VM scenarios. The two new
  cases force terminal echo initially on, check hidden input and exact terminal
  restoration, and verify Ctrl-C cancellation/restoration in an actual guest PTY.
- Record: `vm-lab/tmp/kk-results-fdc15a255887-1792927278765265426.json`.

## Historical Next Work At The Foundation Snapshot

1. Finish the bounded network pairing ceremony around the tested PAKE state.
2. Exchange certificates and grants only through transcript-bound encrypted metadata.
3. Prove both certificate private keys using temporary pinned mutual TLS before persistence.
4. Require both sides' confirmation/persistence receipts; do not promise atomic remote
   storage across a partition. Preserve partial local completion and make retries safe.
5. Expose pair/trust/default-profile/alias CLI and enforce receiver-side grants/revocation.
6. Add real VM tests for wrong/replayed/expired/racing invitations, changed keys and denied actions.

The remaining session/mesh/media/frontend/distribution phases are still required
and listed in TODO.md and backend-map.md. Do not close them on foundation tests.

## Operating Notes

- No global .agents policy edits. AI text detectors are opt-in for this project.
- Do not delegate, spawn help peers or invoke subagents.
- VM tests: `make -C ../vm-lab kk-test`; stop: `make -C ../vm-lab kk-stop`.
- Root app `make check` does not need elevation and does not boot guests.
- The normal host NAT/firewall configuration stays untouched.
- Do not publish a stable/full-product release or start UI before backend gates pass.
