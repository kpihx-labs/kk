use super::*;
use std::{
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
};

struct Paused {
    bytes: Vec<u8>,
    after: usize,
    released: bool,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    wake: Option<Waker>,
}
struct Writer(Arc<Mutex<Paused>>);
impl tokio::io::AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut state = self.0.lock().unwrap();
        if !state.released && state.bytes.len() >= state.after {
            if let Some(entered) = state.entered.take() {
                let _ = entered.send(());
            }
            state.wake = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let length = if state.released {
            bytes.len()
        } else {
            bytes.len().min(state.after - state.bytes.len())
        };
        state.bytes.extend(&bytes[..length]);
        Poll::Ready(Ok(length))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn paused(
    after: usize,
) -> (
    Writer,
    Arc<Mutex<Paused>>,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (entered, reached) = tokio::sync::oneshot::channel();
    let state = Arc::new(Mutex::new(Paused {
        bytes: vec![],
        after,
        released: false,
        entered: Some(entered),
        wake: None,
    }));
    (Writer(state.clone()), state, reached)
}
fn release(state: &Mutex<Paused>) {
    let mut state = state.lock().unwrap();
    state.released = true;
    if let Some(wake) = state.wake.take() {
        wake.wake();
    }
}
async fn snapshot(factory: &Factory) -> Disclosure {
    let state = factory.shared.profile_source.read().unwrap().clone();
    let revision = state.revision;
    let peer = factory.shared.identity.peer();
    let (name, settings) = tokio::task::spawn_blocking(move || state.source.read(&peer))
        .await
        .unwrap()
        .unwrap();
    Disclosure {
        response: ProfileMetadata {
            version: 1,
            epoch: 1,
            name,
        },
        revision,
        settings,
    }
}

#[tokio::test]
async fn every_write_poll_fences_sdk_replacement_and_cross_process_directory_opt_out() {
    for mode in [
        "sdk-opt-out",
        "sdk-source-off",
        "sdk-replace",
        "directory-opt-out",
    ] {
        for after in [0, 4] {
            let root = crate::test_support::tempdir().unwrap();
            let directory = root.path().join("profile");
            crate::profile::private_directory(&directory, true).unwrap();
            let identity = Arc::new(Identity::create(&directory.join("identity")).unwrap());
            let own = PeerId::from_peer(&identity.peer());
            let (factory, server) = Factory::bind(
                [3; 32],
                identity.clone(),
                BTreeMap::from([(own, identity.peer())]),
                "127.0.0.1:0".parse().unwrap(),
            )
            .unwrap();
            if mode == "directory-opt-out" {
                crate::profile::Settings::update_with(&directory, |settings| {
                    settings.display_name = "Old name".into();
                    settings.share_display_name = true;
                    Ok(())
                })
                .unwrap();
                factory
                    .configure_profile_directory(Some(directory.clone()))
                    .unwrap();
            } else {
                factory
                    .configure_profile_name("Old name".into(), true)
                    .unwrap();
            }
            let disclosure = snapshot(&factory).await;
            assert_eq!(disclosure.response.name.as_deref(), Some("Old name"));
            let (mut writer, state, reached) = paused(after);
            let revoker = factory.clone();
            let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(disclosure.send(&factory.shared, &mut writer), async {
                    reached.await.unwrap();
                    let revocation = tokio::task::spawn_blocking(move || match mode {
                        "sdk-opt-out" => revoker
                            .configure_profile_name("Old name".into(), false)
                            .unwrap(),
                        "sdk-source-off" => revoker.configure_profile_directory(None).unwrap(),
                        "sdk-replace" => revoker
                            .configure_profile_name("New name".into(), true)
                            .unwrap(),
                        "directory-opt-out" => {
                            let output = std::process::Command::new(
                                std::env::current_exe().unwrap(),
                            )
                            .args([
                                "--exact",
                                "profile::settings::publication_tests::settings_opt_out_child",
                                "--nocapture",
                            ])
                            .env("KK_TEST_PROFILE_PUBLICATION_DIRECTORY", &directory)
                            .output()
                            .unwrap();
                            assert!(
                                output.status.success(),
                                "{}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                        _ => unreachable!(),
                    });
                    tokio::time::timeout(Duration::from_secs(1), revocation)
                        .await
                        .unwrap()
                        .unwrap();
                    // The old write is still Pending here: completed revocation
                    // must fence its NEXT poll, including after a prefix write.
                    assert_eq!(state.lock().unwrap().bytes.len(), after);
                    release(&state);
                })
            })
            .await
            .unwrap();
            assert!(
                matches!(result, Err(Error::Denied)),
                "{mode}/{after}: {result:?}"
            );
            assert_eq!(
                state.lock().unwrap().bytes.len(),
                after,
                "no old name bytes may be accepted after completed revocation"
            );
            let drivers = factory.driver_owner();
            factory.close();
            drop(server);
            drop(factory);
            drivers.terminate();
            drivers.released().await.unwrap();
        }
    }
}

#[tokio::test]
async fn pending_publication_does_not_hold_a_lease_and_is_deadline_cancellable() {
    let root = crate::test_support::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&root.path().join("identity")).unwrap());
    let own = PeerId::from_peer(&identity.peer());
    let (factory, server) = Factory::bind(
        [3; 32],
        identity.clone(),
        BTreeMap::from([(own, identity.peer())]),
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap();
    factory
        .configure_profile_name("Old name".into(), true)
        .unwrap();
    let disclosure = snapshot(&factory).await;
    let (mut writer, state, reached) = paused(0);
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            Duration::from_millis(50),
            disclosure.send(&factory.shared, &mut writer)
        ),
        async {
            reached.await.unwrap();
        }
    );
    assert!(result.is_err());
    assert!(state.lock().unwrap().bytes.is_empty());
    factory.configure_profile_directory(None).unwrap();
    let drivers = factory.driver_owner();
    factory.close();
    drop(server);
    drop(factory);
    drivers.terminate();
    drivers.released().await.unwrap();
}

