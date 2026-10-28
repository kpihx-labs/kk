use super::*;
use crate::{
    Identity as Profile,
    rooms::{Anchor, Grant, Rights, RootBundle},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

const TIME: u64 = 1_700_000_000;
struct Pair {
    directory: tempfile::TempDir,
    profiles: Vec<Profile>,
    anchor: Anchor,
    alice: Group,
    bob: Group,
}
fn restore(root: &Path, profiles: &[Profile], anchor: &Anchor, index: usize) -> Group {
    Group::restore(
        CryptoMember::open_private(
            &profiles[index],
            anchor,
            &root.join(format!("state-{index}")),
        )
        .unwrap(),
    )
    .unwrap()
}
impl Pair {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let profiles: Vec<_> = (0..2)
            .map(|i| Profile::create(&root.join(format!("profile-{i}"))).unwrap())
            .collect();
        let anchor = RootBundle::create(&profiles[0], "Synthetic Group history")
            .unwrap()
            .verify(&profiles[0].peer())
            .unwrap();
        let grant = Grant::issue(
            &anchor,
            &[],
            &profiles[0],
            &profiles[1].peer(),
            Rights::MODERATOR,
        )
        .unwrap();
        let mut alice = Group::create(
            CryptoMember::create_private(&profiles[0], &anchor, vec![], &root.join("state-0"))
                .unwrap(),
        )
        .unwrap();
        let mut bob =
            CryptoMember::create_private(&profiles[1], &anchor, vec![grant], &root.join("state-1"))
                .unwrap();
        let package = bob.key_package().unwrap();
        let commit = alice
            .prepare_add(alice.validate_package(&package).unwrap())
            .unwrap();
        let welcome = alice.confirm_ordered(1, &commit).unwrap().unwrap();
        let bob = Group::join(bob, &welcome, &profiles[0].peer(), 1).unwrap();
        Self {
            directory,
            profiles,
            anchor,
            alice,
            bob,
        }
    }
}
fn enable(group: &mut Group, policy: store::Policy) -> Identity {
    let identity = Identity::generate();
    group.history_use_identity(identity.clone(), TIME).unwrap();
    group.history_enable(policy, TIME).unwrap();
    identity
}
fn query() -> store::Query<'static> {
    store::Query {
        text: "",
        after_id: 0,
        limit: store::MAX_RESULTS,
        response_bytes: store::MAX_RESPONSE_BYTES,
    }
}
fn page(group: &mut Group) -> HistoryPage {
    group.history_search(query(), TIME).unwrap()
}
fn send(group: &mut Group, body: &str, sensitive: bool) -> Vec<u8> {
    group
        .encrypt_text_referenced(body, sensitive, TIME)
        .unwrap()
        .0
}
fn receive(group: &mut Group, wire: &[u8]) -> Text {
    group.decrypt_text_referenced(wire, None, TIME).unwrap().0
}
fn rows(group: &mut Group) -> usize {
    match group.history_status(TIME).unwrap() {
        store::Status::Configured { rows, .. } => rows,
        _ => panic!("expected configured history"),
    }
}

