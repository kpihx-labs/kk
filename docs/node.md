# Persistent Nodes

> Multiple exchanges, one foreground receiver. Linux is the verified platform.
> This layer connects already paired contacts. It is not dynamic room admission.

## The Path

```text
 terminal: send / status
          |
 private same-user Unix socket
          |
 typed commands + separate bounded text/file lanes
          |
 contact authorization + cached exact-peer TLS links
          |
 shared message/file stream handlers
          |
 another foreground node
          |
 bounded application events / explicitly consented inbox
```

No HTTP listener, cloud rendezvous, installed service or automatic startup.

## Use

Pair both profiles first using the normal code/QR ceremony. Then, on each PC:

```sh
kk serve --bind LOCAL_IP:45454
```

In another terminal, using the same profile:

```sh
kk send alice "hello"
kk send bob "another exchange on the same node"
kk status
```

To consent to incoming files, add these arguments to `serve`:

```sh
--accept-files --inbox PRIVATE_DIRECTORY
```

The destination is selected by the receiver, from authenticated sender identity
and verified content hash. A remote display name never selects a filesystem path.

## Trust And Lifecycle

| Boundary | Rule |
| --- | --- |
| Network | Only exact admitted contact certificates; no system CA acceptance |
| Local commands | Private profile/socket, same effective UID and one node owner |
| Effects | Grants checked for every operation and at file checkpoints |
| Changed trust | Active links rechecked; TLS admission refreshed within one second |
| Broken link | Retired on uncertain operation; next explicit request reauthenticates |
| Exit | Cancel pending work, close links and remove only the socket owned by this node |

An address remains a hint. A changed address cannot silently replace the key.
Restoration of a revoked contact may need the bounded TLS-root refresh interval.
Presence, clean departure and room membership are not inferred from cached links.

## Delivery Is Not Human Reading

- A text ACK means the core accepted a bounded application event, not durable
  storage or proof that a human read it.
- A file ACK follows verified publication. Existing completed content can be
  acknowledged without rewriting it.
- Concurrent streams have no global display order. There is no room event log yet.
- A timeout can leave an uncertain outcome. There is no automatic payload replay,
  persistent text deduplication or exactly-once claim after reconnect/restart.

## Resource Ownership

`src/config.rs` owns peer, connection, stream, event and operation bounds.
The node keeps status readers separate from long text/file work; exceeding a work
lane is refused rather than accumulated in an unbounded queue. File reception
requires both per-file and per-run limits, including a file-attempt budget.

The byte budget is conservative admission accounting, not free-disk reporting:
failed file offers retain their reservation. A successful verified resume returns
its retained-prefix reservation. Restarting a node starts a new explicit budget;
it does not remove existing partial or completed files.

Slow terminal output cannot block the protocol/control loop indefinitely. Event
and output queues remain bounded; shutdown is cancellable even with a full pipe.

## Verification And Remaining Gates

`progress.md` identifies exact binaries and real guest runs. The fixtures use
three independently generated identities, all directed text links, file integrity,
live revocation, constrained bulk traffic, partition/heal, process kill and restart.

The later requirements in `sessions.md` remain open: membership credentials,
delegated admission, replicated/versioned state, history and duplicate ordering,
media, platform-native capture and the optional frontend.
