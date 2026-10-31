use super::*;
use crate::rooms::{
    Grant, Rights, RootBundle,
    control::raft::{
        Node, PeerId, network,
        service::{Mode, Settings},
    },
    mls::CryptoMember,
};
use std::collections::BTreeMap;

const DEADLINE: Duration = Duration::from_secs(5);

struct Fixture {
    directory: tempfile::TempDir,
    identity: Arc<Identity>,
    control: Arc<ControlNode>,
    room: Room,
}
impl Fixture {
    async fn new(initialize: bool) -> Self {
        let directory = crate::test_support::tempdir().unwrap();
        let identity = Arc::new(Identity::create(&directory.path().join("identity")).unwrap());
        let anchor = RootBundle::create(&identity, "Synthetic runtime history")
            .unwrap()
            .verify(&identity.peer())
            .unwrap();
        let crypto = CryptoMember::create_private(
            &identity,
            &anchor,
            vec![],
            &directory.path().join("private"),
        )
        .unwrap();
        let creator = crypto.public_identity().unwrap();
        let group = Group::create(crypto).unwrap();
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
        if initialize {
            control
                .initialize(BTreeMap::from([(
                    control.id(),
                    Node {
                        endpoint: Some(control.local_addr().unwrap()),
                        ..Default::default()
                    },
                )]))
                .await
                .unwrap();
            tokio::time::timeout(DEADLINE, async {
                while control.current_leader().await.is_none() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        let mut room = Room::new(
            identity.clone(),
            control.clone(),
            group,
            Registry::open(directory.path().join("profile")).unwrap(),
        )
        .unwrap();
        room.configure_history_directory(directory.path().join("private"))
            .unwrap();
        Self {
            directory,
            identity,
            control,
            room,
        }
    }
    async fn member(&mut self) -> (Identity, Group) {
        let identity = Identity::create(&self.directory.path().join("remote")).unwrap();
        let anchor = self
            .room
            .root_bundle()
            .verify(&self.identity.peer())
            .unwrap();
        let grant = Grant::issue(
            &anchor,
            &[],
            &self.identity,
            &identity.peer(),
            Rights::PARTICIPANT,
        )
        .unwrap();
        let mut candidate = CryptoMember::create_private(
            &identity,
            &anchor,
            vec![grant],
            &self.directory.path().join("remote-private"),
        )
        .unwrap();
        let proposal = self
            .room
            .prepare_admission(&candidate.key_package().unwrap(), DEADLINE)
            .await
            .unwrap();
        let accepted = self.room.submit_prepared(DEADLINE).await.unwrap();
        assert!(accepted.accepted);
        let wire = self
            .room
            .committed_welcome(proposal.commit_bytes())
            .unwrap()
            .unwrap();
        let (_, records) = self.control.fresh_catch_up(None, DEADLINE).await.unwrap();
        let record = records
            .iter()
            .find(|record| Some(record.position().unwrap().log) == accepted.first_log)
            .unwrap();
        let group = Group::join_committed(candidate, &wire, &self.identity.peer(), record).unwrap();
        (identity, group)
    }
    async fn close(self) {
        drop(self.room);
        match Arc::try_unwrap(self.control) {
            Ok(control) => control.shutdown().await.unwrap(),
            Err(_) => panic!("history fixture leaked control owner"),
        }
    }
}
fn search(after_id: u64, limit: usize) -> history::Search {
    history::Search {
        text: String::new(),
        after_id,
        limit,
        response_bytes: crate::rooms::store::history::MAX_RESPONSE_BYTES,
    }
}

#[tokio::test]
async fn foreground_instance_is_stable_until_reopen_without_changing_identity_or_history() {
    let Fixture {
        directory,
        identity,
        control,
        mut room,
    } = Fixture::new(true).await;
    room.history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let (_, reference) = room
        .encrypt_text_referenced("retained across foreground replacement", false, DEADLINE)
        .await
        .unwrap();
    let anchor = room.root_bundle().verify(&identity.peer()).unwrap();
    let (handle, actor, events) = Actor::new(room);
    let first = handle.status(DEADLINE).await.unwrap();
    for (running, busy) in [(true, false), (true, true), (true, false), (false, false)] {
        actor.publish(running, busy);
        let status = handle.status(DEADLINE).await.unwrap();
        assert_eq!(status.foreground_instance, first.foreground_instance);
        assert_eq!(status.room, first.room);
        assert_eq!(status.owner, first.owner);
        assert_eq!(status.epoch, first.epoch);
    }
    drop(handle);
    drop(events);
    drop(actor);

    // Reopen existing identity/private state/key material, never recreate it.
    let reopened_identity = Arc::new(Identity::load(&directory.path().join("identity")).unwrap());
    assert_eq!(
        reopened_identity.peer().fingerprint_bytes(),
        identity.peer().fingerprint_bytes()
    );
    let crypto = CryptoMember::open_private(
        &reopened_identity,
        &anchor,
        &directory.path().join("private"),
    )
    .unwrap();
    let mut reopened = Room::new(
        reopened_identity,
        control.clone(),
        Group::restore(crypto).unwrap(),
        Registry::open(directory.path().join("profile")).unwrap(),
    )
    .unwrap();
    reopened
        .configure_history_directory(directory.path().join("private"))
        .unwrap();
    reopened.history_open_file(DEADLINE).await.unwrap();
    let page = reopened
        .history_search(search(0, 10), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].record.id, 1);
    assert_eq!(
        page.rows[0].record.body,
        "retained across foreground replacement"
    );
    assert_eq!(page.rows[0].reference, reference);
    let (handle, actor, events) = Actor::new(reopened);
    let second = handle.status(DEADLINE).await.unwrap();
    assert_ne!(second.foreground_instance, first.foreground_instance);
    assert_eq!(second.owner, first.owner);
    assert_eq!(second.room, first.room);
    assert_eq!(second.epoch, first.epoch);
    actor.publish(true, false);
    assert_eq!(
        handle.status(DEADLINE).await.unwrap().foreground_instance,
        second.foreground_instance
    );
    drop(handle);
    drop(events);
    drop(actor);
    match Arc::try_unwrap(control) {
        Ok(control) => control.shutdown().await.unwrap(),
        Err(_) => panic!("foreground instance fixture leaked a control owner"),
    }
}

#[tokio::test]
async fn reply_reference_resolution_is_exact_local_and_rechecks_current_author_rights() {
    use crate::rooms::conversation::Target;
    async fn resolve(
        actor: &mut Actor,
        target: Target,
    ) -> crate::Result<Option<crate::rooms::group::HistoryRow>> {
        let (reply, result) = oneshot::channel();
        actor
            .execute(
                Command::HistoryResolve { target, reply },
                Instant::now() + DEADLINE,
            )
            .await;
        result.await.unwrap()
    }
    let mut fixture = Fixture::new(true).await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let (remote, mut writer) = fixture.member().await;
    let (_, own) = fixture
        .room
        .encrypt_text_referenced("retained own original", false, DEADLINE)
        .await
        .unwrap();
    let wire = writer
        .encrypt_text("retained reply original", false)
        .unwrap();
    let (_, original) = fixture
        .room
        .decrypt_text_referenced(&remote.peer(), &wire, DEADLINE)
        .await
        .unwrap();
    let hidden = writer
        .encrypt_text("excluded sensitive original", true)
        .unwrap();
    let (_, sensitive) = fixture
        .room
        .decrypt_text_referenced(&remote.peer(), &hidden, DEADLINE)
        .await
        .unwrap();
    let (_handle, mut actor, _events) = Actor::new(fixture.room);
    let target = Target::from(&original);
    let row = resolve(&mut actor, target.clone()).await.unwrap().unwrap();
    assert_eq!(row.reference, original);
    assert_eq!(row.record.body, "retained reply original");
    let mut forged = target.clone();
    forged.member = own.member;
    assert!(resolve(&mut actor, forged).await.unwrap().is_none());
    let mut foreign = target.clone();
    foreign.room[0] ^= 1;
    assert!(matches!(
        resolve(&mut actor, foreign).await,
        Err(crate::Error::Denied)
    ));
    assert!(
        resolve(&mut actor, Target::from(&sensitive))
            .await
            .unwrap()
            .is_none()
    );
    actor
        .room
        .prepare_departure(&remote.peer(), DEADLINE)
        .await
        .unwrap();
    actor.room.submit_prepared(DEADLINE).await.unwrap();
    assert!(resolve(&mut actor, target).await.unwrap().is_none());
    assert!(
        resolve(&mut actor, Target::from(&own))
            .await
            .unwrap()
            .is_some()
    );
    actor.room.history_clear(DEADLINE).await.unwrap();
    assert!(
        resolve(&mut actor, Target::from(&own))
            .await
            .unwrap()
            .is_none()
    );
    drop(_handle);
    drop(_events);
    fixture.room = actor.room;
    fixture.close().await;
}

#[tokio::test]
async fn voluntary_departure_is_self_signed_reader_allowed_and_preserves_remaining_authority() {
    use crate::rooms::{Grant, Rights, control::departure::Request};
    let mut fixture = Fixture::new(true).await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    fixture
        .room
        .encrypt_text_referenced("history survives voluntary departure", false, DEADLINE)
        .await
        .unwrap();
    let reader = Identity::create(&fixture.directory.path().join("departing-reader")).unwrap();
    let anchor = fixture
        .room
        .root_bundle()
        .verify(&fixture.identity.peer())
        .unwrap();
    let grant = Grant::issue(
        &anchor,
        &[],
        &fixture.identity,
        &reader.peer(),
        Rights::READER,
    )
    .unwrap();
    let mut candidate = CryptoMember::new(&reader, &anchor, vec![grant]).unwrap();
    fixture
        .room
        .prepare_admission(&candidate.key_package().unwrap(), DEADLINE)
        .await
        .unwrap();
    fixture.room.submit_prepared(DEADLINE).await.unwrap();
    let view = fixture.room.synchronize(DEADLINE).await.unwrap();
    assert!(
        !view
            .member_rights(&reader.peer().fingerprint_bytes())
            .unwrap()
            .can_remove()
    );
    let request = Request::issue(&reader, fixture.room.room_id(), &view).unwrap();
    let before = fixture.room.epoch();
    // The sole controller cannot depart, and a reader's proof cannot name it.
    assert!(matches!(
        fixture.room.leave_self(DEADLINE).await,
        Err(crate::Error::Denied)
    ));
    let mut substituted = serde_json::to_value(&request).unwrap();
    substituted["body"]["member"] =
        serde_json::to_value(PeerId::from_peer(&fixture.identity.peer())).unwrap();
    let substituted = serde_json::from_value(substituted).unwrap();
    assert!(
        fixture
            .room
            .accept_departure_request(substituted, DEADLINE)
            .await
            .is_err()
    );
    assert_eq!(fixture.room.epoch(), before);
    let result = fixture
        .room
        .accept_departure_request(request.clone(), DEADLINE)
        .await
        .unwrap();
    assert!(result.accepted);
    let after = fixture.room.synchronize(DEADLINE).await.unwrap();
    assert_eq!(after.peers().len(), 1);
    assert!(
        after
            .member_rights(&fixture.identity.peer().fingerprint_bytes())
            .unwrap()
            .can_control()
    );
    assert!(
        after
            .member_rights(&reader.peer().fingerprint_bytes())
            .is_none()
    );
    assert!(fixture.room.is_active());
    assert!(matches!(
        fixture
            .room
            .accept_departure_request(request, DEADLINE)
            .await,
        Err(crate::Error::Denied)
    ));
    let page = fixture
        .room
        .history_search(search(0, 10), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        page.rows[0].record.body,
        "history survives voluntary departure"
    );
    fixture.close().await;

    let mut no_quorum = Fixture::new(false).await;
    assert!(
        no_quorum
            .room
            .leave_self(Duration::from_millis(50))
            .await
            .is_err()
    );
    assert_eq!(no_quorum.room.epoch(), 0);
    assert!(no_quorum.room.is_active());
    no_quorum.close().await;
}

#[tokio::test]
async fn incoming_event_references_survive_restart_and_distinguish_equal_bodies() {
    let mut fixture = Fixture::new(true).await;
    let (remote, mut sender) = fixture.member().await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let (handle, mut actor, mut events) = Actor::new(fixture.room);
    let first_owner = handle.status(DEADLINE).await.unwrap();
    actor.publish(true, false);
    assert_eq!(
        handle.status(DEADLINE).await.unwrap().foreground_instance,
        first_owner.foreground_instance,
        "status refresh within one foreground must not retire capture"
    );
    let mut references = Vec::new();
    let mut wires = Vec::new();
    for referenced in [false, true] {
        let wire = sender.encrypt_text("same body", false).unwrap();
        wires.push(wire.clone());
        if referenced {
            let (reply, received) = oneshot::channel();
            actor
                .execute(
                    Command::AcceptReferenced {
                        peer: remote.peer(),
                        wire,
                        reply,
                    },
                    Instant::now() + DEADLINE,
                )
                .await;
            references.push(received.await.unwrap().unwrap());
        } else {
            let (reply, received) = oneshot::channel();
            actor
                .execute(
                    Command::Accept {
                        peer: remote.peer(),
                        wire,
                        reply,
                    },
                    Instant::now() + DEADLINE,
                )
                .await;
            received.await.unwrap().unwrap();
        }
        let Event::ReferencedText { text, reference } = events.try_recv().unwrap() else {
            panic!("missing authenticated reference")
        };
        assert_eq!(text.content.text(), "same body");
        if referenced {
            assert_eq!(references.last().unwrap(), &reference);
        } else {
            references.push(reference);
        }
    }
    assert_ne!(references[0], references[1]);
    let wire = sender.encrypt_text("sensitive live only", true).unwrap();
    let (reply, received) = oneshot::channel();
    actor
        .execute(
            Command::AcceptReferenced {
                peer: remote.peer(),
                wire,
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    let sensitive_reference = received.await.unwrap().unwrap();
    let Event::ReferencedText { text, reference } = events.try_recv().unwrap() else {
        panic!("missing authenticated sensitive reference")
    };
    assert!(text.content.sensitive);
    assert_eq!(text.content.text(), "sensitive live only");
    assert_eq!(reference, sensitive_reference);
    drop(text);
    let anchor = actor
        .room
        .root_bundle()
        .verify(&fixture.identity.peer())
        .unwrap();
    actor.publish(false, false);
    assert_eq!(
        handle.status.borrow().foreground_instance,
        first_owner.foreground_instance,
        "stopping preserves the old instance until a genuine reopen"
    );
    drop(actor);
    drop(handle);
    let crypto = CryptoMember::open_private(
        &fixture.identity,
        &anchor,
        &fixture.directory.path().join("private"),
    )
    .unwrap();
    fixture.room = Room::new(
        fixture.identity.clone(),
        fixture.control.clone(),
        Group::restore(crypto).unwrap(),
        Registry::open(fixture.directory.path().join("profile")).unwrap(),
    )
    .unwrap();
    fixture
        .room
        .configure_history_directory(fixture.directory.path().join("private"))
        .unwrap();
    fixture.room.history_open_file(DEADLINE).await.unwrap();
    let page = fixture
        .room
        .history_search(search(0, 32), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(
        page.rows
            .iter()
            .all(|row| row.reference != sensitive_reference)
    );
    for (row, reference) in page.rows.iter().zip(&references) {
        assert_eq!(&row.reference, reference);
        assert_eq!(row.record.body, "same body");
    }
    let (handle, mut actor, mut events) = Actor::new(fixture.room);
    let reopened_owner = handle.status(DEADLINE).await.unwrap();
    assert_ne!(
        reopened_owner.foreground_instance,
        first_owner.foreground_instance
    );
    assert_eq!(reopened_owner.owner, first_owner.owner);
    assert_eq!(reopened_owner.room, first_owner.room);
    assert_eq!(reopened_owner.epoch, first_owner.epoch);
    assert_eq!(
        Identity::load(&fixture.directory.path().join("identity"))
            .unwrap()
            .peer()
            .fingerprint_bytes(),
        fixture.identity.peer().fingerprint_bytes(),
        "replacement fencing must not regenerate the device identity"
    );
    for wire in wires {
        let (reply, received) = oneshot::channel();
        actor
            .execute(
                Command::AcceptReferenced {
                    peer: remote.peer(),
                    wire,
                    reply,
                },
                Instant::now() + DEADLINE,
            )
            .await;
        assert!(received.await.unwrap().is_err());
        assert!(events.try_recv().is_err());
    }
    assert_eq!(
        actor
            .room
            .history_search(search(0, 32), DEADLINE)
            .await
            .unwrap()
            .rows
            .len(),
        2
    );
    fixture.room = actor.room;
    drop(handle);
    drop(sender);
    fixture.close().await;
}

#[tokio::test]
async fn final_escaped_json_budget_returns_prefix_without_skipping_authorized_rows() {
    let mut fixture = Fixture::new(true).await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    for body in ["\"\\\n\t".repeat(100), "\"\\\n\t".repeat(100)] {
        fixture
            .room
            .encrypt_text(&body, false, DEADLINE)
            .await
            .unwrap();
    }
    let full = fixture
        .room
        .history_search(search(0, 32), DEADLINE)
        .await
        .unwrap();
    let full_size = serde_json::to_vec(&full).unwrap().len();
    // The Group frame fits here, but the runtime's additive withheld field does not.
    let budget = full_size - 1;
    let mut query = search(0, 32);
    query.response_bytes = budget;
    let first = fixture.room.history_search(query, DEADLINE).await.unwrap();
    assert_eq!(first.rows.len(), 1);
    assert!(first.has_more);
    assert_eq!(first.next_after, first.rows[0].record.id);
    assert!(serde_json::to_vec(&first).unwrap().len() <= budget);
    let mut query = search(first.next_after, 32);
    query.response_bytes = budget;
    let last = fixture.room.history_search(query, DEADLINE).await.unwrap();
    assert_eq!(last.rows.len(), 1);
    assert!(!last.has_more);
    assert_eq!(first.rows[0].reference, full.rows[0].reference);
    assert_eq!(last.rows[0].reference, full.rows[1].reference);
    assert!(serde_json::to_vec(&last).unwrap().len() <= budget);
    let mut tiny = search(0, 32);
    tiny.response_bytes = 1;
    assert!(fixture.room.history_search(tiny, DEADLINE).await.is_err());
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn author_removed_after_authenticated_processing_emits_no_live_event() {
    let mut fixture = Fixture::new(true).await;
    let (remote, mut sender) = fixture.member().await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let wire = sender
        .encrypt_text("processed before author removal", false)
        .unwrap();
    let proposal = fixture
        .room
        .prepare_departure(&remote.peer(), DEADLINE)
        .await
        .unwrap();
    let control = fixture.control.clone();
    let (entered, active) = oneshot::channel();
    let (release, paused) = std::sync::mpsc::channel();
    history::AFTER_PRIVATE_OPERATION.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            entered.send(()).unwrap();
            paused.recv_timeout(DEADLINE).unwrap();
        }));
    });
    let removal = tokio::spawn(async move {
        active.await.unwrap();
        assert!(
            control
                .submit_public(proposal, DEADLINE)
                .await
                .unwrap()
                .accepted
        );
        release.send(()).unwrap();
    });
    let (handle, mut actor, mut events) = Actor::new(fixture.room);
    let (reply, received) = oneshot::channel();
    actor
        .execute(
            Command::AcceptReferenced {
                peer: remote.peer(),
                wire,
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    assert!(matches!(
        received.await.unwrap(),
        Err(Error::Denied | Error::Revoked)
    ));
    assert!(events.try_recv().is_err());
    removal.await.unwrap();
    let page = actor
        .room
        .history_search(search(0, 32), DEADLINE)
        .await
        .unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.withheld, 1); // Committed processed fact remains, disclosure does not.
    fixture.room = actor.room;
    drop(handle);
    drop(sender);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_synchronous_history_work_is_not_preempted_by_control_or_timeout() {
    let mut fixture = Fixture::new(true).await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    fixture
        .room
        .encrypt_text("bounded active work", false, DEADLINE)
        .await
        .unwrap();
    let (entered, active) = oneshot::channel();
    let (release, paused) = std::sync::mpsc::channel();
    history::AFTER_PRIVATE_OPERATION.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            entered.send(()).unwrap();
            paused.recv_timeout(DEADLINE).unwrap();
        }));
    });
    let (handle, actor, _events) = Actor::new(fixture.room);
    let observer = handle.clone();
    let observed = tokio::spawn(async move {
        active.await.unwrap();
        assert!(observer.status(DEADLINE).await.unwrap().busy);
        // Cached reads can run on another worker, but control cannot interrupt
        // synchronous work already selected on the owning actor thread.
        assert!(
            observer
                .history_status(Duration::from_millis(30))
                .await
                .is_ok()
        );
        assert!(matches!(
            observer.shutdown(Duration::from_millis(30)).await,
            Err(Error::Timeout)
        ));
        release.send(()).unwrap();
    });
    let room = tokio::task::LocalSet::new()
        .run_until(async {
            let running = tokio::task::spawn_local(actor.run());
            handle
                .history_search(search(0, 32), DEADLINE)
                .await
                .unwrap();
            observed.await.unwrap();
            handle.shutdown(DEADLINE).await.unwrap();
            running.await.unwrap()
        })
        .await;
    drop(handle);
    Fixture { room, ..fixture }.close().await;
}

