use std::process::Command;

#[test]
fn study_clones_and_local_files_stay_out_of_the_package() {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        // Metadata-only inventory; private fixtures and local study clones stay out.
        .args([
            "package",
            "--offline",
            "--locked",
            "--allow-dirty",
            "--no-metadata",
            "--list",
        ])
        .output()
        .expect("inspect package contents");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = String::from_utf8(output.stdout).expect("UTF-8 package paths");
    assert!(files.lines().any(|path| path == "LICENSE"));
    assert!(files.lines().any(|path| path == "CONTRACT.md"));
    assert!(files.lines().any(|path| path == "tests/test_docs.py"));
    for required in [
        "scripts/release.py",
        "tests/test_release.py",
        "docs/distribution.md",
    ] {
        assert!(
            files.lines().any(|path| path == required),
            "portable release tooling or evidence missing: {required}"
        );
    }
    for path in files.lines() {
        assert!(
            !path.starts_with("refs/")
                && !path.starts_with("tmp/")
                && !path.contains("/target/")
                && path != "AGENTS.md"
                && !path.ends_with("private-key.der"),
            "local-only file leaked into package: {path}"
        );
    }
}
