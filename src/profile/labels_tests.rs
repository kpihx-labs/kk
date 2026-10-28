use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Barrier},
    thread,
};

fn profile() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

#[test]
fn announcements_are_validated_and_local_aliases_override_them() {
    let root = profile();
    let room = [1; 32];
    let me = raft::PeerId([2; 32]);
    let remote = raft::PeerId([3; 32]);
    let named = raft::PeerId([4; 32]);
    let announcements = BTreeMap::from([
        (me, "Not my local name".into()),
        (remote, "Remote name".into()),
        (named, "Announced name".into()),
    ]);
    set(root.path(), room, remote, Some("Nickname".into())).unwrap();
    let result =
        present_with_announcements(root.path(), room, me, &[me, remote, named], &announcements)
            .unwrap();
    assert_eq!(result[0].source, PresentationSource::SelfProfile);
    assert_eq!(result[1].display_name, "Nickname");
    assert_eq!(result[1].source, PresentationSource::LocalOverride);
    assert_eq!(result[2].display_name, "Announced name");
    assert_eq!(
        serde_json::to_value(result[2].source).unwrap(),
        "self_announcement"
    );
    let invalid = BTreeMap::from([(named, "x".repeat(65))]);
    assert!(present_with_announcements(root.path(), room, me, &[named], &invalid).is_err());
}

