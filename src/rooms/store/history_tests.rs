use super::{history::*, *};
use age::x25519::Identity;
use openmls_basic_credential::SignatureKeyPair;
use openmls_traits::{signatures::Signer, types::SignatureScheme};
use zeroize::Zeroizing;

fn fixture() -> (Provider, Identity, Binding) {
    let identity = Identity::generate();
    let binding = Binding::new([1; 32], [2; 32], &identity.to_public());
    (Provider::memory().unwrap(), identity, binding)
}
fn capture(body: &str) -> Capture<'_> {
    Capture {
        event: Event::OutgoingPrepared,
        epoch: 7,
        wire_sha256: [9; 32],
        body,
        sensitive: false,
    }
}
fn query(text: &str) -> Query<'_> {
    Query {
        text,
        after_id: 0,
        limit: MAX_RESULTS,
        response_bytes: MAX_RESPONSE_BYTES,
    }
}
fn enable(p: &Provider, key: &Identity, b: &Binding, policy: Policy) {
    let tx = p.transaction().unwrap();
    p.history_enable(&tx, b, key, policy, 100).unwrap();
    tx.commit().unwrap();
}
fn insert(p: &Provider, key: &Identity, b: &Binding, body: &str, now: u64) -> u64 {
    let tx = p.transaction().unwrap();
    let Recorded::Stored {
        id,
        event: Event::OutgoingPrepared,
    } = p
        .history_record(&tx, b, Some(key), capture(body), now)
        .unwrap()
    else {
        panic!("not stored")
    };
    tx.commit().unwrap();
    id
}
fn modify_state(p: &Provider, edit: impl FnOnce(&mut serde_json::Value)) {
    let bytes: Vec<u8> = p
        .session
        .0
        .connection
        .query_row("SELECT state FROM kk_history_state", [], |r| r.get(0))
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    edit(&mut value);
    p.session
        .0
        .connection
        .execute(
            "UPDATE kk_history_state SET state=?1",
            [serde_json::to_vec(&value).unwrap()],
        )
        .unwrap();
}

fn align_synthetic_ciphertext_counter(p: &Provider) {
    let bytes: usize = p
        .session
        .0
        .connection
        .query_row(
            "SELECT coalesce(sum(length(ciphertext)),0) FROM kk_history_rows",
            [],
            |r| r.get(0),
        )
        .unwrap();
    modify_state(p, |state| state["bytes"] = serde_json::json!(bytes));
    // Substitution tests must reach encrypted validation, not merely trip a
    // length-counter mismatch caused by replacing differently sized records.
    super::history::validate_schema(&p.session.0.connection).unwrap();
}