#[test]
fn default_off_v1_future_only_and_sensitive_exclusion() {
    let mut pair = Pair::new();
    let first = send(&mut pair.alice, "before opt-in", false);
    receive(&mut pair.bob, &first);
    assert!(matches!(
        pair.alice.history_status(TIME).unwrap(),
        store::Status::Off
    ));
    let path = pair.directory.path().join("state-0");
    assert!(!path.join("history.agekey").exists());
    let sql = rusqlite::Connection::open(path.join("private.sqlite3")).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'kk_history_%'",
            [],
            |row| row.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    enable(&mut pair.alice, store::Policy::default());
    enable(&mut pair.bob, store::Policy::default());
    let sensitive = send(&mut pair.alice, "must not be recorded", true);
    assert!(receive(&mut pair.bob, &sensitive).content.sensitive);
    assert_eq!(rows(&mut pair.alice), 0);
    assert_eq!(rows(&mut pair.bob), 0);
    let (wire, prepared) = pair
        .alice
        .encrypt_text_referenced("future text", false, TIME)
        .unwrap();
    let (text, processed) = pair
        .bob
        .decrypt_text_referenced(&wire, Some(&pair.profiles[0].peer()), TIME)
        .unwrap();
    assert_eq!(text.content.text(), "future text");
    assert_eq!(prepared.member, pair.profiles[0].peer().fingerprint_bytes());
    assert_eq!(processed.member, prepared.member);
    assert_eq!(processed.wire_sha256, Sha256::digest(&wire).as_slice());
    assert_eq!(prepared.wire_sha256, processed.wire_sha256);
    assert_eq!(prepared.room, processed.room);
    assert_eq!(prepared.epoch, processed.epoch);
    assert_eq!(prepared.event, store::Event::OutgoingPrepared);
    assert_eq!(processed.event, store::Event::IncomingProcessed);
    assert_eq!(page(&mut pair.alice).rows[0].reference, prepared);
    assert_eq!(page(&mut pair.bob).rows[0].reference, processed);
    assert_eq!(rows(&mut pair.bob), 1);
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
    assert_eq!(rows(&mut pair.bob), 1);
    assert!(!path.join("history.agekey").exists()); // SDK identity stays caller-owned.
}

#[test]
fn send_capture_failure_rolls_back_ratchet_checkpoint_and_counter() {
    let mut pair = Pair::new();
    enable(
        &mut pair.alice,
        store::Policy {
            encrypted_bytes: 1,
            ..store::Policy::default()
        },
    );
    assert!(
        pair.alice
            .encrypt_text_referenced("rejected after MLS encrypt", false, TIME)
            .is_err()
    );
    assert!(pair.alice.is_active());
    assert_eq!(rows(&mut pair.alice), 0);
    pair.alice
        .history_enable(store::Policy::default(), TIME)
        .unwrap();
    let wire = send(&mut pair.alice, "first committed", false);
    assert_eq!(
        receive(&mut pair.bob, &wire).content.text(),
        "first committed"
    );
    assert_eq!(page(&mut pair.alice).rows[0].record.id, 1);
}

#[test]
fn receive_capture_failure_allows_same_wire_retry_without_ratchet_divergence() {
    let mut pair = Pair::new();
    enable(
        &mut pair.bob,
        store::Policy {
            encrypted_bytes: 1,
            ..store::Policy::default()
        },
    );
    let wire = send(&mut pair.alice, "retry exactly this wire", false);
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
    assert!(pair.bob.is_active());
    assert_eq!(rows(&mut pair.bob), 0);
    pair.bob
        .history_enable(store::Policy::default(), TIME)
        .unwrap();
    assert_eq!(
        receive(&mut pair.bob, &wire).content.text(),
        "retry exactly this wire"
    );
    assert_eq!(page(&mut pair.bob).rows[0].record.id, 1);
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
}

#[test]
fn direct_transport_author_mismatch_records_nothing_and_does_not_consume_wire() {
    let mut pair = Pair::new();
    enable(&mut pair.bob, store::Policy::default());
    let wire = send(&mut pair.alice, "authenticated member", false);
    assert!(
        pair.bob
            .decrypt_text_referenced(&wire, Some(&pair.profiles[1].peer()), TIME)
            .is_err()
    );
    assert_eq!(rows(&mut pair.bob), 0);
    let (_, reference) = pair
        .bob
        .decrypt_text_referenced(&wire, Some(&pair.profiles[0].peer()), TIME)
        .unwrap();
    assert_eq!(
        reference.member,
        pair.profiles[0].peer().fingerprint_bytes()
    );
}

#[test]
fn failed_send_commit_exposes_no_wire_or_reference_and_reopen_preserves_counters() {
    let mut pair = Pair::new();
    let key = enable(&mut pair.alice, store::Policy::default());
    let wire = send(&mut pair.alice, "before failed send", false);
    receive(&mut pair.bob, &wire);
    pair.alice.actor.provider.inject_checkpoint_commit_failure();
    assert!(
        pair.alice
            .encrypt_text_referenced("no output", false, TIME)
            .is_err()
    );
    assert!(pair.alice.history_status(TIME).is_err());
    assert!(!pair.alice.is_active());
    let Pair {
        directory,
        profiles,
        anchor,
        alice,
        mut bob,
    } = pair;
    drop(alice);
    let mut alice = restore(directory.path(), &profiles, &anchor, 0);
    assert!(
        alice
            .encrypt_text_referenced("locked", false, TIME)
            .is_err()
    );
    alice.history_use_identity(key, TIME).unwrap();
    assert_eq!(rows(&mut alice), 1);
    let wire = send(&mut alice, "after verified reopen", false);
    assert_eq!(
        receive(&mut bob, &wire).content.text(),
        "after verified reopen"
    );
    assert_eq!(page(&mut alice).rows[1].record.id, 2);
}

