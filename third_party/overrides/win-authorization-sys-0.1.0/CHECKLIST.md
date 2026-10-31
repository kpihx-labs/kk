# Checklist: win-authorization-sys

Design decisions are in [DESIGN-NOTES.md](DESIGN-NOTES.md). The crate was made by
`durable-ioring`'s `DI-14.8.1`, moving the request crate's `security` module here.

- [x] **WA-1.1** -- `GetSecurityInfo` as a safe query. -> [completed 2026-10-18](COMPLETED-CHECKLIST.md#wa-11)

- [ ] **WA-1.2** -- `SetSecurityInfo`, the write half of the query above, taking a `SecurityDescriptor` and
  a `SecurityInformation` that selects which parts are written. It is documented under the same Learn
  section, so it belongs here.

- [ ] **WA-1.3** -- `GetSecurityDescriptorOwner` and `GetSecurityDescriptorGroup`, so that an owner and a
  group can be read out of a descriptor beside the DACL and SACL it already reports.

- [ ] **WA-1.4** -- `GetKernelObjectSecurity`, which the same Learn section lists beside
  `GetSecurityInfo`: the descriptor of a kernel object, by handle.