#[test]
fn v1_is_off_without_keys_writes_or_implicit_migration() {
    let (p, _, b) = fixture();
    let tx = p.transaction().unwrap();
    assert_eq!(
        p.history_status(&tx, Some(&b), None, 0).unwrap(),
        Status::Off
    );
    assert_eq!(
        p.history_record(&tx, &b, None, capture("never persisted"), 0)
            .unwrap(),
        Recorded::Off
    );
    assert!(
        p.history_search(&tx, &b, None, query(""), 0)
            .unwrap()
            .rows
            .is_empty()
    );
    tx.commit().unwrap();
    let version: i64 = p
        .session
        .0
        .connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let tables: i64 = p
        .session
        .0
        .connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'kk_history%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}

#[test]
fn empty_pages_enforce_complete_frame_budget_including_v1() {
    let (p, key, b) = fixture();
    let empty = Page {
        rows: Vec::new(),
        next_after: 0,
        has_more: false,
    };
    let length = serde_json::to_vec(&empty).unwrap().len();
    for configured in [false, true] {
        if configured {
            enable(&p, &key, &b, Policy::default());
        }
        let tx = p.transaction().unwrap();
        for response_bytes in [1, length - 1] {
            let q = Query {
                response_bytes,
                ..query("")
            };
            assert!(p.history_search(&tx, &b, Some(&key), q, 100).is_err());
        }
        let q = Query {
            response_bytes: length,
            ..query("")
        };
        let page = p.history_search(&tx, &b, Some(&key), q, 100).unwrap();
        assert_eq!(serde_json::to_vec(&page).unwrap().len(), length);
        tx.commit().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn observed_expiry_clock_is_monotonic_in_transaction_and_after_committed_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic-clock");
    let key = Identity::generate();
    let b = Binding::new([1; 32], [2; 32], &key.to_public());
    let p = Provider::create(&path).unwrap();
    enable(
        &p,
        &key,
        &b,
        Policy {
            retention_seconds: 10,
            ..Policy::default()
        },
    );
    insert(&p, &key, &b, "expires at 111", 101);
    let tx = p.transaction().unwrap();
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 111)
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 105)
            .is_err()
    );
    assert!(p.history_status(&tx, None, Some(&key), 105).is_err());
    tx.commit().unwrap();
    drop(p);
    let p = Provider::open(&path).unwrap();
    let tx = p.transaction().unwrap();
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 105)
            .is_err()
    );
    p.history_status(&tx, None, Some(&key), 112).unwrap();
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 111)
            .is_err()
    );
    tx.commit().unwrap();
    drop(p);
    let p = Provider::open(&path).unwrap();
    let tx = p.transaction().unwrap();
    assert!(p.history_status(&tx, None, Some(&key), 111).is_err());
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 112)
            .unwrap()
            .rows
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn savepoint_cleanup_failure_cannot_commit_partial_history_or_mls_metadata() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic-cleanup");
    let key = Identity::generate();
    let b = Binding::new([1; 32], [2; 32], &key.to_public());
    let p = Provider::create(&path).unwrap();
    enable(&p, &key, &b, Policy::default());
    insert(&p, &key, &b, "must survive", 101);
    let tx = p.transaction().unwrap();
    p.set_metadata("group", &42u64).unwrap();
    tx.commit().unwrap();
    let signer = SignatureKeyPair::new(SignatureScheme::ED25519).unwrap();
    let public = signer.to_public_vec();
    let tx = p.transaction().unwrap();
    signer.store(p.storage()).unwrap();
    p.set_metadata("group", &99u64).unwrap();
    p.history_record(&tx, &b, Some(&key), capture("must rollback"), 102)
        .unwrap();
    assert!(p.inject_history_cleanup_failure(&tx).is_err());
    assert!(!p.session.0.connection.is_autocommit());
    assert!(p.session.0.poisoned.get());
    let dirty_rows: i64 = p
        .session
        .0
        .connection
        .query_row("SELECT count(*) FROM kk_history_rows", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dirty_rows, 0);
    assert!(tx.commit().is_err());
    assert!(p.session.0.connection.is_autocommit());
    assert!(p.transaction().is_err());
    drop(p);
    let p = Provider::open(&path).unwrap();
    assert_eq!(p.metadata::<u64>("group").unwrap(), Some(42));
    assert!(SignatureKeyPair::read(p.storage(), &public, SignatureScheme::ED25519).is_none());
    let tx = p.transaction().unwrap();
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 102)
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].body, "must survive");
}

#[test]
fn typed_roundtrip_sensitive_exclusion_and_disable_preserves_key_and_rows() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    assert_eq!(insert(&p, &key, &b, "bonjour 世界", 101), 1);
    let tx = p.transaction().unwrap();
    let mut incoming = capture("received");
    incoming.event = Event::IncomingProcessed;
    assert_eq!(
        p.history_record(&tx, &b, Some(&key), incoming, 102)
            .unwrap(),
        Recorded::Stored {
            id: 2,
            event: Event::IncomingProcessed
        }
    );
    let mut sensitive = capture("secret excluded");
    sensitive.sensitive = true;
    assert_eq!(
        p.history_record(&tx, &b, Some(&key), sensitive, 102)
            .unwrap(),
        Recorded::SensitiveExcluded
    );
    let check: Vec<u8> = p
        .session
        .0
        .connection
        .query_row("SELECT key_check FROM kk_history_state", [], |r| r.get(0))
        .unwrap();
    p.history_disable(&tx, &b, &key, 102).unwrap();
    assert_eq!(
        p.history_record(&tx, &b, Some(&key), capture("off"), 102)
            .unwrap(),
        Recorded::Off
    );
    let after: Vec<u8> = p
        .session
        .0
        .connection
        .query_row("SELECT key_check FROM kk_history_state", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, after);
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 102)
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    let row = &page.rows[0];
    assert_eq!(row.version, 1);
    assert_eq!(row.binding, b);
    assert_eq!((row.id, row.epoch, row.captured_at), (1, 7, 101));
    assert_eq!(row.wire_sha256, [9; 32]);
    assert_eq!(row.body, "bonjour 世界");
    assert_eq!(page.rows[1].event, Event::IncomingProcessed);
    let ciphertext: Vec<u8> = p
        .session
        .0
        .connection
        .query_row(
            "SELECT ciphertext FROM kk_history_rows WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(ciphertext.starts_with(b"age-encryption.org/v1\n"));
    assert!(
        !ciphertext
            .windows("bonjour 世界".len())
            .any(|w| w == "bonjour 世界".as_bytes())
    );
    tx.commit().unwrap();
}