#[tokio::test]
async fn file_capture_sensitive_exclusion_stable_pages_and_disable_clear() {
    let mut fixture = Fixture::new(true).await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let (_, first) = fixture
        .room
        .encrypt_text_referenced("first", false, DEADLINE)
        .await
        .unwrap();
    fixture
        .room
        .encrypt_text("excluded", true, DEADLINE)
        .await
        .unwrap();
    fixture
        .room
        .encrypt_text("second", false, DEADLINE)
        .await
        .unwrap();
    let page = fixture
        .room
        .history_search(search(0, 1), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].reference, first);
    assert_eq!(page.rows[0].record.body, "first");
    assert!(page.has_more);
    let page = fixture
        .room
        .history_search(search(page.next_after, 1), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].record.body, "second");
    assert!(!page.has_more);
    fixture.room.history_disable(DEADLINE).await.unwrap();
    fixture
        .room
        .encrypt_text("not captured", false, DEADLINE)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .room
            .history_search(search(0, 32), DEADLINE)
            .await
            .unwrap()
            .rows
            .len(),
        2
    );
    fixture.room.history_clear(DEADLINE).await.unwrap();
    assert!(
        fixture
            .room
            .history_search(search(0, 32), DEADLINE)
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    fixture.close().await;
}

#[tokio::test]
async fn removed_authors_are_counted_with_cursor_without_dropping_later_rows() {
    let mut fixture = Fixture::new(true).await;
    let (remote, mut sender) = fixture.member().await;
    fixture
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let wire = sender.encrypt_text("remote before removal", false).unwrap();
    fixture
        .room
        .decrypt_text(&remote.peer(), &wire, DEADLINE)
        .await
        .unwrap();
    let removal = fixture
        .room
        .prepare_departure(&remote.peer(), DEADLINE)
        .await
        .unwrap();
    let pending = fixture.room.pending_mutation().unwrap().unwrap();
    assert_eq!(pending.id, removal.id().unwrap());
    assert_eq!(pending.kind, super::super::PendingMutationKind::Removal);
    assert_eq!(pending.member, PeerId::from_peer(&remote.peer()));
    fixture.room.submit_prepared(DEADLINE).await.unwrap();
    fixture
        .room
        .encrypt_text("current local", false, DEADLINE)
        .await
        .unwrap();
    let page = fixture
        .room
        .history_search(search(0, 1), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.withheld, 1);
    assert!(page.rows.is_empty());
    assert!(page.next_after > 0 && page.has_more);
    let page = fixture
        .room
        .history_search(search(page.next_after, 1), DEADLINE)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].record.body, "current local");
    assert!(
        fixture
            .room
            .decrypt_text(&remote.peer(), &wire, DEADLINE)
            .await
            .is_err()
    );
    drop(sender);
    fixture.close().await;
}

