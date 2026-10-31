// Copyright (c) Mike Grier.

//! The `GetFileInformationByHandleEx` entry.
//!
//! Entry 5 of the audited catalogue, and the most-called namespace operation
//! across all three audited consumers.
//!
//! # Why it is here despite being trivial to marshal
//!
//! This entry needs almost no marshaling work: its inputs are a handle, a
//! scalar class, and a buffer size, with no pointer into caller memory
//! anywhere. That invites the conclusion that it does not belong in a catalogue
//! at all. Membership is decided by whether a **blocking** namespace call needs
//! performing off the caller's thread, not by whether it is awkward to marshal
//! -- the latter test would select for our implementation convenience rather
//! than for consumer need. On the former test this is the call whose lack of an
//! overlapped form is why an unassociated handle had to become a first-class
//! destination at all.
//!
//! # This returns bytes, and does not parse them
//!
//! The audited classes have two result shapes -- fixed-size out-parameters and
//! variable-length batches -- and both collapse to one owned buffer here,
//! because this crate returns bytes plus the unaltered outcome. Per-class
//! parsing stays with the consumer that already owns it.
//!
//! Two constraints on that buffer are not negotiable:
//!
//! - It must be **8-byte aligned**. A `Vec<u8>` guarantees byte alignment and
//!   nothing more, and a misaligned batch is not a subtle problem: the very
//!   first query fails with `ERROR_NOACCESS`. [`AlignedBuf`] states the
//!   alignment rather than arriving at it by luck.
//! - The call **reports no written length**. A batch is walked by its own
//!   next-entry offsets, so the whole buffer comes back and the consumer bounds
//!   its own reads, rather than this entry inventing a byte count it cannot
//!   know.
//!
//! # Only two classes touch the enumeration cursor
//!
//! Measured on Windows 11 Enterprise 10.0.28000, `aarch64-pc-windows-msvc`,
//! against a real directory with a deliberately small buffer:
//!
//! | Question | Measured |
//! |---|---|
//! | Does a duplicated handle share the enumeration cursor? | **Yes** -- a clean continuation |
//! | Control: do two separate opens share it? | **No** -- the second restarts |
//! | Does closing the duplicate disturb the source? | **No** |
//! | Does an interleaved `FileBasicInfo`, `FileIdInfo`, or non-`Ex` query disturb it? | **No** |
//!
//! So the contract is narrower than "handle-taking entries are hazardous":
//! **only the two directory-enumeration classes mutate the shared cursor**, and
//! every other query is a pure read that composes freely with an enumeration in
//! progress, on the same handle or on a duplicate. What follows is specific --
//! a request shares the caller's handle, and even a duplicate is *not* an
//! independent enumeration, so an independent traversal needs a fresh open.
//!
//! # This is single-shot, and is not a streaming engine
//!
//! An entry covering the two directory classes otherwise looks like a second
//! implementation of a shipped streaming enumerator. It is not. This entry is
//! **single-shot**: one call, one batch, and the *client* sequences the next,
//! which is the one-entry-per-Win32-call rule applied literally. A consumer
//! wanting streaming enumeration -- with the cursor, refill loop, quanta, and
//! backpressure owned for it -- wants
//! [windows-file-enumeration-sys](https://docs.rs/windows-file-enumeration-sys)
//! and should not rebuild that loop out of single-shot calls.
//!
//! All five audited classes stay reachable here regardless, because restricting
//! them would narrow the entry for a no-consumer reason.
//!
//! # No ambient context is needed
//!
//! Access was checked at the open. That is exactly why the enumeration crate
//! applies impersonation only around `CreateFileW`, and it makes this the
//! clearest case that a request and a context are **paired at submission**
//! rather than fused.

use std::fmt;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_INFO_BY_HANDLE_CLASS, FileBasicInfo, FileCaseSensitiveInfo, FileFullDirectoryInfo,
    FileFullDirectoryRestartInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    FileIdExtdDirectoryInfo, FileIdExtdDirectoryRestartInfo, FileIdInfo, FileStreamInfo,
    GetFileInformationByHandleEx,
};

use io_buffers::AlignedBuf;

use win_shared_os_owned_handle::SharedHandle;

use crate::Win32Error;
use crate::handle::SharedRaw;
use crate::outcome::{Outcome, out_of_memory, perform_bool};