#[test]
fn configured_empty_disabled_and_cleared_stores_remain_locked_without_matching_key() {
    let (p, key, b) = fixture();
    let wrong = Identity::generate();
    enable(&p, &key, &b, Policy::default());
    let tx = p.transaction().unwrap();
    for identity in [None, Some(&wrong)] {
        assert!(matches!(
            p.history_status(&tx, Some(&b), identity, 100).unwrap(),
            Status::Locked { enabled: true, .. }
        ));
        assert!(p.history_search(&tx, &b, identity, query(""), 100).is_err());
        assert!(
            p.history_record(&tx, &b, identity, capture("no"), 100)
                .is_err()
        );
    }
    p.history_disable(&tx, &b, &key, 100).unwrap();
    p.history_clear(&tx, &b, &key, 100).unwrap();
    assert!(matches!(p.history_status(&tx, None, None, 100).unwrap(),
        Status::Locked { binding, enabled: false, .. } if binding == b));
    assert!(
        matches!(p.history_status(&tx, None, Some(&key), 100).unwrap(),
        Status::Configured { binding, enabled: false, rows: 0, .. } if binding == b)
    );
    let wrong_context = Binding::new([5; 32], b.room, &key.to_public());
    assert!(
        p.history_status(&tx, Some(&wrong_context), None, 100)
            .is_err()
    );
    assert!(p.history_search(&tx, &b, None, query(""), 100).is_err());
    assert!(
        p.history_enable(&tx, &b, &wrong, Policy::default(), 100)
            .is_err()
    );
    tx.commit().unwrap();
}

#[test]
fn binding_and_record_id_substitutions_fail_closed_even_outside_requested_page() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    insert(&p, &key, &b, "first", 101);
    insert(&p, &key, &b, "second", 102);
    let tx = p.transaction().unwrap();
    for different in [
        Binding::new([4; 32], b.room, &key.to_public()),
        Binding::new(b.profile, [4; 32], &key.to_public()),
        Binding::new(b.profile, b.room, &Identity::generate().to_public()),
    ] {
        assert!(
            p.history_search(&tx, &different, Some(&key), query(""), 102)
                .is_err()
        );
    }
    p.session.0.connection.execute("UPDATE kk_history_rows SET ciphertext=(SELECT ciphertext FROM kk_history_rows WHERE id=1) WHERE id=2", []).unwrap();
    align_synthetic_ciphertext_counter(&p);
    let mut q = query("first");
    q.limit = 1;
    assert!(p.history_search(&tx, &b, Some(&key), q, 102).is_err());
}

