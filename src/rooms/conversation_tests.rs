use super::*;

fn target() -> Target {
    Target {
        room: [7; 32],
        member: [8; 32],
        epoch: 3,
        wire_sha256: [9; 32],
    }
}

#[test]
fn target_ids_are_canonical_bounded_and_lossless_at_u64_epoch_limit() {
    let mut original = target();
    original.epoch = u64::MAX;
    let id = original.to_string();
    assert!(id.len() <= MAX_TARGET_ID_BYTES);
    assert_eq!(id.parse::<Target>().unwrap(), original);
    for invalid in [
        id.to_uppercase(),
        format!("{id}.extra"),
        id.replacen("18446744073709551615", "018446744073709551615", 1),
        "x".repeat(MAX_TARGET_ID_BYTES + 1),
    ] {
        assert!(invalid.parse::<Target>().is_err());
    }
}

#[test]
fn negotiated_audience_rejects_room_epoch_and_member_changes() {
    let members = BTreeSet::from([[1; 32], [2; 32]]);
    let proof = Audience::negotiated([7; 32], 4, members.clone()).unwrap();
    proof.validate([7; 32], 4, members.clone()).unwrap();
    assert!(proof.validate([6; 32], 4, members.clone()).is_err());
    assert!(proof.validate([7; 32], 5, members.clone()).is_err());
    assert!(
        proof
            .validate([7; 32], 4, BTreeSet::from([[1; 32]]))
            .is_err()
    );
    assert!(Audience::negotiated([7; 32], 4, BTreeSet::new()).is_err());
}

#[test]
fn reaction_projection_deduplicates_and_removes_only_the_authenticated_author() {
    let reference = super::super::group::StableReference {
        room: [7; 32],
        member: [1; 32],
        epoch: 3,
        event: super::super::store::history::Event::IncomingProcessed,
        wire_sha256: [4; 32],
    };
    let operation = |added, sensitive| Operation::Reaction {
        target: target(),
        value: Reaction::Like,
        added,
        sensitive,
    };
    let mut state = Reactions::default();
    state.apply(&reference, &operation(true, false)).unwrap();
    state.apply(&reference, &operation(true, false)).unwrap();
    let mut other = reference.clone();
    other.member = [2; 32];
    state.apply(&other, &operation(true, false)).unwrap();
    state.apply(&reference, &operation(false, false)).unwrap();
    let summaries = state.summaries();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].members, vec![[2; 32]]);
    let mut sensitive = Reactions::default();
    sensitive.apply(&reference, &operation(true, true)).unwrap();
    assert!(sensitive.summaries().is_empty());
}

#[test]
fn reaction_projection_keeps_targets_values_and_event_order_independent() {
    let mut state = Reactions::default();
    let mut reference = super::super::group::StableReference {
        room: [7; 32],
        member: [1; 32],
        epoch: 4,
        event: super::super::store::history::Event::IncomingProcessed,
        wire_sha256: [4; 32],
    };
    let mut second = target();
    second.wire_sha256[0] ^= 1;
    let operation = |target, value, added| Operation::Reaction {
        target,
        value,
        added,
        sensitive: false,
    };
    // Removing an absent value is a no-op, not a tombstone that hides a later add.
    state
        .apply(&reference, &operation(target(), Reaction::Like, false))
        .unwrap();
    state
        .apply(&reference, &operation(target(), Reaction::Like, true))
        .unwrap();
    state
        .apply(&reference, &operation(target(), Reaction::Love, true))
        .unwrap();
    state
        .apply(&reference, &operation(second.clone(), Reaction::Like, true))
        .unwrap();
    state
        .apply(&reference, &operation(target(), Reaction::Like, false))
        .unwrap();
    reference.member = [2; 32];
    state
        .apply(&reference, &operation(target(), Reaction::Love, false))
        .unwrap();
    let summaries = state.summaries();
    assert_eq!(summaries.len(), 2);
    assert!(summaries.iter().any(|summary| summary.target == target()
        && summary.value == Reaction::Love
        && summary.members == vec![[1; 32]]));
    assert!(summaries.iter().any(|summary| summary.target == second
        && summary.value == Reaction::Like
        && summary.members == vec![[1; 32]]));
}

#[test]
fn reaction_projection_rejects_scope_and_capacity_without_mutating_existing_entries() {
    let mut state = Reactions::default();
    let mut reference = super::super::group::StableReference {
        room: [7; 32],
        member: [0; 32],
        epoch: 3,
        event: super::super::store::history::Event::IncomingProcessed,
        wire_sha256: [4; 32],
    };
    let operation = Operation::Reaction {
        target: target(),
        value: Reaction::Like,
        added: true,
        sensitive: false,
    };
    reference.room = [6; 32];
    assert!(state.apply(&reference, &operation).is_err());
    reference.room = [7; 32];
    for index in 0..MAX_REACTION_ENTRIES {
        reference.member[..8].copy_from_slice(&(index as u64).to_be_bytes());
        state.apply(&reference, &operation).unwrap();
    }
    reference.member = [255; 32];
    assert!(matches!(
        state.apply(&reference, &operation),
        Err(Error::Limit { .. })
    ));
    // Existing duplicates remain idempotent at the cap; a removal releases one
    // slot, and failure above cannot leave a partially inserted new member.
    reference.member = [0; 32];
    state.apply(&reference, &operation).unwrap();
    let removal = Operation::Reaction {
        target: target(),
        value: Reaction::Like,
        added: false,
        sensitive: false,
    };
    state.apply(&reference, &removal).unwrap();
    reference.member = [255; 32];
    state.apply(&reference, &operation).unwrap();
    assert_eq!(
        state
            .summaries()
            .iter()
            .map(|entry| entry.members.len())
            .sum::<usize>(),
        MAX_REACTION_ENTRIES
    );
}
fn text(value: &str, sensitive: bool) -> ReceivedText {
    ReceivedText::from_bytes(value.as_bytes(), sensitive).unwrap()
}

