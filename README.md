# K&K

Encrypted communication between devices, without a permanent host or mandatory
cloud service. A Rust core serves both the CLI and a bundled browser workspace.

```text
                  one shared core
                 /               \
             terminal          browser
                 \               /
            identities + permissions
                        |
                  encrypted peers
                  A -------- B
                   \        /
                    \-- C --/
```

## What Works

| Need | Available behavior |
| --- | --- |
| Pair devices | Single-use code/invitation, confirmation on both devices |
| Keep communicating | Foreground contact inboxes or multi-member rooms |
| Exchange text | Encrypted delivery reports; optional hidden input/output |
| Send files | Explicit receiving consent, verified content, resumable transfer |
| Keep a copy available | Exact-audience offers, lookup and verified retrieval |
| Share media | Consented native/browser sources and selected-source playback |
| Use a workspace | Private loopback UI with neutral light/dark themes |

**Delivery status:** [TODO.md](TODO.md). A tag or signed draft is not a published
release. **Supported environments:** [platforms](docs/platforms.md).

## Start

Use an installed binary, or build from the checkout:

```sh
cargo fetch --locked
make build
./target/release/kk --help
./target/release/kk ui
```

`kk ui` stays in the foreground. Each device owns its private profile; remote
devices join a room through its reachable address, not the loopback UI address.

Prefer the terminal? [Create and join a room](docs/cli.md#rooms), or
[pair two contacts](docs/pairing.md) for direct sends.

## Find The Right Page

| Question | Read |
| --- | --- |
| Which commands should I run? | [CLI](docs/cli.md), [UI](docs/ui.md) |
| Who can join, vote or receive data? | [Rooms](docs/rooms.md), [contract](CONTRACT.md) |
| What happens to files or copies? | [Files](docs/files.md), [replicas](docs/replicas.md) |
| Which media/platform paths are verified? | [Media](docs/media.md), [platforms](docs/platforms.md) |
| How is the backend organized? | [Owners](docs/backend-map.md), [room runtime](docs/room-runtime.md) |
| What has actually passed? | [Verification record](docs/progress.md), [lab](docs/lab.md) |
| How do I verify or install a release? | [Distribution](docs/distribution.md) |
| What are the security boundaries? | [Review](docs/security-review.md), [dependency overrides](docs/dependency-overrides.md) |

## Develop

```sh
make help           # Actual targets and their purpose
make check          # Product format, lint, tests, docs and packaging checks
make docs-check     # Links, tables, wire names and obsolete command examples
make sdk-check      # Upstream vectors and independent OPAQUE interoperability
make lab-check      # Lab tooling checks; does not change VMs
```

K&K's original code uses [BUSL-1.1](LICENSE), transitioning to AGPL-3.0-only under
the terms and date in that file. Imported code retains its own licenses and
[attribution](third_party/README.md). Earlier checkpoints are in [CHANGELOG.md](CHANGELOG.md)
and Git history, not mixed into the current usage guides.