#[test]
fn encrypted_row_copy_across_profile_or_room_is_rejected() {
    let key = Identity::generate();
    for binding in [
        Binding::new([8; 32], [2; 32], &key.to_public()),
        Binding::new([1; 32], [8; 32], &key.to_public()),
    ] {
        let p = Provider::memory().unwrap();
        let other = Provider::memory().unwrap();
        let original = Binding::new([1; 32], [2; 32], &key.to_public());
        enable(&p, &key, &original, Policy::default());
        enable(&other, &key, &binding, Policy::default());
        insert(&p, &key, &original, "same length", 101);
        insert(&other, &key, &binding, "same length", 101);
        let ciphertext: Vec<u8> = p
            .session
            .0
            .connection
            .query_row("SELECT ciphertext FROM kk_history_rows", [], |r| r.get(0))
            .unwrap();
        let tx = other.transaction().unwrap();
        other
            .session
            .0
            .connection
            .execute("UPDATE kk_history_rows SET ciphertext=?1", [ciphertext])
            .unwrap();
        align_synthetic_ciphertext_counter(&other);
        assert!(
            other
                .history_search(&tx, &binding, Some(&key), query(""), 101)
                .is_err()
        );
    }
}

#[test]
fn retention_oldest_first_clear_and_disable_never_reuse_ids() {
    let (p, key, b) = fixture();
    let policy = Policy {
        rows: 2,
        retention_seconds: 10,
        ..Policy::default()
    };
    enable(&p, &key, &b, policy);
    for now in 101..=103 {
        insert(&p, &key, &b, "kept", now);
    }
    let tx = p.transaction().unwrap();
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 103)
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![2, 3]
    );
    p.history_prune(&tx, &b, &key, 112).unwrap();
    assert_eq!(
        p.history_search(&tx, &b, Some(&key), query(""), 112)
            .unwrap()
            .rows[0]
            .id,
        3
    );
    p.history_clear(&tx, &b, &key, 112).unwrap();
    p.history_disable(&tx, &b, &key, 112).unwrap();
    p.history_enable(&tx, &b, &key, policy, 112).unwrap();
    tx.commit().unwrap();
    assert_eq!(insert(&p, &key, &b, "after clear", 113), 4);
    let tx = p.transaction().unwrap();
    assert!(
        p.history_record(&tx, &b, Some(&key), capture("clock reversal"), 112)
            .is_err()
    );
}

#[test]
fn encrypted_byte_limit_prunes_and_oversized_insert_is_operation_atomic() {
    let (p, key, b) = fixture();
    enable(
        &p,
        &key,
        &b,
        Policy {
            encrypted_bytes: 1600,
            ..Policy::default()
        },
    );
    insert(&p, &key, &b, &"a".repeat(300), 101);
    insert(&p, &key, &b, &"b".repeat(300), 102);
    let tx = p.transaction().unwrap();
    assert!(
        p.history_record(&tx, &b, Some(&key), capture(&"x".repeat(2000)), 103)
            .is_err()
    );
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 103)
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].id, 2);
    tx.commit().unwrap();
    assert_eq!(insert(&p, &key, &b, "next", 104), 3);
}

#[test]
fn utf8_literal_search_pagination_and_ipc_bounds() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    for body in [
        "été 世界",
        "O'Reilly %_'; DROP TABLE kk_history_rows;--",
        "été encore",
    ] {
        insert(&p, &key, &b, body, 101);
    }
    let tx = p.transaction().unwrap();
    let mut q = query("été");
    q.limit = 1;
    let page = p.history_search(&tx, &b, Some(&key), q, 101).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert!(page.has_more);
    let mut q = query("été");
    q.after_id = page.next_after;
    let page = p.history_search(&tx, &b, Some(&key), q, 101).unwrap();
    assert_eq!(page.rows[0].id, 3);
    assert!(!page.has_more);
    assert_eq!(
        p.history_search(&tx, &b, Some(&key), query("%_'; DROP"), 101)
            .unwrap()
            .rows
            .len(),
        1
    );
    let unicode = "é".repeat(MAX_QUERY_BYTES / 2);
    assert!(
        p.history_search(&tx, &b, Some(&key), query(&unicode), 101)
            .is_ok()
    );
    assert!(
        p.history_search(&tx, &b, Some(&key), query(&(unicode + "é")), 101)
            .is_err()
    );
    for limit in [0, MAX_RESULTS + 1] {
        let mut q = query("");
        q.limit = limit;
        assert!(p.history_search(&tx, &b, Some(&key), q, 101).is_err());
    }
    let mut q = query("");
    q.response_bytes = 1;
    assert!(p.history_search(&tx, &b, Some(&key), q, 101).is_err());
    let first = p
        .history_search(
            &tx,
            &b,
            Some(&key),
            Query {
                limit: 1,
                ..query("")
            },
            101,
        )
        .unwrap();
    let budget = serde_json::to_vec(&first).unwrap().len() + 1;
    let mut q = query("");
    q.response_bytes = budget;
    let page = p.history_search(&tx, &b, Some(&key), q, 101).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert!(page.has_more);
    assert!(serde_json::to_vec(&page).unwrap().len() <= budget);
}