fn write_raw(profile: &Path, bytes: &[u8]) {
    fs::write(profile.join(FILE), bytes).unwrap();
    fs::set_permissions(profile.join(FILE), fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn reads_do_not_create_and_set_requires_existing_profile_without_keys() {
    let root = profile();
    let missing = root.path().join("missing");
    assert!(list(&missing, [1; 32]).unwrap().is_empty());
    assert!(!missing.exists());
    assert!(
        set(
            &missing,
            [1; 32],
            raft::PeerId([2; 32]),
            Some("Name".into())
        )
        .is_err()
    );
    assert!(!missing.exists());
    assert!(list(root.path(), [1; 32]).unwrap().is_empty());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    // Metadata is sufficient: no certificate or private key is consulted.
    set(
        root.path(),
        [1; 32],
        raft::PeerId([2; 32]),
        Some("Name".into()),
    )
    .unwrap();
    assert!(!root.path().join("identity").exists());
    assert!(!root.path().join("contacts.json").exists());
    assert!(!root.path().join("rooms").exists());
}

#[test]
fn exact_full_bindings_profile_room_isolation_and_removal() {
    let first = profile();
    let second = profile();
    let room = [1; 32];
    let mut other_room = room;
    other_room[31] = 2;
    let peer = raft::PeerId([3; 32]);
    let mut other_peer = peer;
    other_peer.0[31] = 4;
    for (r, p, name) in [
        (room, peer, "First"),
        (room, other_peer, "Other peer"),
        (other_room, peer, "Other room"),
    ] {
        set(first.path(), r, p, Some(name.into())).unwrap();
    }
    assert!(list(second.path(), room).unwrap().is_empty());
    set(second.path(), room, peer, Some("Second profile".into())).unwrap();
    set(first.path(), room, peer, Some("Renamed".into())).unwrap();
    assert_eq!(list(first.path(), room).unwrap()[&peer], "Renamed");
    set(first.path(), room, peer, None).unwrap();
    set(first.path(), room, peer, None).unwrap();
    assert_eq!(
        list(first.path(), room).unwrap(),
        BTreeMap::from([(other_peer, "Other peer".into())])
    );
    assert_eq!(list(first.path(), other_room).unwrap()[&peer], "Other room");
    assert_eq!(list(second.path(), room).unwrap()[&peer], "Second profile");
}

#[test]
fn corrupt_unknown_and_unsupported_data_is_not_reset() {
    let root = profile();
    let peer = raft::PeerId([2; 32]);
    let valid = serde_json::json!({"version": VERSION, "aliases": [{
        "room": ([1; 32]), "peer": peer, "name": "Saved"
    }]});
    let mut unknown = valid.clone();
    unknown["unknown"] = true.into();
    let mut future = valid.clone();
    future["version"] = (VERSION + 1).into();
    let mut duplicate = valid.clone();
    duplicate["aliases"]
        .as_array_mut()
        .unwrap()
        .push(valid["aliases"][0].clone());
    let mut unsafe_name = valid.clone();
    unsafe_name["aliases"][0]["name"] = "unsafe\u{202e}".into();
    let mut short_peer = valid.clone();
    short_peer["aliases"][0]["peer"] = "0202".into();
    let mut unknown_entry = valid.clone();
    unknown_entry["aliases"][0]["trusted"] = true.into();
    let mut cases = vec![
        b"not json".to_vec(),
        b"{}".to_vec(),
        vec![b' '; 128 * 1024 + 1],
    ];
    for value in [
        unknown,
        future,
        duplicate,
        unsafe_name,
        short_peer,
        unknown_entry,
    ] {
        cases.push(serde_json::to_vec(&value).unwrap());
    }
    for bytes in cases {
        write_raw(root.path(), &bytes);
        // Even a different room must not hide invalid stored entries.
        assert!(list(root.path(), [9; 32]).is_err());
        assert!(set(root.path(), [9; 32], peer, Some("New".into())).is_err());
        assert!(set(root.path(), [1; 32], peer, None).is_err());
        assert_eq!(fs::read(root.path().join(FILE)).unwrap(), bytes);
    }
}

#[test]
fn concurrent_distinct_updates_preserve_every_binding() {
    let root = profile();
    let barrier = Arc::new(Barrier::new(16));
    thread::scope(|scope| {
        for index in 0..16 {
            let barrier = Arc::clone(&barrier);
            let path = root.path();
            scope.spawn(move || {
                barrier.wait();
                set(
                    path,
                    [1; 32],
                    raft::PeerId([index; 32]),
                    Some(format!("Peer {index}")),
                )
                .unwrap();
            });
        }
    });
    let saved = list(root.path(), [1; 32]).unwrap();
    assert_eq!(saved.len(), 16);
    for index in 0..16 {
        assert_eq!(saved[&raft::PeerId([index; 32])], format!("Peer {index}"));
    }
}

#[test]
fn reusable_name_validation_rejects_spoofing_but_preserves_international_names() {
    let root = profile();
    let peer = raft::PeerId([2; 32]);
    for name in [
        "Élodie",
        "李明",
        "محمد",
        "שָׁלוֹם",
        "क्\u{200d}ष",
        "نیم\u{200c}فاصله",
        "e\u{301}",
    ] {
        set(root.path(), [1; 32], peer, Some(name.into())).unwrap();
        assert_eq!(list(root.path(), [1; 32]).unwrap()[&peer], name);
        super::super::Settings::update(root.path(), Some(name.into()), None).unwrap();
    }
    let before = fs::read(root.path().join(FILE)).unwrap();
    let mut unsafe_names = vec![
        String::new(),
        " \t".into(),
        "\u{2003}".into(),
        "x".repeat(65),
    ];
    for c in [
        '\0', '\n', '\u{7f}', '\u{85}', '\u{061c}', '\u{200b}', '\u{200e}', '\u{200f}', '\u{202a}',
        '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2060}', '\u{2066}', '\u{2067}',
        '\u{2068}', '\u{2069}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}', '\u{feff}',
    ] {
        unsafe_names.push(format!("Name{c}"));
    }
    for name in unsafe_names {
        assert!(set(root.path(), [1; 32], peer, Some(name.clone())).is_err());
        assert!(super::super::Settings::update(root.path(), Some(name), None).is_err());
        assert_eq!(fs::read(root.path().join(FILE)).unwrap(), before);
    }
}

#[test]
fn cardinality_and_aggregate_limits_preserve_existing_bytes() {
    let root = profile();
    let mut labels = Labels::default();
    for index in 0..MAX_LABELS {
        labels.aliases.push(Alias {
            room: [0; 32],
            peer: raft::PeerId::from(index as u64),
            name: "N".into(),
        });
    }
    let bytes = serde_json::to_vec(&labels).unwrap();
    assert!(bytes.len() < 128 * 1024);
    write_raw(root.path(), &bytes);
    assert!(
        set(
            root.path(),
            [1; 32],
            raft::PeerId([1; 32]),
            Some("Extra".into())
        )
        .is_err()
    );
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), bytes);
    labels.aliases.push(Alias {
        room: [1; 32],
        peer: raft::PeerId([1; 32]),
        name: "Extra".into(),
    });
    write_raw(root.path(), &serde_json::to_vec(&labels).unwrap());
    assert!(list(root.path(), [0; 32]).is_err());
    labels.aliases.truncate(400);
    for alias in &mut labels.aliases {
        alias.room = [255; 32];
        alias.name = "界".repeat(64);
    }
    let bytes = serde_json::to_vec(&labels).unwrap();
    assert!(bytes.len() > 128 * 1024);
    write_raw(root.path(), &bytes);
    assert!(list(root.path(), [255; 32]).is_err());
    // A valid file near the aggregate bound cannot grow past it on update.
    while serde_json::to_vec(&labels).unwrap().len() > 128 * 1024 {
        labels.aliases.pop();
    }
    let bytes = serde_json::to_vec(&labels).unwrap();
    write_raw(root.path(), &bytes);
    assert!(
        set(
            root.path(),
            [255; 32],
            raft::PeerId([255; 32]),
            Some("界".repeat(64))
        )
        .is_err()
    );
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), bytes);
}