#[tokio::test]
async fn queue_reservation_precedes_ratchet_and_failed_capture_returns_no_event() {
    let mut fixture = Fixture::new(true).await;
    let (remote, mut sender) = fixture.member().await;
    fixture
        .room
        .history_enable_file(
            history::Policy {
                encrypted_bytes: 1,
                ..Default::default()
            },
            DEADLINE,
        )
        .await
        .unwrap();
    let wire = sender
        .encrypt_text("retry after full queue", false)
        .unwrap();
    let (handle, mut actor, mut events) = Actor::new(fixture.room);
    let event_sender = actor.events.clone();
    let permits = event_sender
        .try_reserve_many(config::ROOM_EVENT_QUEUE)
        .unwrap()
        .collect::<Vec<_>>();
    let (reply, received) = oneshot::channel();
    actor
        .execute(
            Command::Accept {
                peer: remote.peer(),
                wire: wire.clone(),
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    assert!(matches!(
        received.await.unwrap(),
        Err(Error::Limit {
            resource: "room event queue",
            ..
        })
    ));
    drop(permits);
    drop(event_sender);
    let (reply, received) = oneshot::channel();
    actor
        .execute(
            Command::Accept {
                peer: remote.peer(),
                wire: wire.clone(),
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    assert!(received.await.unwrap().is_err());
    assert!(events.try_recv().is_err());
    actor
        .room
        .history_enable_file(history::Policy::default(), DEADLINE)
        .await
        .unwrap();
    let (reply, received) = oneshot::channel();
    actor
        .execute(
            Command::AcceptReferenced {
                peer: remote.peer(),
                wire,
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    assert!(received.await.unwrap().is_ok());
    assert!(matches!(
        events.try_recv().unwrap(),
        Event::ReferencedText { .. }
    ));
    fixture.room = actor.room;
    drop(handle);
    drop(sender);
    fixture.close().await;
}

#[tokio::test]
async fn history_queue_is_separate_and_cached_status_never_needs_quorum() {
    let fixture = Fixture::new(false).await;
    let (handle, actor, _events) = Actor::new(fixture.room);
    let local = tokio::task::LocalSet::new();
    let room = local
        .run_until(async {
            let mut calls = Vec::new();
            for _ in 0..HISTORY_QUEUE {
                let caller = handle.clone();
                calls.push(tokio::task::spawn_local(async move {
                    caller.history_open_file(DEADLINE).await
                }));
            }
            tokio::task::yield_now().await;
            assert!(matches!(
                handle.history_clear(DEADLINE).await,
                Err(Error::Limit {
                    max: HISTORY_QUEUE,
                    ..
                })
            ));
            assert_eq!(
                handle
                    .history_status(Duration::from_millis(20))
                    .await
                    .unwrap(),
                history::Summary::Unknown
            );
            let running = tokio::task::spawn_local(actor.run());
            // Reserved control lane is selected before queued history I/O.
            handle.shutdown(DEADLINE).await.unwrap();
            for call in calls {
                assert!(call.await.unwrap().is_err());
            }
            running.await.unwrap()
        })
        .await;
    Fixture { room, ..fixture }.close().await;
}

#[test]
fn bounded_query_and_expired_result_reject_before_disclosure() {
    let mut query = search(0, 1);
    assert!(query.validate().is_ok());
    query.limit = 0;
    assert!(query.validate().is_err());
    query.limit = 1;
    query.text = "x".repeat(crate::rooms::store::history::MAX_QUERY_BYTES + 1);
    assert!(query.validate().is_err());
    assert!(matches!(
        before_deadline(Ok("prepared"), Instant::now()),
        Err(Error::Timeout)
    ));
}

#[tokio::test]
async fn post_operation_local_veto_withholds_query_wire_and_incoming_event() {
    use crate::contacts::{Contact, Permissions};
    for operation in 0..3 {
        let mut fixture = Fixture::new(true).await;
        let (remote, mut sender) = fixture.member().await;
        fixture
            .room
            .history_enable_file(history::Policy::default(), DEADLINE)
            .await
            .unwrap();
        fixture
            .room
            .encrypt_text("existing row", false, DEADLINE)
            .await
            .unwrap();
        let policy = Registry::open(fixture.directory.path().join("profile")).unwrap();
        let permissions = Permissions {
            text: true,
            rooms: true,
            files: false,
            media: false,
        };
        policy
            .put(
                "remote",
                Contact {
                    certificate: remote.peer().certificate.as_ref().to_vec(),
                    local_identity: fixture.identity.peer().fingerprint(),
                    incoming: permissions,
                    outgoing: permissions,
                    endpoints: vec![],
                },
            )
            .unwrap();
        history::AFTER_PRIVATE_OPERATION.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || policy.revoke("remote").unwrap()));
        });
        match operation {
            0 => assert!(matches!(
                fixture.room.history_search(search(0, 32), DEADLINE).await,
                Err(Error::Revoked | Error::Denied)
            )),
            1 => assert!(matches!(
                fixture
                    .room
                    .encrypt_text_referenced("prepared but withheld", false, DEADLINE)
                    .await,
                Err(Error::Revoked | Error::Denied)
            )),
            _ => {
                let wire = sender
                    .encrypt_text("processed but withheld", false)
                    .unwrap();
                let (_handle, mut actor, mut events) = Actor::new(fixture.room);
                let (reply, received) = oneshot::channel();
                actor
                    .execute(
                        Command::AcceptReferenced {
                            peer: remote.peer(),
                            wire,
                            reply,
                        },
                        Instant::now() + DEADLINE,
                    )
                    .await;
                assert!(matches!(
                    received.await.unwrap(),
                    Err(Error::Revoked | Error::Denied)
                ));
                assert!(events.try_recv().is_err());
                fixture.room = actor.room;
            }
        }
        // SDK storage facts remain prepared/processed, never a runtime receipt.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let page = fixture
            .room
            .group
            .history_search(
                crate::rooms::store::history::Query {
                    text: "",
                    after_id: 0,
                    limit: 32,
                    response_bytes: crate::rooms::store::history::MAX_RESPONSE_BYTES,
                },
                now,
            )
            .unwrap();
        assert_eq!(page.rows.len(), if operation == 0 { 1 } else { 2 });
        drop(sender);
        fixture.close().await;
    }
}

#[tokio::test]
async fn no_quorum_history_read_and_cancelled_receive_release_nothing() {
    let mut fixture = Fixture::new(false).await;
    // An enrolled SDK context does not bypass the runtime's quorum gate.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    fixture
        .room
        .group
        .history_use_identity(age::x25519::Identity::generate(), now)
        .unwrap();
    fixture
        .room
        .group
        .history_enable(history::Policy::default(), now)
        .unwrap();
    assert!(
        fixture
            .room
            .history_search(search(0, 32), Duration::from_millis(30))
            .await
            .is_err()
    );
    fixture.close().await;

    let mut fixture = Fixture::new(true).await;
    let (remote, mut sender) = fixture.member().await;
    let wire = sender.encrypt_text("cancelled caller", false).unwrap();
    let (_handle, mut actor, mut events) = Actor::new(fixture.room);
    let (reply, received) = oneshot::channel();
    drop(received);
    actor
        .execute(
            Command::AcceptReferenced {
                peer: remote.peer(),
                wire,
                reply,
            },
            Instant::now() + DEADLINE,
        )
        .await;
    assert!(events.try_recv().is_err());
    fixture.room = actor.room;
    drop(sender);
    fixture.close().await;
}