#[test]
fn failed_receive_commit_exposes_no_plaintext_and_reopen_can_retry_exact_wire() {
    let mut pair = Pair::new();
    let key = enable(&mut pair.bob, store::Policy::default());
    let wire = send(&mut pair.alice, "not released on failed commit", false);
    pair.bob.actor.provider.inject_checkpoint_commit_failure();
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
    assert!(pair.bob.history_search(query(), TIME).is_err());
    let Pair {
        directory,
        profiles,
        anchor,
        alice: _,
        bob,
    } = pair;
    drop(bob);
    let mut bob = restore(directory.path(), &profiles, &anchor, 1);
    bob.history_use_identity(key, TIME).unwrap();
    assert_eq!(rows(&mut bob), 0);
    assert_eq!(
        receive(&mut bob, &wire).content.text(),
        "not released on failed commit"
    );
    assert_eq!(page(&mut bob).rows[0].record.id, 1);
}

#[test]
fn search_and_status_results_never_escape_failed_commit() {
    for status in [false, true] {
        let mut pair = Pair::new();
        let key = enable(&mut pair.alice, store::Policy::default());
        send(&mut pair.alice, "withheld page", false);
        pair.alice.actor.provider.inject_checkpoint_commit_failure();
        if status {
            assert!(pair.alice.history_status(TIME + 10).is_err());
        } else {
            assert!(pair.alice.history_search(query(), TIME + 10).is_err());
        }
        assert!(!pair.alice.is_active());
        let Pair {
            directory,
            profiles,
            anchor,
            alice,
            bob: _,
        } = pair;
        drop(alice);
        let mut alice = restore(directory.path(), &profiles, &anchor, 0);
        // Failed observation did not commit a clock floor.
        alice.history_use_identity(key, TIME).unwrap();
        assert_eq!(page(&mut alice).rows[0].record.body, "withheld page");
    }
}

#[test]
fn disable_clear_and_reopen_keep_key_binding_and_monotonic_ids() {
    let mut pair = Pair::new();
    let key = enable(&mut pair.alice, store::Policy::default());
    send(&mut pair.alice, "retained", false);
    pair.alice.history_disable(TIME).unwrap();
    send(&mut pair.alice, "disabled future", false);
    assert_eq!(rows(&mut pair.alice), 1);
    let Pair {
        directory,
        profiles,
        anchor,
        alice,
        bob: _,
    } = pair;
    drop(alice);
    let mut alice = restore(directory.path(), &profiles, &anchor, 0);
    assert!(matches!(
        alice.history_status(TIME).unwrap(),
        store::Status::Locked { enabled: false, .. }
    ));
    assert!(
        alice
            .encrypt_text_referenced("missing key", false, TIME)
            .is_err()
    );
    assert!(
        alice
            .history_use_identity(Identity::generate(), TIME)
            .is_err()
    );
    alice.history_use_identity(key, TIME).unwrap();
    assert!(matches!(
        alice.history_status(TIME).unwrap(),
        store::Status::Configured {
            enabled: false,
            rows: 1,
            ..
        }
    ));
    alice.history_clear(TIME).unwrap();
    alice
        .history_enable(store::Policy::default(), TIME)
        .unwrap();
    send(&mut alice, "new after clear", false);
    assert_eq!(page(&mut alice).rows[0].record.id, 2);
}

