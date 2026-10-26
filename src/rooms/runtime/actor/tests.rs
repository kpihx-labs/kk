use super::*;
use crate::rooms::{
    RootBundle,
    control::raft::{
        PeerId, network,
        service::{Mode, Settings},
    },
    mls::CryptoMember,
};
use std::collections::BTreeMap;

#[tokio::test]
async fn cached_status_and_cancellation_remain_responsive_without_a_quorum() {
    let directory = tempfile::tempdir().unwrap();
    let identity = Arc::new(Identity::create(&directory.path().join("identity")).unwrap());
    let anchor = RootBundle::create(&identity, "Bounded actor control test")
        .unwrap()
        .verify(&identity.peer())
        .unwrap();
    let actor = CryptoMember::create_private(
        &identity,
        &anchor,
        vec![],
        &directory.path().join("private"),
    )
    .unwrap();
    let creator = actor.public_identity().unwrap();
    let group = Group::create(actor).unwrap();
    let control = Arc::new(
        ControlNode::start(
            identity.clone(),
            &directory.path().join("control"),
            Settings {
                anchor,
                listen: "127.0.0.1:0".parse().unwrap(),
                peers: BTreeMap::from([(PeerId::from_peer(&identity.peer()), identity.peer())]),
                timing: network::configuration().unwrap(),
            },
            Mode::Create(creator),
        )
        .await
        .unwrap(),
    );
    // Deliberately NEVER initialize this control node: no read quorum is possible.
    let room = Room::new(
        identity,
        control.clone(),
        group,
        Registry::open(directory.path().join("profile")).unwrap(),
    )
    .unwrap();
    let (handle, actor, _events) = Actor::new(room);
    assert!(
        !handle
            .status(Duration::from_millis(20))
            .await
            .unwrap()
            .running
    );
    assert!(matches!(
        handle.ready(Duration::from_millis(5)).await,
        Err(Error::Timeout)
    ));
    let returned = tokio::task::LocalSet::new()
        .run_until(async {
            let running = tokio::task::spawn_local(actor.run());
            handle.ready(Duration::from_secs(1)).await.unwrap();
            let caller = handle.clone();
            let pending =
                tokio::spawn(async move { caller.synchronize(Duration::from_secs(5)).await });
            let mut observed = handle.status.clone();
            drop(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    observed.wait_for(|status| status.busy),
                )
                .await
                .unwrap()
                .unwrap(),
            );
            let cached = handle.status(Duration::from_millis(20)).await.unwrap();
            assert!(cached.running && cached.busy);
            assert_eq!(cached.epoch, 0);
            assert!(cached.applied.is_none());
            pending.abort();
            assert!(pending.await.unwrap_err().is_cancelled());
            // Cancellation drops the pending read at an async boundary, not five
            // seconds later. It never fabricates membership/order or rolls keys back.
            drop(
                tokio::time::timeout(
                    Duration::from_millis(500),
                    observed.wait_for(|status| !status.busy),
                )
                .await
                .unwrap()
                .unwrap(),
            );
            let cached = handle.status(Duration::from_millis(20)).await.unwrap();
            assert_eq!(cached.epoch, 0);
            assert!(cached.applied.is_none());
            handle.shutdown(Duration::from_secs(1)).await.unwrap();
            let returned = running.await.unwrap();
            assert!(matches!(
                handle.status(Duration::from_millis(20)).await,
                Err(Error::Cancelled)
            ));
            returned
        })
        .await;
    drop(returned);
    match Arc::try_unwrap(control) {
        Ok(control) => control.shutdown().await.unwrap(),
        Err(_) => panic!("actor cancellation leaked a control owner"),
    }
}
