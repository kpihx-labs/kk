# Delivery State

Single source for current delivery status. Version: `Cargo.toml`.
Evidence: [verification record](docs/progress.md).

## Now

| Item | Actual state |
| --- | --- |
| Release | `0.8.2`, published on [GitHub](https://github.com/kpihx-labs/kk/releases/tag/v0.8.2) and [GitLab](https://gitlab.com/kpihx-labs/kk/-/releases/v0.8.2) |
| Previous draft | Signed `v0.8.1` at `fa6a29f`; retained, **not publicly released** |
| Software | Product, docs, audit, SDK and both hosted CI platforms pass |
| Final executable | Exact signed x86_64 bytes pass 25 public, 66 baseline and native contention scenarios |
| Installation | Authenticated release executable installed; managed `0.8.0` retained |
| Branch name | `main` locally and by default on GitHub/GitLab; commits/tags retained |

## Not Qualified

| Scope | Work still needed |
| --- | --- |
| Windows/macOS native | Platform storage, IPC, lifecycle and real OS qualification |
| Physical media/Wayland portal | Explicitly consented hardware and portal lifecycle tests |
| Physical networks | Wi-Fi/AP isolation, sustained load and traversal evidence |

These remain outside this Linux delivery. They are not silently closed by a
compilation, virtual display or mobile layout fixture. Exact digests, CI and lab
report references are kept in the verification record, not duplicated here.