#[test]
fn protected_file_load_requires_existing_policy_and_same_opened_directory() {
    let mut pair = Pair::new();
    let path = pair.directory.path().join("state-0");
    assert!(pair.alice.history_open_file(&path, TIME).is_err());
    assert!(!path.join("history.agekey").exists());
    assert!(
        pair.alice
            .history_enable_file(
                &pair.directory.path().join("state-1"),
                store::Policy::default(),
                TIME
            )
            .is_err()
    );
    assert!(
        !pair
            .directory
            .path()
            .join("state-1/history.agekey")
            .exists()
    );
    pair.alice
        .history_enable_file(&path, store::Policy::default(), TIME)
        .unwrap();
    send(&mut pair.alice, "protected file", false);
    pair.alice.history_disable(TIME).unwrap();
    let Pair {
        directory,
        profiles,
        anchor,
        alice,
        bob: _,
    } = pair;
    drop(alice);
    let mut alice = restore(directory.path(), &profiles, &anchor, 0);
    assert!(
        alice
            .encrypt_text_referenced("needs verified key load", false, TIME)
            .is_err()
    );
    alice.history_open_file(&path, TIME).unwrap();
    assert!(matches!(
        alice.history_status(TIME).unwrap(),
        store::Status::Configured {
            enabled: false,
            rows: 1,
            ..
        }
    ));
    alice
        .history_enable(store::Policy::default(), TIME)
        .unwrap();
    send(&mut alice, "second protected file", false);
    assert_eq!(rows(&mut alice), 2);
}

#[test]
fn orphan_key_never_auto_adopted_and_recovery_requires_expected_public_recipient() {
    let mut pair = Pair::new();
    let path = pair.directory.path().join("state-0");
    pair.alice.actor.provider.inject_checkpoint_commit_failure();
    assert!(
        pair.alice
            .history_enable_file(&path, store::Policy::default(), TIME)
            .is_err()
    );
    assert!(path.join("history.agekey").exists());
    let expected = HistoryKeyAnchor::open(
        &path,
        pair.profiles[0].peer().fingerprint_bytes(),
        pair.anchor.id(),
    )
    .unwrap()
    .load()
    .unwrap()
    .recipient();
    let Pair {
        directory,
        profiles,
        anchor,
        alice,
        bob: _,
    } = pair;
    drop(alice);
    let mut alice = restore(directory.path(), &profiles, &anchor, 0);
    assert!(matches!(
        alice.history_status(TIME).unwrap(),
        store::Status::Off
    ));
    assert!(alice.history_open_file(&path, TIME).is_err());
    assert!(
        alice
            .history_enable_file(&path, store::Policy::default(), TIME)
            .is_err()
    );
    assert!(
        alice
            .history_recover_file(
                &path,
                &Identity::generate().to_public(),
                store::Policy::default(),
                TIME
            )
            .is_err()
    );
    assert!(matches!(
        alice.history_status(TIME).unwrap(),
        store::Status::Off
    ));
    alice
        .history_recover_file(&path, &expected, store::Policy::default(), TIME)
        .unwrap();
    assert_eq!(rows(&mut alice), 0);
    assert!(
        alice
            .history_recover_file(&path, &expected, store::Policy::default(), TIME)
            .is_err()
    );
}

#[test]
fn retained_pending_key_blocks_normal_enable_without_enrollment_or_regeneration() {
    use std::os::unix::fs::MetadataExt;
    for length in [0, 17, usize::MAX] {
        let mut pair = Pair::new();
        let path = pair.directory.path().join("state-0");
        let source = tempfile::tempdir().unwrap();
        fs::set_permissions(source.path(), fs::Permissions::from_mode(0o700)).unwrap();
        HistoryKeyAnchor::open(
            source.path(),
            pair.profiles[0].peer().fingerprint_bytes(),
            pair.anchor.id(),
        )
        .unwrap()
        .initialize()
        .unwrap();
        let complete =
            zeroize::Zeroizing::new(fs::read(source.path().join("history.agekey")).unwrap());
        let bytes = &complete[..length.min(complete.len())];
        let pending = path.join(".history-key-0123456789abcdef0123456789abcdef.pending");
        fs::write(&pending, bytes).unwrap();
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::metadata(&pending).unwrap();
        let entries = fs::read_dir(&path).unwrap().count();
        for _ in 0..3 {
            assert!(matches!(
                pair.alice.history_enable_file(&path, store::Policy::default(), TIME),
                Err(Error::Destination(message)) if message.contains("explicit recovery required")
            ));
            assert!(pair.alice.history_open_file(&path, TIME).is_err());
            assert!(matches!(
                pair.alice.history_status(TIME).unwrap(),
                store::Status::Off
            ));
            assert!(!path.join("history.agekey").exists());
            assert_eq!(fs::read_dir(&path).unwrap().count(), entries);
        }
        let after = fs::metadata(&pending).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        let after_bytes = zeroize::Zeroizing::new(fs::read(&pending).unwrap());
        assert!(after_bytes.as_slice() == bytes);
    }
}

