# Database actor ownership and SQLite locking

The shared [lease boundary](../src/rooms/lease.rs) protects distinct private MLS
and public control databases. Actor exclusion is separate from SQLite transaction
locking; neither substitutes for the other.

| Platform | Actor lease | SQLite policy |
| --- | --- | --- |
| Linux | The existing exclusive database-inode flock is retained for compatibility with existing actors | Existing SQLite flags, locking, durability and resource policies remain enabled |
| Apple | An exclusive flock on a separate empty ownership entry, named from the validated database filename inside its private directory | SQLite retains its own native VFS locking on the database inode; no no-lock VFS or weakened pragma is selected |
| Unsupported protection model | Refused | Not presented as a working protected store |

Darwin SQLite can select different locking styles for different filesystems,
including flock. An independent application flock on the same database inode
must not compete with SQLite's transaction lock. The Apple-specific ownership
entry separates those roles without changing encryption or database contents.
See [SQLite locking styles](https://sqlite.org/compile.html#enable_locking_style)
and [Darwin flock semantics](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html).

The ownership entry is opened without following links or blocking on a special
file. It must be owned, private, regular, single-link and empty. Existing unsafe
entries are refused and preserved, not repaired or replaced. Open verifies both
the database pathname and the ownership pathname against their held descriptors.
The ownership entry is not unlinked when its owner stops.

SQLite connection destruction still precedes lease release. The logical lease
explicitly unlocks its ownership descriptor on drop, so an inherited descriptor
does not extend that logical owner's exclusion. The original database descriptor
remains available for database identity and history-directory binding checks.

Native acceptance must demonstrate policy initialization while the actor lease
is held, competing-owner refusal, retained data after reopen, inherited-description
release, and refusal of unsafe or replaced ownership entries. The selected native
workflow executes store and lease regressions. Linux results do not establish
Apple behavior; fresh hosted results and broader filesystem/product qualification
remain required. This page describes the ownership mechanism, not a passing CI
result or an unsupported-filesystem durability claim.
