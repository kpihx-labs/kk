# Contribution And Module Ownership

The [delivery tracker](../TODO.md) owns the current documentation/packaging batch
and final release sequence. This page is its integration map, not another plan
or a historical peer/pane register.

```text
changed invariant -> module owner -> focused regression
                           |
                   shared interface delta
                           |
                      coordinator -> stable integration gate
```

| Current work | Boundary | Handoff |
| --- | --- | --- |
| First use | README and getting-started | One local success, then a remote message; no architecture prerequisite |
| Internal docs | Daily guides, optional concepts, advanced install and evidence | Current links, compact diagrams; no qualification diaries |
| Distribution | Installer/bootstrap, archive inventory and standalone archive README | Exact companion bytes, trust model and first-run instructions agree |
| Integration | Shared interfaces, version, stable candidate and hosted jobs | New checks bind the frozen candidate; no inherited passing claim |

Before parallel work, assign a specific disjoint scope and acceptance check.
The coordinator owns shared CLI/IPC schemas, configuration, Cargo wiring and
integration. Review a changed interface and its failure/cancellation meaning
before changing callers. No worker publishes alone.

Use focused regressions during implementation; integrated gates concern a stable
candidate. Browser mocks, real HTTP/QUIC, rendered browser interactions, native
OS execution and signed installation each need their own evidence. Resource
release and actual exit matter more than a success notice.

Implementation map: [backend owners](backend-map.md). Current evidence:
[verification record](progress.md). Historical assignments and retired peers remain in the
[original register](https://github.com/kpihx-labs/kk/blob/44f1a88c976eb44c7886df9233c980ce96ab6383/docs/peers.md)
and [workstream history](https://github.com/kpihx-labs/kk/blob/44f1a88c976eb44c7886df9233c980ce96ab6383/docs/workstreams.md).
