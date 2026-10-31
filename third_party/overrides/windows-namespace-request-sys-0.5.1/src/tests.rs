// Copyright (c) Mike Grier.

//! The property that binds the foundations together.
//!
//! Each foundation already proves its own independence in its own module: a
//! shared handle outlives its other holders, a captured descriptor outlives the
//! absolute one it was built from, a request's path outlives the string it came
//! from. What only a test at this level can show is the **composite** -- one
//! value holding all three, outliving *every* input at once and still working
//! on a thread that never saw any of them.
//!
//! That is the property an entry will depend on, so it is asserted here rather
//! than inferred from three separate results.

use std::fs::File;
use std::os::windows::io::AsHandle;

use win_shared_os_owned_handle::SharedHandle;
use wtf_string::Wtf16String;

use crate::handle::tests::{FILE_CONTENTS, Fixture, handle_allocation};
use crate::{AclState, OpenFile, SecurityAttributes, SecurityDescriptor};

/// Everything an entry will hold, standing in for the entries themselves,
/// which land with M25.
struct Foundations {
    path: OpenFile,
    directory: SharedHandle,
    file: SharedHandle,
    security: SecurityAttributes,
}

impl Foundations {
    /// Builds every foundation from one fixture, then lets the caller drop it.
    fn capture(fixture: &Fixture) -> Self {
        let directory = fixture.open_directory();
        let file = fixture.open_file();
        let text = fixture
            .directory()
            .to_str()
            .expect("the fixture path is valid UTF-8");

        let descriptor = SecurityDescriptor::from_sddl(&Wtf16String::from("D:(A;;GA;;;WD)"))
            .expect("a descriptor with one entry");

        Self {
            path: OpenFile::new(Wtf16String::from(text)).expect("build the request"),
            directory: SharedHandle::from(
                directory
                    .as_handle()
                    .try_clone_to_owned()
                    .expect("own the directory handle"),
            ),
            file: SharedHandle::from(
                file.as_handle()
                    .try_clone_to_owned()
                    .expect("own the file handle"),
            ),
            security: SecurityAttributes::new(Some(descriptor), true),
        }
    }

    /// Exercises every foundation, so a value that merely still exists is not
    /// mistaken for one that still works.
    fn assert_usable(&self, expected_path: &str) {
        assert_eq!(self.path.win32_path().to_string_lossy(), expected_path);

        let directory = File::from(
            self.directory
                .as_handle()
                .try_clone_to_owned()
                .expect("own the directory handle"),
        );
        assert!(
            directory
                .metadata()
                .expect("read directory metadata")
                .is_dir(),
            "the shared directory handle must still name the directory"
        );

        let file = File::from(
            self.file
                .as_handle()
                .try_clone_to_owned()
                .expect("own the file handle"),
        );
        assert_eq!(
            file.metadata().expect("read file metadata").len(),
            FILE_CONTENTS.len() as u64
        );

        assert!(self.security.inherit_handle());
        assert_eq!(
            self.security
                .descriptor()
                .expect("a descriptor was captured")
                .dacl()
                .expect("read the DACL"),
            AclState::Populated(1)
        );
    }
}

#[test]
fn the_foundations_outlive_every_input_they_were_built_from() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    let (foundations, expected_path) = {
        let fixture = Fixture::new("composite-outlives");
        let expected_path = fixture
            .directory()
            .to_str()
            .expect("the fixture path is valid UTF-8")
            .to_owned();

        (Foundations::capture(&fixture), expected_path)
        // The fixture, both open `File`s, and the absolute descriptor with the
        // SID and ACL it points at are all dropped here.
    };

    foundations.assert_usable(&expected_path);
}

#[test]
fn the_foundations_work_on_a_thread_that_never_saw_their_inputs() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    let (foundations, expected_path) = {
        let fixture = Fixture::new("composite-thread");
        let expected_path = fixture
            .directory()
            .to_str()
            .expect("the fixture path is valid UTF-8")
            .to_owned();

        (Foundations::capture(&fixture), expected_path)
    };

    std::thread::spawn(move || foundations.assert_usable(&expected_path))
        .join()
        .expect("the worker did not panic");
}

#[test]
fn the_composite_moves_and_shares_across_threads() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    // Asserted rather than assumed: `Send` alone would let this design pass its
    // own suite and then fail to compile in a consumer that shares one capture
    // across concurrent workers.
    assert_send::<Foundations>();
    assert_sync::<Foundations>();
}