#[test]
fn missing_replaced_or_unprotected_file_fails_before_sending_or_consuming_wire() {
    let mut pair = Pair::new();
    let path = pair.directory.path().join("state-1");
    pair.bob
        .history_enable_file(&path, store::Policy::default(), TIME)
        .unwrap();
    let keypath = path.join("history.agekey");
    let saved = path.join("synthetic-saved-key");
    fs::rename(&keypath, &saved).unwrap();
    let wire = send(&mut pair.alice, "retry after missing file", false);
    assert!(
        pair.bob
            .encrypt_text_referenced("must fail", false, TIME)
            .is_err()
    );
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
    assert!(!keypath.exists());
    symlink(&saved, &keypath).unwrap();
    assert!(pair.bob.history_status(TIME).is_err());
    fs::rename(&keypath, path.join("synthetic-link")).unwrap();
    fs::rename(&saved, &keypath).unwrap();
    fs::set_permissions(&keypath, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(pair.bob.decrypt_text_referenced(&wire, None, TIME).is_err());
    fs::set_permissions(&keypath, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        receive(&mut pair.bob, &wire).content.text(),
        "retry after missing file"
    );
    assert_eq!(rows(&mut pair.bob), 1);
}

#[test]
fn retained_anchor_rejects_root_swap_even_when_replacement_is_private() {
    let mut pair = Pair::new();
    let path = pair.directory.path().join("state-0");
    let moved = pair.directory.path().join("state-moved");
    pair.alice
        .history_enable_file(&path, store::Policy::default(), TIME)
        .unwrap();
    fs::rename(&path, &moved).unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        pair.alice
            .encrypt_text_referenced("not released", false, TIME)
            .is_err()
    );
    assert!(pair.alice.history_status(TIME).is_err());
    fs::rename(&path, pair.directory.path().join("replacement")).unwrap();
    fs::rename(&moved, &path).unwrap();
    let wire = send(&mut pair.alice, "original anchor restored", false);
    assert_eq!(
        receive(&mut pair.bob, &wire).content.text(),
        "original anchor restored"
    );
}

#[test]
fn empty_unicode_maximally_escaped_bodies_are_not_silently_dropped() {
    let mut pair = Pair::new();
    enable(&mut pair.alice, store::Policy::default());
    enable(&mut pair.bob, store::Policy::default());
    for body in [
        String::new(),
        "café 🦀 你好".to_owned(),
        "\0".repeat(config::MAX_MESSAGE_BYTES),
    ] {
        let wire = send(&mut pair.alice, &body, false);
        assert_eq!(receive(&mut pair.bob, &wire).content.text(), body);
        let all = page(&mut pair.bob);
        assert_eq!(all.rows.last().unwrap().record.body, body);
    }
    assert_eq!(rows(&mut pair.bob), 3);
    assert!(
        pair.alice
            .encrypt_text_referenced(&"x".repeat(config::MAX_MESSAGE_BYTES + 1), false, TIME)
            .is_err()
    );
    assert_eq!(rows(&mut pair.alice), 3);
}

