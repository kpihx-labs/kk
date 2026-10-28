# After-reboot history lifecycle and cold restart

This extends [the same disposable two-profile UI run](reboot-ui-qualification.md)
on 2026-10-28. It is development-binary functional evidence, not release-artifact,
cross-platform, physical-device or network-traversal qualification.

| Case | Result | Evidence |
| --- | --- | --- |
| Disable capture | Pass | Morgan's committed status reported capture off. A newly acknowledged synthetic message remained visible live but had zero stored matches. Two previously captured equal-body records remained stored. |
| Re-enable without backfill | Pass | The explicit enable confirmation restored capture on. The public recipient binding stayed unchanged, both earlier records remained, and the message received while disabled still had zero stored matches. |
| Clear only local stored records | Pass | Morgan's explicit clear confirmation removed those two stored matches while retaining the same enrollment binding, both live messages and the two-member room. Alex still had both corresponding stored records. |
| Whole-UI and room restart | Pass | Both authenticated shutdown actions were acknowledged and both foreground processes returned to their shells. The same profile paths were relaunched without deleting or recreating state, then both rooms were explicitly started. Exact device fingerprints, two members and epoch one were unchanged. |
| Retained and cleared history after restart | Pass | Both profiles reported capture enabled and their existing keys unlocked. Alex decrypted both retained records; Morgan still had zero matches after its clear. Both fresh live views were empty before new traffic. |
| Received-file inventory after restart | Pass for inventory | Alex still listed all four received test files. This does not re-attest their content bytes after restart; earlier byte checks are in the data/media report. |
| Bidirectional traffic after restart | Pass | New messages from Morgan and Alex were received by the other profile, with event-queue acknowledgements rather than human-read claims. Membership stayed at two and epoch one. |

The lifecycle used the actual page handlers and authenticated backend operations.
Synthetic submit callbacks supplied their form as `currentTarget`; an earlier
manually constructed event without that field did not perform the confirmation
and was not treated as a successful mutation. Stored-query checks used the real
history endpoint. No private key, bootstrap capability or invitation body was read.

Activate each target before asynchronous browser evaluations. Background-target
probes exceeded their tool deadlines; foreground state/history requests completed.
Those probes were not evidence of a completed operation or a backend failure.

Only the disposable QA dataset was cleared. Original operator profiles and their
data were not touched. Native file picking, disk-save completion and physical
capture remain outside this evidence.
