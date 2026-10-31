# win-authorization-sys

Memory-safe Rust over the Win32 authorization API, the area Microsoft Learn files under *Security and
Identity > Authorization*. Thin over Win32, adding no policy.

- `SecurityDescriptor` is an owned, self-relative security descriptor in one DWORD-aligned buffer, so it
  names nothing outside itself and can be carried to another thread. It is made from an absolute or a
  self-relative descriptor (`capture`, which is `unsafe` because the pointer is the caller's), or from SDDL
  text (`from_sddl`), and is written back as SDDL (`to_sddl`).
- `SecurityAttributes` is an owned capture of a caller's `lpSecurityAttributes`: a descriptor, or none, and
  the caller's choice of whether the handle is inheritable.
- `AclState` keeps apart the three states of an access-control list that look alike and are not: absent,
  **NULL** (everyone has complete access) and empty (nobody has any).
- `SecurityInformation` is the selection of which parts of a descriptor a call reads or writes.
- `get_security_info` is `GetSecurityInfo`: the parts of the security descriptor of the object an open handle
  names, as an owned `SecurityDescriptor`, with `ObjectType` saying what kind of object it is. A part the
  handle was not opened with the right to read is Windows' refusal, with its code.

```rust
use win_authorization_sys::{AclState, SecurityDescriptor};
use wtf_string::Wtf16String;

// Everyone is granted generic-all by a protected DACL of one entry.
let descriptor = SecurityDescriptor::from_sddl(&Wtf16String::from("D:P(A;;GA;;;WD)"))?;
assert_eq!(descriptor.dacl()?, AclState::Populated(1));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The crate was made by moving this code out of `windows-namespace-request-sys`. From that crate's release that
depends on this one, it re-exports this crate at the paths it had; a release from before then has its own copy,
whose types are distinct from these. This file says what the crate is; what has been released is the changelog's to say.