#[test]
fn presentation_has_explicit_provenance_and_only_requested_members_in_order() {
    let root = profile();
    let room = [1; 32];
    let self_peer = raft::PeerId([2; 32]);
    let remote = raft::PeerId([3; 32]);
    let absent = raft::PeerId([4; 32]);
    let mut unknown = remote;
    unknown.0[31] = 5;
    super::super::Settings::update(root.path(), Some("My name".into()), None).unwrap();
    for (peer, name) in [
        (self_peer, "Ignored self alias"),
        (remote, "Local nickname"),
        (absent, "Absent alias"),
    ] {
        set(root.path(), room, peer, Some(name.into())).unwrap();
    }
    let before = fs::read(root.path().join(FILE)).unwrap();
    let members = [remote, unknown, self_peer];
    let result = present(root.path(), room, self_peer, &members).unwrap();
    assert_eq!(
        result,
        vec![
            MemberPresentation {
                id: remote,
                display_name: "Local nickname".into(),
                source: PresentationSource::LocalOverride,
                is_self: false
            },
            MemberPresentation {
                id: unknown,
                display_name: "Unnamed member".into(),
                source: PresentationSource::Unknown,
                is_self: false
            },
            MemberPresentation {
                id: self_peer,
                display_name: "My name".into(),
                source: PresentationSource::SelfProfile,
                is_self: true
            },
        ]
    );
    let serialized = serde_json::to_value(&result).unwrap();
    assert_eq!(serialized[0]["source"], "local_override");
    assert_eq!(serialized[1]["source"], "unknown");
    assert_eq!(serialized[2]["source"], "self_profile");
    assert_eq!(serialized[0]["id"], remote.to_string());
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), before);
    assert_eq!(list(root.path(), room).unwrap()[&absent], "Absent alias");
    let other_room = present(root.path(), [9; 32], self_peer, &[remote]).unwrap();
    assert_eq!(other_room[0].source, PresentationSource::Unknown);
    assert_eq!(other_room[0].display_name, "Unnamed member");
    let other_profile = profile();
    let result = present(other_profile.path(), room, self_peer, &[remote]).unwrap();
    assert_eq!(result[0].source, PresentationSource::Unknown);
    assert!(!result[0].is_self);
    assert_eq!(fs::read_dir(other_profile.path()).unwrap().count(), 0);
}

#[test]
fn presentation_bounds_members_and_never_invents_self_or_writes_metadata() {
    let root = profile();
    let missing = root.path().join("missing");
    let self_peer = raft::PeerId([1; 32]);
    assert!(
        present(&missing, [2; 32], self_peer, &[])
            .unwrap()
            .is_empty()
    );
    let members: Vec<_> = (0..crate::config::ROOM_MAX_MEMBERS)
        .map(|index| raft::PeerId::from(index as u64))
        .collect();
    let result = present(&missing, [2; 32], self_peer, &members).unwrap();
    assert_eq!(result.len(), crate::config::ROOM_MAX_MEMBERS);
    assert_eq!(
        result.iter().map(|member| member.id).collect::<Vec<_>>(),
        members
    );
    assert!(
        result
            .iter()
            .all(|member| !member.is_self && member.source == PresentationSource::Unknown)
    );
    assert!(present(&missing, [2; 32], self_peer, &[self_peer, self_peer]).is_err());
    let oversized: Vec<_> = (0..=crate::config::ROOM_MAX_MEMBERS)
        .map(|index| raft::PeerId::from(index as u64))
        .collect();
    assert!(present(&missing, [2; 32], self_peer, &oversized).is_err());
    let own = present(&missing, [2; 32], self_peer, &[self_peer]).unwrap();
    assert_eq!(
        own[0].display_name,
        super::super::Settings::default().display_name
    );
    assert_eq!(own[0].source, PresentationSource::SelfProfile);
    assert!(own[0].is_self);
    assert!(!missing.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn presentation_propagates_invalid_metadata_without_resetting_it() {
    let root = profile();
    let peer = raft::PeerId([1; 32]);
    write_raw(root.path(), b"invalid labels");
    assert!(present(root.path(), [2; 32], peer, &[peer]).is_err());
    assert_eq!(fs::read(root.path().join(FILE)).unwrap(), b"invalid labels");
    write_raw(root.path(), br#"{"version":1,"aliases":[]}"#);
    super::super::storage::update(
        root.path(),
        "settings.json",
        |settings: &mut super::super::Settings| {
            settings.display_name = "unsafe\u{202e}".into();
            Ok(())
        },
    )
    .unwrap();
    let before = fs::read(root.path().join("settings.json")).unwrap();
    assert!(present(root.path(), [2; 32], peer, &[peer]).is_err());
    assert_eq!(fs::read(root.path().join("settings.json")).unwrap(), before);
}
