# Pending Work

## Current Autonomous Completion Order (2026-10-25)

The operator authorized completion and publication, without subagents. The older
gate notes below are historical; only the backend-before-UI gate remains active.

1. [DONE] Verify the 0.5 unified CLI, local sensitive handling and age envelopes in real VMs.
2. [DONE 0.6] Implement PAKE/QR invitation, transcript/key binding, contacts, grants and revocation.
3. Replace one-shot-only orchestration with bounded multi-peer sessions and typed events.
4. Implement authenticated membership, creator-independent lifecycle and replicated state.
5. Implement audio, camera/screen tracks, native sources/sinks, pacing and congestion behavior.
6. Test at least three guests: joins/leaves, creator loss, partitions, revocation and resource caps.
7. Implement the optional light/dark frontend only after backend acceptance evidence exists.
8. Finish portable signed artifacts, verified installers/offline bundle, CI and both remotes.

Current verified scope must be distinguished from this full target. No placeholder,
generated video pattern or two-peer test closes a native capture or mesh gate.

## Next Operator Gate

- Review the discovery and file evidence in docs/lab.md and docs/files.md.
- Approve the next layer explicitly; no media, UI or permanent service starts automatically.

## Physical Hardware Follow-up

- Select two physical test devices and an isolated LAN.
- Verify mutual pairing, message receipt, wrong-peer rejection and interruption.
- Observe attempted external traffic with Internet unavailable.
- Record hosts, OS versions, firewall settings and results.
- Obtain operator approval before implementing another layer.

Stages 1-3 are implemented with separate VM gates: messages, discovery, then files.
Completed work is recorded in CHANGELOG and the lab report, not repeated here.

## Later, Separately Gated

- Voice, followed by screen and video with measured contention behavior.
- Browser interoperability proof before a browser UI or additional transport commitment.
- Mobile capability matrix and native integrations where needed.
- Windows private-storage ACL support and verified multi-OS packaging.
- Security review and performance measurements before production distribution.