#[test]
fn reference_map_is_bounded_pruned_and_replay_cannot_relabel_another_record() {
    let mut pair = Pair::new();
    enable(
        &mut pair.alice,
        store::Policy {
            rows: 2,
            ..store::Policy::default()
        },
    );
    send(&mut pair.alice, "one", false);
    let old = pair
        .alice
        .actor
        .provider
        .read_blob(REFERENCES)
        .unwrap()
        .unwrap();
    send(&mut pair.alice, "two", false);
    send(&mut pair.alice, "three", false);
    assert_eq!(rows(&mut pair.alice), 2);
    let context = pair.alice.history.clone().unwrap();
    assert_eq!(
        pair.alice
            .history_references(&context)
            .unwrap()
            .entries
            .len(),
        2
    );
    let transaction = pair.alice.actor.provider.transaction().unwrap();
    pair.alice
        .actor
        .provider
        .write_blob(REFERENCES, &old)
        .unwrap();
    transaction.commit().unwrap();
    assert!(pair.alice.history_search(query(), TIME).is_err());
    assert!(
        pair.alice
            .encrypt_text_referenced("cannot lose author context", false, TIME)
            .is_err()
    );
}

#[test]
fn group_page_budget_includes_references_and_pagination_has_no_silent_loss() {
    let mut pair = Pair::new();
    enable(&mut pair.alice, store::Policy::default());
    for body in ["one", "two", "three"] {
        send(&mut pair.alice, body, false);
    }
    let first = pair
        .alice
        .history_search(
            store::Query {
                limit: 1,
                ..query()
            },
            TIME,
        )
        .unwrap();
    let bytes = response_size(&first).unwrap();
    let first = pair
        .alice
        .history_search(
            store::Query {
                response_bytes: bytes,
                ..query()
            },
            TIME,
        )
        .unwrap();
    assert_eq!(first.rows.len(), 1);
    assert!(first.has_more);
    assert!(response_size(&first).unwrap() <= bytes);
    let second = pair
        .alice
        .history_search(
            store::Query {
                after_id: first.next_after,
                ..query()
            },
            TIME,
        )
        .unwrap();
    assert_eq!(second.rows.len(), 2);
    assert_eq!(second.rows[0].record.body, "two");
    assert!(
        pair.alice
            .history_search(
                store::Query {
                    response_bytes: bytes - 1,
                    limit: 1,
                    ..query()
                },
                TIME
            )
            .is_err()
    );
}

#[test]
fn sdk_and_file_bindings_refuse_cross_profile_and_room_contexts() {
    let mut pair = Pair::new();
    let key = enable(&mut pair.alice, store::Policy::default());
    enable(&mut pair.bob, store::Policy::default());
    assert!(pair.bob.history_use_identity(key.clone(), TIME).is_err());
    let mut another = Pair::new();
    another.alice.history_use_identity(key, TIME).unwrap();
    let context = pair.alice.history.clone().unwrap();
    assert!(context.verify(&pair.bob).is_err());
    assert!(context.verify(&another.alice).is_err());
    let path = pair.directory.path().join("state-1");
    assert!(
        another
            .alice
            .history_enable_file(&path, store::Policy::default(), TIME)
            .is_err()
    );
    assert!(!path.join("history.agekey").exists());
}

#[test]
fn sdk_foundation_records_without_group_author_context_are_not_adopted() {
    let mut pair = Pair::new();
    let key = Identity::generate();
    let binding = store::Binding::new(
        pair.profiles[0].peer().fingerprint_bytes(),
        pair.anchor.id(),
        &key.to_public(),
    );
    let transaction = pair.alice.actor.provider.transaction().unwrap();
    pair.alice
        .actor
        .provider
        .history_enable(&transaction, &binding, &key, store::Policy::default(), TIME)
        .unwrap();
    pair.alice
        .actor
        .provider
        .history_record(
            &transaction,
            &binding,
            Some(&key),
            store::Capture {
                event: store::Event::IncomingProcessed,
                epoch: 1,
                wire_sha256: [0; 32],
                body: "no proven MLS author",
                sensitive: false,
            },
            TIME,
        )
        .unwrap();
    transaction.commit().unwrap();
    pair.alice.history_use_identity(key, TIME).unwrap();
    assert!(pair.alice.history_search(query(), TIME).is_err());
    assert!(
        pair.alice
            .history_enable(store::Policy::default(), TIME)
            .is_err()
    );
    assert!(
        pair.alice
            .encrypt_text_referenced("refuse ambiguous history", false, TIME)
            .is_err()
    );
}
