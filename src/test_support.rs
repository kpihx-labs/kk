//! Private test-owned roots, independent of a permissive caller umask.
//! Never changes process-global umask or production path validation.

pub(crate) fn tempdir() -> std::io::Result<tempfile::TempDir> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut builder = tempfile::Builder::new();
        builder.permissions(std::fs::Permissions::from_mode(0o700));
        builder.tempdir()
    }
    #[cfg(not(unix))]
    {
        tempfile::Builder::new().tempdir()
    }
}

#[cfg(unix)]
#[test]
fn fixture_root_is_private_without_changing_global_umask() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempdir().unwrap();
    assert_eq!(
        directory.path().metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
}
