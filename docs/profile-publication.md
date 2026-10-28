# Profile Publication Boundary

This expands the [K&K contract](../CONTRACT.md), including its local-account-
compromise exclusion. User controls: [profiles](profiles.md).

Display-name publication rechecks sharing at each transport write poll. Directory
settings use the existing writer lock, then the SDK source guard is acquired only
inside the publication callback. No SDK source guard is held during filesystem
verification, and neither guard spans an async wait. Bytes already accepted by
the transport cannot be recalled.

Non-waiting here means try-lock acquisition and a nonblocking transport write poll.
The directory gate still performs synchronous path/open/stat operations. A stalled
filesystem or kernel syscall can delay the executing runtime thread and prevent
an async deadline or cancellation from being polled. There is no hard wall-clock
availability, shutdown or other-stream-progress guarantee under that condition.

The publication gate assumes stable, owned private paths and writers cooperating
through the same settings lock. Retained descriptors and inode checks refuse
observed path replacements; they do not make pathname validation atomic against
an uncoordinated same-UID attacker replacing paths or lock files after the checks.
That attacker remains inside the contract's local-account-compromise exclusion.
