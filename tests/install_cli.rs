use std::process::Command;

#[test]
fn installation_commands_reject_profile_flags_before_catalog_resolution() {
    let scratch = tempfile::tempdir().unwrap();
    let data = scratch.path().join("nonexistent-profile-data");
    let prefix = scratch.path().join("uncreated prefix");
    for operation in ["install", "rollback"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kk"));
        command
            .env("XDG_DATA_HOME", &data)
            .args([
                "--profile-name",
                "not-registered",
                operation,
                "--version",
                "0.8.2",
                "--prefix",
            ])
            .arg(&prefix);
        if operation == "install" {
            command.args(["--commit", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("does not use runtime profiles"), "{error}");
        assert!(!data.exists(), "installer must not create profile metadata");
        assert!(
            !prefix.exists(),
            "refused command must not create installation state"
        );
    }
}

#[test]
fn installer_help_explains_verified_inputs_without_performing_work() {
    for operation in ["install", "rollback"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kk"))
            .args([operation, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--version"));
        assert!(help.contains("--prefix"));
        if operation == "install" {
            assert!(help.contains("--archive"));
            assert!(help.contains("--commit"));
            assert!(help.contains("--bundle"));
        }
    }
}
