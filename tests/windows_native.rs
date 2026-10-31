//! Real Windows capability qualification, never executed as a Linux substitute.
#![cfg(windows)]

use kk::{
    Identity,
    profile::windows::ProtectedDirectory,
    windows_ipc::{Client, Listener},
};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kk"))
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn stdio_child(mode: &str) -> ! {
    kk::stdio_worker::configure(worker()).unwrap();
    let ok = {
        let rt = runtime();
        rt.block_on(async {
            if mode == "blocked-output" {
                let writer = kk::stdio_worker::configured().unwrap();
                let (signal, cancel) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let mut byte = [0];
                    let _ = std::io::stdin().read_exact(&mut byte);
                    let _ = signal.send(());
                });
                // Parent cancels only after observing a payload byte, not merely
                // after a startup timer. A completed writer is not a passing cancel.
                tokio::select! {
                    biased;
                    _ = writer.write(zeroize::Zeroizing::new(vec![b'~'; kk::config::MAX_MESSAGE_BYTES * 6])) => false,
                    _ = cancel => true,
                    _ = tokio::time::sleep(Duration::from_secs(8)) => false,
                }
            } else {
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    kk::input::text(Some("-".into()), false),
                )
                .await
                {
                    Ok(Ok(value)) => mode == "stdin-eof" && value.as_str() == "native-fixture",
                    Ok(Err(kk::Error::MessageTooLarge)) => mode == "stdin-overflow",
                    _ => false,
                }
            }
        })
    }; // Runtime shutdown must not wait for a detached blocked producer.
    std::process::exit(if ok { 0 } else { 3 });
}

fn check_stdio_child(mode: &str, input: Vec<u8>) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "native_windows_primitives", "--nocapture"])
        .env("KK_WINDOWS_NATIVE_CHILD", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _held_stdout = if mode == "blocked-output" {
        let mut stdout = child.stdout.take().unwrap();
        let (observed, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = [0];
            for _ in 0..4096 {
                if stdout.read_exact(&mut byte).is_err() {
                    return;
                }
                if byte[0] == b'~' {
                    let _ = observed.send(stdout);
                    return;
                }
            }
        });
        match receive.recv_timeout(Duration::from_secs(10)) {
            Ok(stdout) => {
                stdin.write_all(b"cancel").unwrap();
                Some(stdout) // Keep the nonconsuming reader OPEN, never signal EPIPE.
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("worker payload never became observable: {error}");
            }
        }
    } else {
        None
    };
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            writer.join().unwrap();
            assert!(status.success(), "native stdio mode {mode}: {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native stdio mode {mode} exceeded its owned-process deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_windows_primitives() {
    if let Ok(mode) = std::env::var("KK_WINDOWS_NATIVE_CHILD") {
        stdio_child(&mode);
    }
    let sandbox = tempfile::tempdir().unwrap();
    let parent = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(sandbox.path())
        .unwrap();
    let profile = ProtectedDirectory::create_in(&parent, OsStr::new("protected")).unwrap();
    let mut private = profile
        .create_private_file(OsStr::new("private-fixture"))
        .unwrap();
    private.write_bytes(b"private fixture").unwrap();
    assert!(private.write_bytes(b"replacement").is_err());
    drop(private);
    assert_eq!(
        &*profile
            .read_private_file(OsStr::new("private-fixture"))
            .unwrap(),
        b"private fixture"
    );
    profile
        .publish_public(OsStr::new("public-fixture"), b"complete")
        .unwrap();
    assert!(
        profile
            .publish_public(OsStr::new("public-fixture"), b"replacement")
            .is_err()
    );
    assert_eq!(
        &*profile
            .read_private_file(OsStr::new("public-fixture"))
            .unwrap(),
        b"complete"
    );
    let first = Identity::create_windows(&profile, OsStr::new("identity")).unwrap();
    let fingerprint = first.peer().fingerprint();
    assert!(Identity::create_windows(&profile, OsStr::new("identity")).is_err());
    assert_eq!(
        Identity::load_windows(&profile, OsStr::new("identity"))
            .unwrap()
            .peer()
            .fingerprint(),
        fingerprint
    );
    println!("NATIVE PASS: protected storage, exclusive identity and no-replace publication");
    runtime().block_on(async {
        let name = format!(r"\\.\pipe\kk-l4-{fingerprint}");
        let name = OsStr::new(&name);
        let mut listener = Listener::bind(&profile, name).unwrap();
        assert!(Listener::bind(&profile, name).is_err());
        assert!(matches!(
            listener.accept(Duration::from_millis(30)).await,
            Err(kk::Error::Timeout)
        ));
        let sid = win_authorization_sys::effective_user_sid().unwrap();
        let server = async {
            let mut session = listener.accept(Duration::from_secs(5)).await?;
            let value: serde_json::Value = session.read(Duration::from_secs(5)).await?;
            assert_eq!(value, serde_json::json!({"fixture":"native"}));
            session
                .write(
                    &serde_json::json!({"accepted":true}),
                    Duration::from_secs(5),
                )
                .await?;
            Ok::<(), kk::Error>(())
        };
        let client = async {
            let mut client = Client::connect(&profile, name, Duration::from_secs(5)).await?;
            client
                .write(
                    &serde_json::json!({"fixture":"native"}),
                    Duration::from_secs(5),
                )
                .await?;
            let reply: serde_json::Value = client.read(Duration::from_secs(5)).await?;
            assert_eq!(reply, serde_json::json!({"accepted":true}));
            Ok::<(), kk::Error>(())
        };
        let (server, client) = tokio::join!(server, client);
        assert!(
            server.is_ok() && client.is_ok(),
            "server={server:?}; client={client:?}"
        );
        assert_eq!(win_authorization_sys::effective_user_sid().unwrap(), sid);
    });
    println!(
        "NATIVE PASS: exclusive local pipe, SID authentication, context restore and timeout recovery"
    );
    check_stdio_child("stdin-eof", b"native-fixture".to_vec());
    check_stdio_child(
        "stdin-overflow",
        vec![b'x'; kk::config::MAX_MESSAGE_BYTES + 1],
    );
    check_stdio_child("blocked-output", Vec::new());
    println!("NATIVE PASS: redirected stdin/overflow and blocked-output cancellation");
}
