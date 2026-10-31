# File Transfers

The receiver chooses the destination and explicitly consents. Files stream in
bounded chunks; remote filenames do not select filesystem paths.

```text
offer -> verify retained prefix -> send remainder -> verify full SHA-256
                                                      |
                                             sync + no-replace publish
                                                      |
                                                 file receipt
```

## Use

| Surface | Receiver | Sender |
| --- | --- | --- |
| One-shot | `kk receive -f --bind LOCAL_IP:45454 --from bob --output PRIVATE_DIR/report.pdf --accept` | `kk send alice ./report.pdf -f` |
| Contact inbox | `kk serve --bind LOCAL_IP:45454 --accept-files --inbox PRIVATE_DIR` | `kk send alice ./report.pdf -f` |
| Room | `kk room serve team --accept-files --inbox PRIVATE_DIR` | `kk room send team ./report.pdf -f --to PEER_DIGEST` |

Use an existing owned private directory. `kk inspect PATH` calculates content
metadata without loading an identity. Limits/options are exposed by each command's
`--help`; their authority remains `src/config.rs`.

### Foreground-Owned File Jobs

Synchronous sending remains the default. Opt in to admission without waiting for
the receipt only when the existing foreground room should own the operation:

```sh
kk --profile "$PROFILE" room send team ./report.pdf --file --to "$PEER_DIGEST" --background
kk --profile "$PROFILE" room transfers team
kk --profile "$PROFILE" room transfer team "$JOB_ID"
kk --profile "$PROFILE" room cancel-transfer team "$JOB_ID" --confirm
```

Take the exact opaque job ID from the admission reply's `result.id`. Admission
success/exit zero means a local job exists, not that the peer stored its bytes.
Source-byte counters are not receipts. Only `completed` with a verified receipt
means durable recipient storage. Status history is bounded to its foreground
owner; stopping/restarting retires IDs without inventing remote rollback.

Cancellation distinguishes `requested`, `acknowledged` and the resulting job.
Acknowledgement ends local work, not already handed-off bytes or remote storage.
Post-handoff loss can remain `uncertain`; inspect before explicitly retrying the
same stable content. A late cancellation cannot undo an already verified receipt.
No command installs or creates an implicit background daemon.

The browser's Room actions / File transfers exposes the same owner jobs and
explicit cancellation. Closing a view stops monitoring, not the job. Received
copies can be forwarded by opaque ID while incoming-file consent stays off;
the destination must still consent, and unknown original sensitivity defaults
the new forwarding operation to sensitive. [Actual GUI evidence](ui-candidate-f817-qualification.md).

## Storage Contract

| Condition | Result |
| --- | --- |
| Valid new offer | Protected partial/meta files bound to peer and content |
| Interrupted transfer | Retain partial bytes; rehash the actual prefix on retry |
| Same complete content | Verify and acknowledge without rewriting |
| Changed peer/content, corrupt prefix or unsafe path | Refuse and preserve existing data |
| Full verified file | Atomic no-replace publication and durability receipt |

Storage checks reject unsafe ownership/modes, final symlinks, hard links and
concurrent writers. Unsupported OS protections fail closed. Ancestor paths remain
operator-selected; local account compromise is outside this guarantee.

## Wire And Meaning

`kk-file/2` uses a sensitivity byte, u64 size and SHA-256 offer; resume metadata
also binds the pinned sender certificate. It does not accept the obsolete v1
layout. A missed receipt can mean the file was already published: retry the same
stable bytes explicitly instead of deleting state or generating new ciphertext.

The sender observes the bounded receipt while uploading, so response EOF/refusal
can cancel a flow-control-blocked body. Success still requires the entire
authorized upload and the exact receipt, never an early receipt alone.
[Duplex regression evidence](file-duplex-qualification.md) distinguishes this
correction from the still-unexplained native manual-peer timing failure.

The receipt proves the honest receiver's verified durable publication, not future
availability or honesty of a compromised peer. For retained ciphertext, see
[CLI encryption](cli.md#input-and-local-encryption). For offers after sender loss,
see [replicas](replicas.md).