#[test]
fn maximum_escaped_body_uses_bounded_multichunk_codec() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    let body = Zeroizing::new("\0".repeat(config::MAX_MESSAGE_BYTES));
    insert(&p, &key, &b, &body, 101);
    let tx = p.transaction().unwrap();
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 101)
        .unwrap();
    assert_eq!(page.rows[0].body, *body);
    assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert!(
        p.history_record(
            &tx,
            &b,
            Some(&key),
            capture(&"x".repeat(config::MAX_MESSAGE_BYTES + 1)),
            101
        )
        .is_err()
    );
}

#[test]
fn policy_counters_versions_and_ciphertext_corruption_fail_closed() {
    for (field, value) in [
        ("rows", serde_json::json!(999)),
        ("bytes", serde_json::json!(1)),
        ("next_id", serde_json::json!(0)),
        ("version", serde_json::json!(2)),
        ("next_id", serde_json::json!(u64::MAX)),
    ] {
        let (p, key, b) = fixture();
        enable(&p, &key, &b, Policy::default());
        modify_state(&p, |state| state[field] = value);
        let tx = p.transaction().unwrap();
        assert!(p.history_status(&tx, Some(&b), Some(&key), 100).is_err());
    }
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    modify_state(&p, |state| {
        state["policy"]["rows"] = serde_json::json!(MAX_ROWS + 1)
    });
    let tx = p.transaction().unwrap();
    assert!(p.history_status(&tx, None, None, 100).is_err());
    drop(tx);
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    let tx = p.transaction().unwrap();
    for policy in [
        Policy {
            rows: 0,
            ..Policy::default()
        },
        Policy {
            encrypted_bytes: MAX_ENCRYPTED_BYTES + 1,
            ..Policy::default()
        },
        Policy {
            retention_seconds: MAX_RETENTION_SECONDS + 1,
            ..Policy::default()
        },
    ] {
        assert!(p.history_enable(&tx, &b, &key, policy, 100).is_err());
    }
    p.session
        .0
        .connection
        .execute("UPDATE kk_history_state SET key_check=x'00'", [])
        .unwrap();
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 100)
            .is_err()
    );
}

#[test]
fn hard_row_limit_and_policy_shrink_prune_in_transaction() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    let tx = p.transaction().unwrap();
    // Populate a synthetic full store directly so this boundary regression does
    // not perform a quadratic number of AGE decryptions during fixture setup.
    let mut bytes = 0usize;
    for id in 1..=MAX_ROWS as u64 {
        let row = Row {
            version: 1,
            binding: b.clone(),
            id,
            event: Event::OutgoingPrepared,
            epoch: 7,
            wire_sha256: [9; 32],
            captured_at: 101,
            body: "bounded".to_owned(),
            relation: None,
        };
        let ciphertext = crate::envelope::record::seal(
            Zeroizing::new(serde_json::to_vec(&row).unwrap()),
            &key.to_public(),
        )
        .unwrap();
        bytes += ciphertext.len();
        p.session
            .0
            .connection
            .execute(
                "INSERT INTO kk_history_rows VALUES(?1,?2)",
                rusqlite::params![id as i64, ciphertext],
            )
            .unwrap();
    }
    modify_state(&p, |state| {
        state["rows"] = serde_json::json!(MAX_ROWS);
        state["bytes"] = serde_json::json!(bytes);
        state["next_id"] = serde_json::json!(MAX_ROWS + 1);
        state["clock"] = serde_json::json!(101);
    });
    p.history_record(&tx, &b, Some(&key), capture("bounded"), 101)
        .unwrap();
    assert!(
        matches!(p.history_status(&tx, Some(&b), Some(&key), 101).unwrap(),
        Status::Configured { rows: MAX_ROWS, next_id, .. } if next_id == MAX_ROWS as u64 + 2)
    );
    assert_eq!(
        p.history_search(&tx, &b, Some(&key), query(""), 101)
            .unwrap()
            .rows[0]
            .id,
        2
    );
    p.history_enable(
        &tx,
        &b,
        &key,
        Policy {
            rows: 1,
            ..Policy::default()
        },
        101,
    )
    .unwrap();
    assert_eq!(
        p.history_search(&tx, &b, Some(&key), query(""), 101)
            .unwrap()
            .rows[0]
            .id,
        MAX_ROWS as u64 + 1
    );
    tx.commit().unwrap();
}

