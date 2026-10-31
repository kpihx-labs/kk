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
    let directory = crate::test_support::tempdir().unwrap();
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
    let mut group = Group::create(actor).unwrap();
    let candidate_identity = Identity::create(&directory.path().join("candidate")).unwrap();
    let grant = crate::rooms::Grant::issue(
        &anchor,
        &[],
        &identity,
        &candidate_identity.peer(),
        crate::rooms::Rights::PARTICIPANT,
    )
    .unwrap();
    let mut candidate = CryptoMember::new(&candidate_identity, &anchor, vec![grant]).unwrap();
    let view = crate::rooms::control::View::genesis(&anchor, creator.clone()).unwrap();
    let proposal = group
        .prepare_admission(&identity, &view, &candidate.key_package().unwrap())
        .unwrap();
    let pending_id = proposal.id().unwrap();
    let prepared = proposal.encode().unwrap();
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
    let peer = identity.peer();
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
            let pending_peer = peer.clone();
            let pending = tokio::spawn(async move {
                caller
                    .authorize_profile_metadata(pending_peer, Duration::from_secs(5))
                    .await
            });
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
            // Reading recovery state is local and bounded: it must work without
            // quorum after cancellation and preserve the exact signed nonce.
            assert_eq!(
                handle
                    .prepared_control(Duration::from_millis(100))
                    .await
                    .unwrap()
                    .unwrap()
                    .encode()
                    .unwrap(),
                prepared
            );
            let summary = handle
                .pending_mutation(Duration::from_millis(100))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(summary.id, pending_id);
            assert_eq!(summary.kind, super::super::PendingMutationKind::Admission);
            assert_eq!(
                summary.member,
                PeerId::from_peer(&candidate_identity.peer())
            );
            assert_eq!(summary.epoch_before, 0);
            let public = serde_json::to_value(&summary).unwrap();
            assert_eq!(public.as_object().unwrap().len(), 4);
            assert_eq!(public["member"], summary.member.to_string());
            let mut stale = pending_id;
            stale[0] ^= 1;
            // An explicit confirmation of a stale/different operation cannot
            // turn into blind retry of the operation currently in storage.
            assert!(matches!(
                handle
                    .retry_prepared(stale, Duration::from_millis(100))
                    .await,
                Err(Error::Denied)
            ));
            assert_eq!(
                handle
                    .pending_mutation(Duration::from_millis(100))
                    .await
                    .unwrap(),
                Some(summary)
            );
            assert!(matches!(
                handle
                    .authorize_profile_metadata(peer, Duration::from_millis(100))
                    .await,
                Err(Error::Timeout) | Err(Error::Node(_))
            ));
            handle.shutdown(Duration::from_secs(1)).await.unwrap();
            let returned = running.await.unwrap();
            assert!(matches!(
                handle.status(Duration::from_millis(20)).await,
                Err(Error::Cancelled)
            ));
            returned
        })
        .await;
    assert_eq!(
        returned
            .prepared_control()
            .unwrap()
            .unwrap()
            .encode()
            .unwrap(),
        prepared
    );
    drop(returned);
    match Arc::try_unwrap(control) {
        Ok(control) => control.shutdown().await.unwrap(),
        Err(_) => panic!("actor cancellation leaked a control owner"),
    }
}