#[tokio::test]
async fn directory_validation_precedes_sdk_guard_and_callback_holds_only_the_poll_fences() {
    for revoke_before_callback in [false, true] {
        let root = crate::test_support::tempdir().unwrap();
        let directory = root.path().join("profile");
        crate::profile::private_directory(&directory, true).unwrap();
        let identity = Arc::new(Identity::create(&directory.join("identity")).unwrap());
        let own = PeerId::from_peer(&identity.peer());
        let (factory, server) = Factory::bind(
            [3; 32],
            identity.clone(),
            BTreeMap::from([(own, identity.peer())]),
            "127.0.0.1:0".parse().unwrap(),
        )
        .unwrap();
        crate::profile::Settings::update_with(&directory, |settings| {
            settings.display_name = "Shared name".into();
            settings.share_display_name = true;
            Ok(())
        })
        .unwrap();
        factory
            .configure_profile_directory(Some(directory.clone()))
            .unwrap();
        let disclosure = snapshot(&factory).await;
        let revoker = factory.clone();
        disclosure
            .settings
            .as_ref()
            .unwrap()
            .before_publication_callback(move || {
                // This hook is inside the REAL directory gate, after its filesystem
                // checks and before it invokes the network callback. The previous
                // source-first ordering fails here instead of hanging the test.
                drop(
                    revoker
                        .shared
                        .profile_source
                        .try_write()
                        .expect("filesystem gate must not hold the SDK source read guard"),
                );
                if revoke_before_callback {
                    revoker.configure_profile_directory(None).unwrap();
                }
            });
        let called = std::cell::Cell::new(false);
        let result = disclosure.try_publish(&factory.shared, || {
            called.set(true);
            assert!(
                factory.shared.profile_source.try_write().is_err(),
                "poll callback must hold the SDK source guard"
            );
            let writer = std::fs::File::open(directory.join("settings.json.lock")).unwrap();
            assert_eq!(
                rustix::fs::flock(
                    &writer,
                    rustix::fs::FlockOperation::NonBlockingLockExclusive
                ),
                Err(rustix::io::Errno::WOULDBLOCK),
                "poll callback must also hold the settings publication lease"
            );
            Poll::<()>::Pending
        });
        if revoke_before_callback {
            assert!(matches!(result, Err(Error::Denied)));
            assert!(
                !called.get(),
                "completed SDK off must prevent entry into poll_write"
            );
        } else {
            assert!(matches!(result, Ok(Poll::Pending)));
            assert!(called.get());
        }
        // Pending is just a return value: BOTH fences must have gone away.
        factory.configure_profile_directory(None).unwrap();
        crate::profile::Settings::update_with(&directory, |settings| {
            settings.share_display_name = false;
            Ok(())
        })
        .unwrap();
        let drivers = factory.driver_owner();
        factory.close();
        drop(server);
        drop(factory);
        drivers.terminate();
        drivers.released().await.unwrap();
    }
}