/// The alignment a directory-information batch requires.
///
/// A `FILE_ID_EXTD_DIR_INFO` contains `i64` fields and the API keeps every
/// record in a batch on an 8-byte boundary -- but only if the batch itself
/// starts on one. Changing this value is a breaking change.
const BATCH_ALIGNMENT: usize = align_of::<u64>();

// `AlignedBuf` refuses an alignment that is not a power of two, and `perform` turns every refusal
// into an out-of-memory report, so this is checked where it cannot be mistaken for one.
const _: () = assert!(BATCH_ALIGNMENT.is_power_of_two());

/// The classes whose `lpFileInformation` structure's own page requires an alignment: the class,
/// the structure, and the alignment in bytes.
///
/// The `GetFileInformationByHandleEx` page (`ms.date` 2018-12-05, read 2026-10-20) gives, for each
/// class, "the corresponding data structure types for use with this function" as the
/// `lpFileInformation` type. Of those structures' pages, read the same day, three say the
/// structure "must be aligned on a" "(8-byte) boundary": `FILE_STREAM_INFO`,
/// `FILE_ID_BOTH_DIR_INFO` and `FILE_FULL_DIR_INFO`, each `ms.date` 2018-12-05. No other structure
/// the page lists says so. Changing an entry is a breaking change.
const REQUIRED_ALIGNMENT: [(FILE_INFO_BY_HANDLE_CLASS, &str, usize); 5] = [
    (FileStreamInfo, "FILE_STREAM_INFO", 8),
    (FileIdBothDirectoryInfo, "FILE_ID_BOTH_DIR_INFO", 8),
    (FileIdBothDirectoryRestartInfo, "FILE_ID_BOTH_DIR_INFO", 8),
    (FileFullDirectoryInfo, "FILE_FULL_DIR_INFO", 8),
    (FileFullDirectoryRestartInfo, "FILE_FULL_DIR_INFO", 8),
];

// `perform` allocates at `BATCH_ALIGNMENT`, so it can never be refused by the check below.
const _: () = {
    let mut i = 0;
    while i < REQUIRED_ALIGNMENT.len() {
        assert!(REQUIRED_ALIGNMENT[i].2 <= BATCH_ALIGNMENT);
        i += 1;
    }
};

/// The structure `class` writes and the alignment its page requires, when its page requires one.
fn required_alignment(class: FileInformationClass) -> Option<(&'static str, usize)> {
    REQUIRED_ALIGNMENT
        .iter()
        .find(|(raw, _, _)| *raw == class.as_raw())
        .map(|&(_, structure, required)| (structure, required))
}

/// Refuses a buffer at `address` for `class` when the page of the structure `class` writes requires
/// an alignment the address does not have. Decided on the arguments alone, before any call.
fn check_alignment(class: FileInformationClass, address: usize) -> Result<(), PerformIntoError> {
    match required_alignment(class) {
        Some((structure, required)) if !address.is_multiple_of(required) => {
            Err(PerformIntoError::Misaligned {
                class,
                structure,
                required,
                address,
            })
        }
        _ => Ok(()),
    }
}

/// Why [`QueryFileInformation::perform_into`] did not fill the caller's buffer.
#[derive(Debug)]
#[non_exhaustive]
pub enum PerformIntoError {
    /// Windows refused the call, with the raw code unaltered.
    Win32(Win32Error),
    /// Refused before the call, which was not made: the page of the structure the class writes
    /// says it "must be aligned on" a boundary the buffer's address is not on. It carries no OS
    /// code, because Windows reported nothing.
    Misaligned {
        /// The class asked for.
        class: FileInformationClass,
        /// The structure the `GetFileInformationByHandleEx` page gives as that class's
        /// `lpFileInformation` type.
        structure: &'static str,
        /// The alignment, in bytes, that structure's page requires.
        required: usize,
        /// The buffer's address.
        address: usize,
    },
}

impl PerformIntoError {
    /// The Win32 code, when Windows produced one; `None` for a refusal made before the call.
    #[must_use]
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::Win32(error) => Some(error.code()),
            Self::Misaligned { .. } => None,
        }
    }
}