#[test]
fn authenticated_wrong_row_key_unknown_version_and_expiry_are_not_empty_fallbacks() {
    for wrong_version in [false, true] {
        let (p, key, b) = fixture();
        enable(&p, &key, &b, Policy::default());
        insert(&p, &key, &b, "test", 101);
        let ciphertext: Vec<u8> = p
            .session
            .0
            .connection
            .query_row("SELECT ciphertext FROM kk_history_rows", [], |r| r.get(0))
            .unwrap();
        let bytes = crate::envelope::record::open(&ciphertext, &key).unwrap();
        let mut row: Row = serde_json::from_slice(&bytes).unwrap();
        let other = Identity::generate();
        if wrong_version {
            row.version = 9;
        }
        let recipient = if wrong_version {
            key.to_public()
        } else {
            other.to_public()
        };
        let ciphertext = crate::envelope::record::seal(
            Zeroizing::new(serde_json::to_vec(&row).unwrap()),
            &recipient,
        )
        .unwrap();
        let tx = p.transaction().unwrap();
        p.session
            .0
            .connection
            .execute("UPDATE kk_history_rows SET ciphertext=?1", [ciphertext])
            .unwrap();
        assert!(
            p.history_search(
                &tx,
                &b,
                Some(&key),
                query("no match"),
                101 + MAX_RETENTION_SECONDS
            )
            .is_err()
        );
    }
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    insert(&p, &key, &b, "expired", 101);
    let tx = p.transaction().unwrap();
    assert!(
        p.history_search(&tx, &b, Some(&key), query(""), 101 + MAX_RETENTION_SECONDS)
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(matches!(
        p.history_status(&tx, None, Some(&key), 101 + MAX_RETENTION_SECONDS)
            .unwrap(),
        Status::Configured { rows: 1, .. }
    ));
}

#[test]
fn monotonic_sequence_overflow_is_not_reused() {
    let (p, key, b) = fixture();
    enable(&p, &key, &b, Policy::default());
    modify_state(&p, |state| state["next_id"] = serde_json::json!(i64::MAX));
    let tx = p.transaction().unwrap();
    assert!(
        p.history_record(&tx, &b, Some(&key), capture("overflow"), 100)
            .is_err()
    );
    assert!(
        matches!(p.history_status(&tx, Some(&b), Some(&key), 100).unwrap(), Status::Configured { next_id, rows: 0, .. } if next_id == i64::MAX as u64)
    );
}

#[test]
fn migration_and_history_rollback_preserve_populated_mls_storage() {
    let (p, key, b) = fixture();
    let signer = SignatureKeyPair::new(SignatureScheme::ED25519).unwrap();
    let public = signer.to_public_vec();
    let tx = p.transaction().unwrap();
    signer.store(p.storage()).unwrap();
    p.set_metadata("group", &42u64).unwrap();
    tx.commit().unwrap();
    let tx = p.transaction().unwrap();
    p.history_enable(&tx, &b, &key, Policy::default(), 100)
        .unwrap();
    p.history_record(&tx, &b, Some(&key), capture("rolled back"), 100)
        .unwrap();
    p.set_metadata("group", &99u64).unwrap();
    drop(tx);
    let tx = p.transaction().unwrap();
    assert_eq!(
        p.history_status(&tx, Some(&b), None, 100).unwrap(),
        Status::Off
    );
    assert_eq!(p.metadata::<u64>("group").unwrap(), Some(42));
    p.history_enable(&tx, &b, &key, Policy::default(), 100)
        .unwrap();
    tx.commit().unwrap();
    let restored = SignatureKeyPair::read(p.storage(), &public, SignatureScheme::ED25519).unwrap();
    assert!(restored.sign(b"synthetic migration").is_ok());
    let tx = p.transaction().unwrap();
    p.history_record(&tx, &b, Some(&key), capture("also rollback"), 101)
        .unwrap();
    drop(tx);
    assert_eq!(insert(&p, &key, &b, "committed", 101), 1);
}

#[test]
fn migration_error_rolls_back_ddl_without_aborting_caller_transaction() {
    let (p, key, b) = fixture();
    // Synthetic conflicting table: CREATE state succeeds before rows fails.
    p.session
        .0
        .connection
        .execute_batch("CREATE TABLE kk_history_rows(existing INTEGER)")
        .unwrap();
    let tx = p.transaction().unwrap();
    assert!(
        p.history_enable(&tx, &b, &key, Policy::default(), 100)
            .is_err()
    );
    let state_tables: i64 = p
        .session
        .0
        .connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='kk_history_state'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state_tables, 0);
    assert_eq!(
        p.history_status(&tx, Some(&b), None, 100).unwrap(),
        Status::Off
    );
    p.set_metadata("still_writable", &true).unwrap();
    tx.commit().unwrap();
}

