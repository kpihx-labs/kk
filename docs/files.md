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

Use an existing owned private directory. `kk file-info PATH` calculates content
metadata without loading an identity. Limits/options are exposed by each command's
`--help`; their authority remains `src/config.rs`.

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

The receipt proves the honest receiver's verified durable publication, not future
availability or honesty of a compromised peer. For retained ciphertext, see
[CLI encryption](cli.md#input-and-local-encryption). For offers after sender loss,
see [replicas](replicas.md).