impl fmt::Display for PerformIntoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(error) => write!(f, "GetFileInformationByHandleEx: {error}"),
            Self::Misaligned {
                structure,
                required,
                address,
                ..
            } => write!(
                f,
                "GetFileInformationByHandleEx not called: {structure} must be aligned on a \
                 {required}-byte boundary, and the buffer is at {address:#x}"
            ),
        }
    }
}

impl std::error::Error for PerformIntoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(error) => Some(error),
            Self::Misaligned { .. } => None,
        }
    }
}

impl From<Win32Error> for PerformIntoError {
    fn from(error: Win32Error) -> Self {
        Self::Win32(error)
    }
}

/// Which information the query asks for.
///
/// A newtype over `FILE_INFO_BY_HANDLE_CLASS` rather than an enum, because
/// Windows defines classes this crate has never heard of and refusing them
/// would narrow the entry. The named constants are the five the audit found;
/// any other class reaches Windows unaltered through [`from_raw`](Self::from_raw).
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::query::FileInformationClass;
/// use windows_sys::Win32::Storage::FileSystem::{FileBasicInfo, FileStandardInfo};
///
/// assert_eq!(FileInformationClass::BASIC.as_raw(), FileBasicInfo);
///
/// // A class with no named constant here is still expressible.
/// let standard = FileInformationClass::from_raw(FileStandardInfo);
/// assert_eq!(standard.as_raw(), FileStandardInfo);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileInformationClass(FILE_INFO_BY_HANDLE_CLASS);

impl FileInformationClass {
    /// `FileBasicInfo`: timestamps and attributes. A pure read.
    /// [`QueryFileBasicInfo`](crate::file_basic::QueryFileBasicInfo) reads it as values.
    pub const BASIC: Self = Self(FileBasicInfo);
    /// `FileIdInfo`: the volume serial and 128-bit file id. A pure read.
    /// [`QueryFileIdInfo`](crate::file_id::QueryFileIdInfo) reads it as values.
    pub const ID: Self = Self(FileIdInfo);
    /// `FileCaseSensitiveInfo`: whether the directory is case-sensitive. A pure
    /// read.
    pub const CASE_SENSITIVE: Self = Self(FileCaseSensitiveInfo);
    /// `FileIdExtdDirectoryInfo`: the next batch of directory entries.
    ///
    /// **Advances the enumeration cursor**, which lives in the file object and
    /// is therefore shared with every duplicate of the handle.
    pub const ID_EXTD_DIRECTORY: Self = Self(FileIdExtdDirectoryInfo);
    /// `FileIdExtdDirectoryRestartInfo`: the first batch, restarting the
    /// enumeration.
    ///
    /// **Resets the enumeration cursor**, with the same sharing consequence.
    pub const ID_EXTD_DIRECTORY_RESTART: Self = Self(FileIdExtdDirectoryRestartInfo);

    /// Wraps a raw `FILE_INFO_BY_HANDLE_CLASS`.
    #[must_use]
    pub const fn from_raw(class: FILE_INFO_BY_HANDLE_CLASS) -> Self {
        Self(class)
    }

    /// The raw class value.
    #[must_use]
    pub const fn as_raw(self) -> FILE_INFO_BY_HANDLE_CLASS {
        self.0
    }

    /// Whether this class mutates the handle's shared enumeration cursor.
    ///
    /// True only for the two directory-enumeration classes. Every other class
    /// is a pure read that composes freely with an enumeration in progress,
    /// measured rather than assumed.
    ///
    /// An unrecognised class reports `false`, because this answers "is this one
    /// of the two known cursor-moving classes", not "is this provably safe".
    ///
    /// # Example
    ///
    /// ```
    /// use windows_namespace_request_sys::query::FileInformationClass;
    ///
    /// assert!(FileInformationClass::ID_EXTD_DIRECTORY.moves_enumeration_cursor());
    /// assert!(FileInformationClass::ID_EXTD_DIRECTORY_RESTART.moves_enumeration_cursor());
    ///
    /// // Interleaving one of these with an enumeration is safe: measured, not
    /// // reasoned from the object model.
    /// assert!(!FileInformationClass::BASIC.moves_enumeration_cursor());
    /// assert!(!FileInformationClass::ID.moves_enumeration_cursor());
    /// ```
    #[must_use]
    pub const fn moves_enumeration_cursor(self) -> bool {
        self.0 == FileIdExtdDirectoryInfo || self.0 == FileIdExtdDirectoryRestartInfo
    }
}