#[test]
fn transactions_are_required_and_cannot_come_from_another_provider() {
    let (p, key, b) = fixture();
    let other = Provider::memory().unwrap();
    let foreign = other.transaction().unwrap();
    assert!(
        p.history_enable(&foreign, &b, &key, Policy::default(), 100)
            .is_err()
    );
    let tx = p.transaction().unwrap();
    assert!(
        p.history_enable(&foreign, &b, &key, Policy::default(), 100)
            .is_err()
    );
    p.history_enable(&tx, &b, &key, Policy::default(), 100)
        .unwrap();
}

#[cfg(unix)]
#[test]
fn reopen_v1_v2_unknown_schema_and_failed_commit_quarantine() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic-room");
    let key = Identity::generate();
    let b = Binding::new([1; 32], [2; 32], &key.to_public());
    drop(Provider::create(&path).unwrap());
    let p = Provider::open(&path).unwrap();
    let tx = p.transaction().unwrap();
    assert_eq!(
        p.history_status(&tx, Some(&b), None, 100).unwrap(),
        Status::Off
    );
    tx.commit().unwrap();
    enable(&p, &key, &b, Policy::default());
    insert(&p, &key, &b, "durable", 101);
    drop(p);
    let p = Provider::open(&path).unwrap();
    let tx = p.transaction().unwrap();
    assert_eq!(
        p.history_search(&tx, &b, Some(&key), query(""), 101)
            .unwrap()
            .rows[0]
            .body,
        "durable"
    );
    tx.commit().unwrap();
    p.session.0.connection.execute_batch("CREATE TEMP TABLE fault_parent(id INTEGER PRIMARY KEY);
        CREATE TEMP TABLE fault_child(parent INTEGER REFERENCES fault_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
    let tx = p.transaction().unwrap();
    p.history_record(&tx, &b, Some(&key), capture("failed commit"), 102)
        .unwrap();
    p.set_metadata("group", &99u64).unwrap();
    p.session
        .0
        .connection
        .execute("INSERT INTO fault_child VALUES(99)", [])
        .unwrap();
    assert!(tx.commit().is_err());
    assert!(p.transaction().is_err());
    drop(p);
    let p = Provider::open(&path).unwrap();
    assert!(p.metadata::<u64>("group").unwrap().is_none());
    let tx = p.transaction().unwrap();
    let page = p
        .history_search(&tx, &b, Some(&key), query(""), 102)
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    tx.commit().unwrap();
    p.session
        .0
        .connection
        .pragma_update(None, "user_version", 999)
        .unwrap();
    drop(p);
    assert!(Provider::open(&path).is_err());
}
