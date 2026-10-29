# Delivery State

Single source for current delivery status. Version: `Cargo.toml`.
Evidence: [verification record](docs/progress.md).

## Now

| Item | Actual state |
| --- | --- |
| Release | `0.8.2`, published on [GitHub](https://github.com/kpihx-labs/kk/releases/tag/v0.8.2) and [GitLab](https://gitlab.com/kpihx-labs/kk/-/releases/v0.8.2) |
| Previous draft | Signed `v0.8.1` at `fa6a29f`; retained, **not publicly released** |
| Released software | The published Linux baseline passed product, docs, audit, SDK and both hosted CI platforms |
| Final executable | Exact signed x86_64 bytes pass 25 public, 66 baseline and native contention scenarios |
| Installation | Authenticated `0.8.2` executable installed; obsolete managed `0.8.0` trashed on request, profiles preserved |
| Branch name | `main` locally and by default on GitHub/GitLab; commits/tags retained |

## Product Expansion In Progress

The working tree is newer than the installed release. Coordination, file owners
and acceptance cases live in [product workstreams](docs/workstreams.md).

| Scope | Current verification boundary |
| --- | --- |
| Live incoming files | Publication/cancellation/shutdown and lifetime-budget regressions pass; actual two-UI opt-in, denied retry and binary/empty transfer digests verified |
| Profiles and useful status | Locked updates, local member labels, CLI profile management and public-only offline/live status pass combined checks |
| Human-facing UI | Live profile-name collision/routing, local aliases and Settings-conflict controls verified; 39 static layout cases pass; broader interactive qualification remains open |
| Portable archives | TAR/ZIP transaction regressions pass; real signed 0.8.2 TAR online install, same-version reinstall/rollback verified in isolated prefix; new/raw/offline artifact gates remain open |
| Encrypted local history | Runtime/CLI/UI controls, stable references and literal search implemented with regression evidence; fresh post-reboot two-tab flow and exact-artifact qualification remain open |
| Long-path recovery | IPC correction and runtime-only creation retry implemented; independent review findings require corrective acceptance and a stable integrated gate before delivery |
| Meetings and native targets | Two simultaneous synthetic UI screen publications verified; Windows foundation passes, Apple stdio runtime correction in progress; full native product not claimed |
| Encrypted room history | Transactional capture, key enrollment, authorized CLI/IPC and UI controls implemented; real two-profile HTTP tests pass; latest Edge pairing/history/restart qualification remains open |
| Bootstrap recovery | Public bundle retry tests cover nine publication boundaries without replacing existing bytes; authenticated helper/notice binding tested with mocked network/build inputs, not final release assets |
| Long-profile recovery | Initial SUN_LEN correction passes real IPC/HTTP tests; independent IPC findings require corrective acceptance. Runtime-only create retry now has a passing regression; persistent or ambiguous room state remains refused |
| Persisted QA permissions | Explicitly approved nonrecursive group-write removal applied to workspace/repository/target ancestors: 0775 to 0755. Strict IPC policy retained; QA can resume at unchanged profile paths |

### Current checkpoint: 2026-10-29

This checkpoint supersedes the pending wording in the earlier expansion snapshot;
it does not change the installed release or qualify new distributed bytes.

| Gate | Current state |
| --- | --- |
| Source | Code checkpoint `e284fbf`, package `0.9.0-dev.2`, includes quorum regression `ceb592c`; verified at the same full commit on local, GitHub and GitLab main. Earlier `8761b9f` was delivered to both remotes |
| Hosted verification | `e284fbf` [Verify 40913981729](https://github.com/kpihx-labs/kk/actions/runs/40913981729) failed because mocked lab tests required ignored local VM XML; private test definitions now pass 33 lab cases, hosted replay pending. Prior Verify `40881343912` and Native `40881343914` pass on `03887b8`. Historical [40823238849](https://github.com/kpihx-labs/kk/actions/runs/40823238849) failed on the runtime deadline; fixture corrections do not prove that historical cause |
| Native foundation | `e284fbf` [Native 40913981568](https://github.com/kpihx-labs/kk/actions/runs/40913981568) fails Intel macOS probe 3 while reading fresh Accepted after successful old-transfer revocation. Receiver-side diagnostic phases and separate Room-policy coverage pass 16 local consent cases, not a proven causal fix. Prior selected passes do not qualify the full native product |
| GitLab account gate | `e284fbf` pipeline `3004941331`, like `03887b8` pipeline `3004039518`: all three jobs refuse before runner assignment with `ci_quota_exceeded`, no YAML error; no quota or global runner change made |
| Historical tag | `v0.9.0-dev.1` remains immutable at `7f93adf`; its failed verification is not replaced or promoted |
| History lifecycle | Actual two-tab disable/re-enable, local clear, unchanged-profile cold restart and bidirectional messages pass; [evidence](docs/reboot-history-qualification.md) |
| File/media UI | Binary/empty file digests, refused post-withdrawal offer and simultaneous synthetic screens pass; [evidence](docs/reboot-data-media-qualification.md), not physical capture or file-under-media-load proof |
| Distribution | Earlier bundles had integrity-only evidence; locally built corrected helper now passes real-gh offline installation/reinstall/same-version rollback of authenticated public 0.8.2 TAR bytes as nobody. Current-source published bootstrap, new ZIP/TAR exact-byte replay and inter-version rollback remain open; [executed bindings](docs/install-qualification.md) |
| Lab preflight | Declared `fedora-44`, `mint-22.3`, `kk-peer-3` definitions/disks retained; Fedora offline replay completed and guest/network confirmed off afterwards. Earlier preflight had no artifact replay; this does not qualify new-version bytes |

## Not Qualified

| Scope | Work still needed |
| --- | --- |
| Windows/macOS native | Platform storage, IPC, lifecycle and real OS qualification |
| Physical media/Wayland portal | Explicitly consented hardware and portal lifecycle tests |
| Physical networks | Wi-Fi/AP isolation, sustained load and traversal evidence |

These remain outside this Linux delivery. They are not silently closed by a
compilation, virtual display or mobile layout fixture. Exact digests, CI and lab
report references are kept in the verification record, not duplicated here.