/// An owned, marshalable parameter set for `GetFileInformationByHandleEx`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::query::{FileInformationClass, QueryFileInformation};
///
/// let path = std::env::temp_dir().join(format!("wnrs-q-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// let request = QueryFileInformation::new(file, FileInformationClass::BASIC).with_capacity(256);
///
/// // The whole buffer comes back: the call reports no written length, so the
/// // consumer bounds its own reads rather than trusting a count we invented.
/// let bytes = request.perform()?;
/// assert_eq!(bytes.len(), 256);
/// assert_eq!(bytes.as_ptr() as usize % 8, 0);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryFileInformation {
    handle: SharedHandle,
    class: FileInformationClass,
    capacity: usize,
}

impl QueryFileInformation {
    /// The capacity a request starts with.
    ///
    /// Large enough for any of the fixed-size classes and for a useful batch of
    /// directory entries. A caller enumerating a large directory will want more
    /// and should say so; this is a starting point, not a recommendation.
    pub const DEFAULT_CAPACITY: usize = 64 * 1024;

    /// Begins a request against `handle` for `class`, holding `handle` as one more holder of it.
    pub fn new(handle: impl Into<SharedHandle>, class: FileInformationClass) -> Self {
        Self {
            handle: handle.into(),
            class,
            capacity: Self::DEFAULT_CAPACITY,
        }
    }

    /// Sets the buffer size the query is given.
    ///
    /// For a fixed-size class this must be at least the size of that class's
    /// structure, and Windows reports `ERROR_BAD_LENGTH` if it is not. For a
    /// directory class it bounds how many entries one call can return, and a
    /// buffer too small for even one entry fails with `ERROR_MORE_DATA` --
    /// neither is pre-empted here.
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    /// The handle being queried, shared with its other holders.
    pub fn handle(&self) -> &SharedHandle {
        &self.handle
    }

    /// The information class this request asks for.
    #[must_use]
    pub fn class(&self) -> FileInformationClass {
        self.class
    }

    /// The buffer size the query will be given.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Performs the query on the calling thread.
    ///
    /// Returns the **whole** buffer, 8-byte aligned. The call reports no
    /// written length -- a batch is walked by its own next-entry offsets -- so
    /// nothing here invents a byte count it cannot know.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered. `ERROR_NO_MORE_FILES` ends a
    /// directory enumeration and `ERROR_MORE_DATA` means the buffer held no
    /// complete entry; both are the caller's to interpret.
    ///
    /// The one code the call did not produce is `ERROR_NOT_ENOUGH_MEMORY`, returned when the
    /// buffer for the capacity this query was built with cannot be allocated -- too large for
    /// the machine, or refused by the allocator. The call was not made, and a smaller capacity
    /// can be tried.
    pub fn perform(&self) -> Outcome<AlignedBuf> {
        let mut buffer =
            AlignedBuf::zeroed(self.capacity, BATCH_ALIGNMENT).map_err(|_| out_of_memory())?;
        // At `BATCH_ALIGNMENT`, which every entry of `REQUIRED_ALIGNMENT` divides (checked at
        // build), so the refusal `perform_into` makes cannot apply.
        self.call_into(&mut buffer)?;
        Ok(buffer)
    }

