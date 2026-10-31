// Copyright (c) Mike Grier.

//! Owned, marshalable parameter sets for synchronous Win32 namespace calls.
//!
//! Win32's namespace and metadata surface -- opening, querying, closing -- is
//! synchronous-only. A call that blocks on a dead network path blocks the thread
//! that made it, and no overlapped form exists. This crate makes such a call
//! **capturable as a value**: an owned parameter set built on one thread and
//! executed faithfully on another.
//!
//! It schedules nothing. It is the catalogue-plus-faithful-execution layer,
//! testable with no ring, no pool, and no async anywhere near it.
//!
//! # What a request is, and is not
//!
//! A request carries call **parameters**. It does not carry the impersonation
//! token or any other thread-scoped state the call runs under -- that belongs to
//! [`windows-thread-ambient-sys`][ambient], which this crate does not depend on.
//! The two are siblings rather than a stack: a request can be executed with no
//! captured context at all, and a context is useful to work that never opens a
//! file. Whoever owns both pairs them at the submission site.
//!
//! A request also chooses **no delivery model**. An opened handle comes back
//! plain and unassociated, because associating it with a completion port
//! irreversibly forecloses `IoRing` use, and that choice belongs to a layer that
//! knows the handle's destination.
//!
//! [ambient]: https://docs.rs/windows-thread-ambient-sys
//!
//! # Faithful means unaltered
//!
//! An entry reports the raw Win32 outcome. `ERROR_FILE_NOT_FOUND` means a
//! missing directory from an open, an empty directory from a first query, and a
//! genuine failure from a later one; only a consumer can tell those apart, so
//! nothing here normalises or reclassifies.
//!
//! The code is also snapshotted before any cleanup can overwrite it, because
//! `GetLastError` is volatile thread state that a `Drop` or a buffer release
//! will happily clobber. That guarantee is a primitive rather than a rule each
//! entry remembers: see [`outcome`].
//!
//! # A path is copied; a handle is shared
//!
//! Several entries take a handle rather than a path, and a request holds a
//! [`SharedHandle`](win_shared_os_owned_handle::SharedHandle): a share of the
//! caller's handle, not a new one, and a clone of the request shares it again.
//! Nothing in this crate duplicates a handle. The distinction is easy to get
//! backwards: a path is a value and is copied, while a handle is a reference to a
//! kernel object.
//!
//! A request is therefore self-contained with respect to **lifetime** -- the
//! handle stays open while any holder keeps its share, so a request cannot be
//! left naming a handle its originator closed -- and is **not** isolated with
//! respect to **state**. A request names the caller's kernel object, so an
//! enumeration through it continues the caller's rather than starting its own;
//! an independent traversal needs a fresh open. A caller holding only a borrow
//! makes an owned handle itself, visibly, before handing it over.
//!
//! # Scope
//!
//! One entry per Win32 call; a consumer needing two makes two requests and
//! sequences them itself. The round-one entry list is audited from three real
//! consumers rather than chosen by taste, and its omissions are deliberate and
//! written down. Volume and disk operations are not namespace operations and are in
//! `win-volume-sys`, not here. See `DESIGN-NOTES.md` in the crate
//! root.
//!
//! # Example
//!
//! Capture the parameters on the submitting thread, where a failure is still
//! the caller's to see and the process current directory still means what the
//! caller thinks it means, then use them on a worker that saw none of it:
//!
//! ```
//! use std::fs;
//! use std::thread;
//!
//! use windows_namespace_request_sys::{FileType, OpenFile, QueryFileType};
//! use wtf_string::Wtf16String;
//!
//! let path = std::env::temp_dir().join(format!("wnrs-doc-{}.tmp", std::process::id()));
//! fs::write(&path, b"example")?;
//!
//! // Built here, not on the worker: a relative path is fixed to the process
//! // current directory now, which is shared mutable state that any thread can
//! // change in between. This one is absolute, so it is handed over as given.
//! let text = path.to_str().expect("a temporary path is valid UTF-8");
//! let open = OpenFile::new(Wtf16String::from(text))?;
//! assert_eq!(open.win32_path().to_string_lossy(), text);
//!
//! // A handle is shared, not duplicated: the request holds the caller's
//! // handle, so it stays open for the worker however the caller lets go.
//! let query = QueryFileType::new(fs::File::open(&path)?);
//!
//! let kind = thread::spawn(move || query.perform())
//!     .join()
//!     .expect("the worker did not panic")?;
//!
//! assert_eq!(kind, FileType::DISK);
//! # fs::remove_file(&path)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![cfg(windows)]
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

mod anchored;
mod create_relative;
pub use create_relative::{
    RelativeFile, create_directory_relative, create_file_relative, open_directory_relative,
    open_file_relative, validate_relative_component,
};
pub mod close;
pub mod file_basic;
pub mod file_id;
pub mod file_info;
pub mod file_type;
pub mod final_path;
pub mod full_path;
mod handle;
pub mod open;
pub mod open_by_id;
pub mod outcome;
#[cfg(feature = "tokio-pipe")]
pub mod private_pipe;
pub mod query;
mod rename_relative;
pub mod reopen;
pub mod request;
pub mod volume;
pub mod watch;

pub use anchored::{AnchoredPath, PathCaptureError};
pub use close::{CloseFn, CloseRequest};
pub use file_basic::{FileBasicInfo, QueryFileBasicInfo};
pub use file_id::{FileIdInfo, QueryFileIdInfo};
pub use file_info::QueryFileInformationByHandle;
pub use file_type::{FileType, QueryFileType};
pub use final_path::{FinalPathError, FinalPathFlags, QueryFinalPath};
pub use full_path::{FullPathError, ResolveFullPath};
pub use io_buffers::AlignedBuf;
pub use open::OpenFile;
pub use open_by_id::{FileIdentifier, OpenFileByIdentifier};
pub use outcome::{Outcome, Win32Error};
pub use query::{FileInformationClass, PerformIntoError, QueryFileInformation};
pub use reopen::ReOpenFile;
pub use request::{ConsumingRequest, Request};

/// The owned security descriptor and attributes the requests take: `win-authorization-sys`, where
/// they were moved to and which is where Microsoft Learn files them (*Security and Identity >
/// Authorization*). They are re-exported here at the paths they always had.
pub use win_authorization_sys as security;

/// Compiles the README's examples, so a contract change breaks the build
/// rather than silently teaching the old answer.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(test)]
mod security_open_tests;
#[cfg(test)]
mod shared_handle_tests;
#[cfg(test)]
mod tests;

pub use security::{
    AclState, SecurityAttributes, SecurityCaptureError, SecurityCaptureFailure, SecurityDescriptor,
    SecurityInformation,
};
pub use volume::{QueryVolumeInformation, VolumeInformation};
pub use watch::{ChangeNotification, NotifyFilter, WatchDirectory};