#[test]
fn directions_share_one_exact_target_without_claiming_human_reading() {
    let mut reference = super::super::group::StableReference {
        room: [7; 32],
        member: [8; 32],
        epoch: 3,
        event: super::super::store::history::Event::OutgoingPrepared,
        wire_sha256: [9; 32],
    };
    let outgoing = Target::from(&reference);
    reference.event = super::super::store::history::Event::IncomingProcessed;
    assert_eq!(outgoing, Target::from(&reference));
    reference.wire_sha256[0] ^= 1;
    assert_ne!(outgoing, Target::from(&reference));
}

#[test]
fn text_and_replies_roundtrip_all_bounded_utf8_without_json_magic_or_debug_content() {
    for body in [
        String::new(),
        "Unicode and \\ JSON-looking {\"kind\":\"reply\"}".into(),
        "x".repeat(config::MAX_MESSAGE_BYTES),
    ] {
        for sensitive in [false, true] {
            for reply in [false, true] {
                let operation = if reply {
                    Operation::Reply {
                        target: target(),
                        text: text(&body, sensitive),
                    }
                } else {
                    Operation::Text(text(&body, sensitive))
                };
                operation.validate_publication().unwrap();
                let bytes = operation.encode([7; 32], 3).unwrap();
                assert!(bytes.len() <= MAX_ENCODED_BYTES);
                let decoded = Operation::decode(&bytes, [7; 32], 3).unwrap();
                assert_eq!(decoded.sensitive(), sensitive);
                match decoded {
                    Operation::Text(value) | Operation::Reply { text: value, .. } => {
                        assert_eq!(value.text(), body)
                    }
                    _ => panic!("wrong operation"),
                }
            }
        }
    }
    let secret = Operation::Reply {
        target: target(),
        text: text("never expose this body", true),
    };
    assert!(!format!("{secret:?}").contains("never expose this body"));
    assert!(matches!(
        Operation::decode(br#"{"kind":"reply"}"#, [7; 32], 3),
        Err(Error::Protocol)
    ));
}

#[test]
fn reactions_have_finite_values_add_remove_and_no_caller_supplied_author() {
    for value in [
        Reaction::Like,
        Reaction::Love,
        Reaction::Laugh,
        Reaction::Surprised,
        Reaction::Sad,
        Reaction::Thanks,
    ] {
        for added in [false, true] {
            let operation = Operation::Reaction {
                target: target(),
                value,
                added,
                sensitive: true,
            };
            assert!(matches!(
                operation.validate_publication(),
                Err(Error::Configuration(_))
            ));
            let bytes = operation.encode([7; 32], 3).unwrap();
            match Operation::decode(&bytes, [7; 32], 3).unwrap() {
                Operation::Reaction {
                    target: got,
                    value: actual,
                    added: action,
                    sensitive,
                } => {
                    assert_eq!(got, target());
                    assert_eq!(actual, value);
                    assert_eq!(action, added);
                    assert!(sensitive);
                }
                _ => panic!("wrong operation"),
            }
            let mut invalid = bytes.to_vec();
            *invalid.last_mut().unwrap() = 255;
            assert!(matches!(
                Operation::decode(&invalid, [7; 32], 3),
                Err(Error::Protocol)
            ));
            invalid = bytes.to_vec();
            invalid.push(0);
            assert!(matches!(
                Operation::decode(&invalid, [7; 32], 3),
                Err(Error::Protocol)
            ));
        }
    }
}

#[test]
fn invalid_headers_scope_epoch_utf8_and_work_bounds_fail_closed() {
    let valid = Operation::Reply {
        target: target(),
        text: text("body", false),
    }
    .encode([7; 32], 3)
    .unwrap();
    for length in 0..HEADER_BYTES + TARGET_BYTES {
        assert!(Operation::decode(&valid[..length], [7; 32], 3).is_err());
    }
    for (index, value) in [(0, 2), (1, 255), (2, 2)] {
        let mut bad = valid.to_vec();
        bad[index] = value;
        assert!(matches!(
            Operation::decode(&bad, [7; 32], 3),
            Err(Error::Protocol)
        ));
    }
    assert!(matches!(
        Operation::decode(&valid, [6; 32], 3),
        Err(Error::Denied)
    ));
    assert!(matches!(
        Operation::decode(&valid, [7; 32], 2),
        Err(Error::Denied)
    ));
    assert!(matches!(
        Operation::Reply {
            target: target(),
            text: text("body", false)
        }
        .encode([6; 32], 3),
        Err(Error::Denied)
    ));
    let mut bad = valid.to_vec();
    *bad.last_mut().unwrap() = 255;
    assert!(matches!(
        Operation::decode(&bad, [7; 32], 3),
        Err(Error::Protocol)
    ));
    assert!(matches!(
        Operation::decode(&vec![0; MAX_ENCODED_BYTES + 1], [7; 32], 3),
        Err(Error::MessageTooLarge)
    ));
}