    /// Performs the query on the calling thread, into a buffer the caller owns and keeps.
    ///
    /// For a consumer that reads batch after batch: one buffer serves every call, and nothing is
    /// allocated here. The buffer's length is the size the call is given, and this request's
    /// [`capacity`](Self::capacity) is not used.
    ///
    /// # What is asked of the buffer
    ///
    /// **Its address is refused, before the call, when the class's structure must be aligned and
    /// the address is not.** The `GetFileInformationByHandleEx` page (`ms.date` 2018-12-05) gives
    /// each class's `lpFileInformation` type, and three of those structures' pages say the
    /// structure "must be aligned on a" "(8-byte) boundary": `FILE_STREAM_INFO` (for
    /// `FileStreamInfo`), `FILE_ID_BOTH_DIR_INFO` (`FileIdBothDirectoryInfo` and its restart form)
    /// and `FILE_FULL_DIR_INFO` (`FileFullDirectoryInfo` and its restart form). For those five
    /// classes a buffer whose address is not on an 8-byte boundary is
    /// [`PerformIntoError::Misaligned`] and the call is not made. It is the address that is
    /// checked, since that is what the pages constrain: a buffer stated at 8 or more
    /// ([`AlignedBuf::align`]) is never refused, and one stated at less is refused only where it
    /// actually lies off the boundary.
    ///
    /// For every other class nothing is refused. The `FILE_ID_EXTD_DIR_INFO` page, for the two
    /// classes this crate names, states no alignment; those classes were measured failing with
    /// `ERROR_NOACCESS` on a misaligned buffer, which is not made a check, and is why
    /// [`perform`](Self::perform) allocates at 8.
    ///
    /// A buffer longer than a `DWORD` can count is given as `u32::MAX` bytes, its first that many,
    /// as `perform` gives an over-large capacity.
    ///
    /// # What the buffer holds afterwards
    ///
    /// What the call wrote. The buffer is not cleared first, so bytes the call does not write keep
    /// what they held -- unlike `perform`'s buffer, which starts zeroed. A batch is walked by its
    /// own next-entry offsets, so a consumer reads only what the call wrote. After a failure the
    /// contents are whatever the call left; the page does not say.
    ///
    /// # Errors
    ///
    /// [`PerformIntoError::Misaligned`], with no OS code, for a buffer refused as above. Otherwise
    /// [`PerformIntoError::Win32`] with the raw code, unaltered, as [`perform`](Self::perform)
    /// returns it. With no allocation, the out-of-memory report `perform` can make does not arise.
    ///
    /// # Example
    ///
    /// ```
    /// use windows_namespace_request_sys::AlignedBuf;
    /// use windows_namespace_request_sys::query::{
    ///     FileInformationClass, PerformIntoError, QueryFileInformation,
    /// };
    /// use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
    /// use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    ///
    /// use std::os::windows::fs::OpenOptionsExt;
    ///
    /// let path = std::env::temp_dir().join(format!("wnrs-into-{}", std::process::id()));
    /// std::fs::create_dir_all(&path)?;
    /// std::fs::write(path.join("a.t"), b"a")?;
    /// let directory = std::fs::OpenOptions::new()
    ///     .read(true)
    ///     .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
    ///     .open(&path)?;
    /// let restart = QueryFileInformation::new(
    ///     directory.try_clone()?,
    ///     FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
    /// );
    /// let next = QueryFileInformation::new(directory, FileInformationClass::ID_EXTD_DIRECTORY);
    ///
    /// // One buffer, allocated once, for every batch.
    /// let mut buffer = AlignedBuf::zeroed(64 * 1024, 8)?;
    /// restart.perform_into(&mut buffer)?;
    /// let mut batches = 1;
    /// loop {
    ///     match next.perform_into(&mut buffer) {
    ///         Ok(()) => batches += 1,
    ///         Err(PerformIntoError::Win32(error)) if error.code() == ERROR_NO_MORE_FILES => break,
    ///         Err(error) => return Err(error.into()),
    ///     }
    /// }
    /// assert!(batches >= 1);
    /// # drop((restart, next));
    /// # std::fs::remove_dir_all(&path)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn perform_into(&self, buffer: &mut AlignedBuf) -> Result<(), PerformIntoError> {
        check_alignment(self.class, buffer.as_ptr().addr())?;
        Ok(self.call_into(buffer)?)
    }

    /// The call, into `buffer`, with no check of the buffer.
    fn call_into(&self, buffer: &mut AlignedBuf) -> Outcome<()> {
        let size = u32::try_from(buffer.len()).unwrap_or(u32::MAX);

        perform_bool(|| {
            // SAFETY: the handle is one this request holds and keeps open across the call, and the
            // buffer is writable for at least `size` bytes, its length or `u32::MAX` if that is
            // less. Its alignment, where a structure's page requires one, was checked by the
            // caller (`perform_into`) or is `BATCH_ALIGNMENT` (`perform`).
            unsafe {
                GetFileInformationByHandleEx(
                    self.handle.raw(),
                    self.class.as_raw(),
                    buffer.as_mut_ptr().cast(),
                    size,
                )
            }
        })
    }
}

impl crate::request::Request for QueryFileInformation {
    type Error = crate::Win32Error;
    type Output = AlignedBuf;

    fn perform(&self) -> Outcome<AlignedBuf> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod alignment_tests;
#[cfg(test)]
mod tests;
