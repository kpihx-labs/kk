//! Explicit pending-join repair from a complete, protected committed log prefix.
use super::*;
use crate::{Error, Result};
use std::os::unix::fs::PermissionsExt;

impl StateMachine {
    pub(crate) async fn replay_pending(
        store: LogStore,
        anchor: &Anchor,
        expected: [u8; 32],
    ) -> Result<()> {
        let root = anchor.clone();
        let (original, mut entries, committed, canonical) = store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                checked_view(&root, &state)?;
                let committed: LogId<PeerId> =
                    metadata::<Option<LogId<PeerId>>>(&database.connection, "committed")?
                        .flatten()
                        .ok_or_else(|| failure(ErrorVerb::Read, "no committed control prefix"))?;
                let mut query = database
                    .connection
                    .prepare("SELECT entry FROM kk_control_logs ORDER BY idx LIMIT ?1")
                    .map_err(|_| failure(ErrorVerb::Read, "cannot read control prefix"))?;
                let rows = query
                    .query_map([config::ROOM_APPLIED_RECORDS + 1], |row| {
                        row.get::<_, Vec<u8>>(0)
                    })
                    .map_err(|_| failure(ErrorVerb::Read, "cannot read control prefix"))?;
                let mut entries = Vec::new();
                let mut bytes = 0usize;
                for row in rows {
                    let encoded =
                        row.map_err(|_| failure(ErrorVerb::Read, "cannot read control entry"))?;
                    bytes = bytes
                        .checked_add(encoded.len())
                        .ok_or_else(|| failure(ErrorVerb::Read, "control prefix overflow"))?;
                    if bytes > config::ROOM_CONTROL_RPC_BYTES {
                        return Err(failure(
                            ErrorVerb::Read,
                            "control prefix exceeds repair bound",
                        ));
                    }
                    let entry: Entry<TypeConfig> = decode(&encoded)?;
                    if entry.log_id.index > committed.index {
                        break;
                    }
                    if entries.len() >= config::ROOM_APPLIED_RECORDS
                        || entry.log_id.index != entries.len() as u64
                    {
                        return Err(failure(
                            ErrorVerb::Read,
                            "complete bounded control prefix required",
                        ));
                    }
                    entries.push(entry);
                }
                if entries.last().map(|entry| entry.log_id) != Some(committed) {
                    return Err(failure(
                        ErrorVerb::Read,
                        "committed control prefix is missing or purged",
                    ));
                }
                // Only an independently confirmed matching hash promotes local
                // legacy outcomes to the selected canonical history. A divergent
                // node's own rejected records are not otherwise authority.
                let canonical = if state
                    .position
                    .is_some_and(|position| position.hash == expected)
                {
                    let journal: Vec<StoredRecord> =
                        metadata(&database.connection, "applied_journal")?
                            .ok_or_else(|| failure(ErrorVerb::Read, "canonical journal absent"))?;
                    records::validate(&journal, root.id(), state.position)?;
                    if journal.len() != entries.len() {
                        return Err(failure(
                            ErrorVerb::Read,
                            "complete canonical outcome journal required",
                        ));
                    }
                    Some(journal)
                } else {
                    None
                };
                Ok((state, entries, committed, canonical))
            })
            .await
            .map_err(|_| {
                Error::Storage("complete protected committed prefix required for recovery")
            })?;

        for (index, entry) in entries.iter_mut().enumerate() {
            if let EntryPayload::Normal(proposal) = &mut entry.payload
                && proposal.ordered_decision()?.is_none()
            {
                let accepted = if let Some(journal) = &canonical {
                    let record = &journal[index];
                    if record.log != entry.log_id {
                        return Err(Error::Authentication);
                    }
                    match &record.change {
                        Some(change) => {
                            if !change.accepted
                                || change.author != proposal.author()
                                || change.commit.as_slice() != proposal.commit_bytes()
                                || change.proposal != Some(proposal.id()?)
                            {
                                return Err(Error::Authentication);
                            }
                            true
                        }
                        None => false, // rejected or exact retry, never a new admission
                    }
                } else {
                    // Historical reconstruction remains only a candidate:
                    // its ENTIRE final journal must match the supplied hash
                    // before replacing anything. Ambiguity fails closed.
                    true
                };
                *proposal = proposal.clone().with_ordered_decision(accepted);
            }
        }
        let temporary = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let rebuilt_store = LogStore::create(temporary.path())?;
        let mut rebuilt = Self::create(rebuilt_store.clone(), anchor, original.origin.clone())
            .await
            .map_err(|_| Error::Storage("cannot initialize public replay"))?;
        rebuilt
            .apply(entries)
            .await
            .map_err(|_| Error::Storage("committed public replay failed"))?;
        let room = anchor.id();
        let (replacement, journal) = rebuilt_store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                let journal: Vec<StoredRecord> = metadata(&database.connection, "applied_journal")?
                    .ok_or_else(|| failure(ErrorVerb::Read, "replayed journal absent"))?;
                records::validate(&journal, room, state.position)?;
                Ok((state, journal))
            })
            .await
            .map_err(|_| Error::Storage("replayed control proof invalid"))?;
        if replacement
            .position
            .is_none_or(|position| position.hash != expected)
            || replacement.applied != Some(committed)
            || replacement.membership != original.membership
        {
            return Err(Error::Authentication);
        }
        let root = anchor.clone();
        store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    let current = read_state(connection)?;
                    let current_committed =
                        metadata::<Option<LogId<PeerId>>>(connection, "committed")?.flatten();
                    if encode(&current)? != encode(&original)?
                        || current_committed != Some(committed)
                    {
                        return Err(failure(
                            ErrorVerb::Write,
                            "control state changed during recovery",
                        ));
                    }
                    checked_view(&root, &replacement)?;
                    let old_journal = blob(connection, "applied_journal")?
                        .ok_or_else(|| failure(ErrorVerb::Read, "original journal absent"))?;
                    let suffix = original
                        .position
                        .map(|position| position.hash)
                        .unwrap_or([0; 32]);
                    let suffix = suffix
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    let backup = format!("recovery-machine-{suffix}");
                    let journal_backup = format!("recovery-journal-{suffix}");
                    match blob(connection, &backup)? {
                        None => {
                            put(connection, &backup, &original)?;
                            put_blob(connection, &journal_backup, &old_journal)?;
                        }
                        Some(prior)
                            if prior == encode(&original)?
                                && blob(connection, &journal_backup)?.as_deref()
                                    == Some(old_journal.as_slice()) => {}
                        Some(_) => {
                            return Err(failure(
                                ErrorVerb::Write,
                                "recovery backup collision; original state preserved",
                            ));
                        }
                    }
                    // A retained snapshot may encode the same divergent public
                    // state. Preserve its exact bytes, then replace both halves
                    // in this transaction so it cannot undo/export the repair.
                    match (
                        blob(connection, "snapshot_meta")?,
                        blob(connection, "snapshot_data")?,
                    ) {
                        (None, None) => {}
                        (Some(old_meta), Some(old_data)) => {
                            let meta_backup = format!("recovery-snapshot-meta-{suffix}");
                            let data_backup = format!("recovery-snapshot-data-{suffix}");
                            match (
                                blob(connection, &meta_backup)?,
                                blob(connection, &data_backup)?,
                            ) {
                                (None, None) => {
                                    put_blob(connection, &meta_backup, &old_meta)?;
                                    put_blob(connection, &data_backup, &old_data)?;
                                }
                                (Some(meta), Some(data))
                                    if meta == old_meta && data == old_data => {}
                                _ => return Err(failure(
                                    ErrorVerb::Write,
                                    "snapshot recovery backup collision; original state preserved",
                                )),
                            }
                            let data = encode(&replacement)?;
                            if data.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
                                return Err(failure(ErrorVerb::Write, "repaired snapshot exceeds aggregate limit"));
                            }
                            let meta = SnapshotMeta {
                                last_log_id: replacement.applied,
                                last_membership: replacement.membership.clone(),
                                snapshot_id: snapshot_id(&data),
                            };
                            put(connection, "snapshot_meta", &meta)?;
                            put_blob(connection, "snapshot_data", &data)?;
                        }
                        _ => return Err(failure(
                            ErrorVerb::Write,
                            "incomplete retained snapshot; original state preserved",
                        )),
                    }
                    put(connection, "machine", &replacement)?;
                    put(connection, "applied_journal", &journal)
                })
            })
            .await
            .map_err(|_| {
                Error::Storage("public recovery not confirmed; inspect before retrying")
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Identity,
        rooms::{Grant, Rights, RootBundle, control::Proposal, mls::CryptoMember},
    };
    use openraft::{CommittedLeaderId, Vote};

    #[tokio::test]
    async fn legacy_temporal_rejection_cannot_become_admission_and_fresh_guard_remains_required() {
        use tls_codec::Serialize as _;
        let root = crate::test_support::tempdir().unwrap();
        let alice = Identity::create(&root.path().join("alice")).unwrap();
        let bob = Identity::create(&root.path().join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "Committed temporal decisions")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let owner = CryptoMember::new(&alice, &anchor, vec![]).unwrap();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::READER).unwrap();
        let candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
        let package = openmls::prelude::KeyPackage::builder()
            .key_package_lifetime(openmls::prelude::Lifetime::init(1, 2))
            .build(
                crate::rooms::mls::SUITE,
                &candidate.provider,
                &candidate.signer,
                candidate.credential.clone(),
            )
            .unwrap()
            .key_package()
            .tls_serialize_detached()
            .unwrap();
        let proposal = Proposal::add(
            &alice,
            anchor.id(),
            0,
            anchor.id(),
            b"opaque already-rejected public-log fixture",
            &package,
        )
        .unwrap();
        let encoded = proposal.encode().unwrap();
        assert_eq!(
            proposal
                .clone()
                .with_ordered_decision(true)
                .encode()
                .unwrap(),
            encoded
        );
        let ordered = proposal.clone().with_ordered_decision(true);
        let persisted: Proposal =
            serde_json::from_slice(&serde_json::to_vec(&ordered).unwrap()).unwrap();
        assert_eq!(persisted.ordered_decision().unwrap(), Some(true));
        assert_eq!(
            Proposal::decode(&encoded)
                .unwrap()
                .ordered_decision()
                .unwrap(),
            None
        );
        let log = LogId::new(
            CommittedLeaderId::new(1, PeerId::from_peer(&alice.peer())),
            0,
        );
        let raw = Entry::<TypeConfig> {
            log_id: log,
            payload: EntryPayload::Normal(proposal.clone()),
        };
        let store = LogStore::create(&root.path().join("canonical-rejection")).unwrap();
        let mut machine =
            StateMachine::create(store.clone(), &anchor, owner.public_identity().unwrap())
                .await
                .unwrap();
        // A caller-supplied decision cannot make expired admission pass preflight.
        assert!(
            !machine
                .preflight(proposal.clone().with_ordered_decision(true))
                .await
                .unwrap()
                .unwrap_err()
                .accepted
        );
        // Unproven legacy replay stops without inventing either acceptance or a
        // new rejected journal at the old index.
        assert!(machine.apply([raw.clone()]).await.is_err());
        assert!(machine.applied_state().await.unwrap().0.is_none());
        assert_eq!(machine.public_view().await.unwrap().epoch(), 0);
        // Model the protected canonical outcome from the old FSM, preserving
        // its raw unmarked log separately from the known rejected journal.
        let rejected = machine
            .apply([Entry::<TypeConfig> {
                log_id: log,
                payload: EntryPayload::Normal(proposal.clone().with_ordered_decision(false)),
            }])
            .await
            .unwrap();
        assert!(!rejected[0].accepted);
        let expected = machine.committed_after(None).await.unwrap()[0]
            .position()
            .unwrap();
        let raw_bytes = serde_json::to_vec(&raw).unwrap();
        let saved = raw_bytes.clone();
        store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    connection
                        .execute(
                            "INSERT INTO kk_control_logs(idx,entry) VALUES(?1,?2)",
                            rusqlite::params![log.index.to_be_bytes().to_vec(), raw_bytes],
                        )
                        .map_err(|_| failure(ErrorVerb::Write, "legacy fixture log insert"))?;
                    put(connection, "committed", &Some(log))
                })
            })
            .await
            .unwrap();
        StateMachine::replay_pending(store.clone(), &anchor, expected.hash)
            .await
            .unwrap();
        let repaired = StateMachine::open(store.clone(), &anchor).await.unwrap();
        assert_eq!(repaired.public_view().await.unwrap().epoch(), 0);
        let journal = repaired.committed_after(None).await.unwrap();
        assert_eq!(journal[0].position().unwrap(), expected);
        assert!(journal[0].change().is_none());
        let preserved = store
            .io(ErrorVerb::Read, move |database| {
                database
                    .connection
                    .query_row(
                        "SELECT entry FROM kk_control_logs WHERE idx=?1",
                        [log.index.to_be_bytes().to_vec()],
                        |row| row.get::<_, Vec<u8>>(0),
                    )
                    .map_err(|_| failure(ErrorVerb::Read, "legacy fixture log absent"))
            })
            .await
            .unwrap();
        assert_eq!(preserved, saved);
        // Conversely an ordered acceptance survives expiry, but still verifies
        // the package and proposal signatures rather than trusting the flag.
        let mut accepted = StateMachine::create(
            LogStore::create(&root.path().join("accepted")).unwrap(),
            &anchor,
            owner.public_identity().unwrap(),
        )
        .await
        .unwrap();
        assert!(
            accepted
                .apply([Entry::<TypeConfig> {
                    log_id: log,
                    payload: EntryPayload::Normal(proposal.clone().with_ordered_decision(true)),
                }])
                .await
                .unwrap()[0]
                .accepted
        );
        let mut bytes = encoded;
        *bytes.last_mut().unwrap() ^= 1;
        let tampered = Proposal::decode(&bytes)
            .unwrap()
            .with_ordered_decision(true);
        let mut invalid = StateMachine::create(
            LogStore::create(&root.path().join("tampered")).unwrap(),
            &anchor,
            owner.public_identity().unwrap(),
        )
        .await
        .unwrap();
        assert!(
            !invalid
                .apply([Entry::<TypeConfig> {
                    log_id: log,
                    payload: EntryPayload::Normal(tampered),
                }])
                .await
                .unwrap()[0]
                .accepted
        );
        assert_eq!(invalid.public_view().await.unwrap().epoch(), 0);
    }

    #[tokio::test]
    async fn public_recovery_requires_expected_proof_and_preserves_logs_vote_and_prior_state() {
        recovery_preserves_public_state(false).await;
    }

    #[tokio::test]
    async fn public_recovery_replaces_divergent_snapshot_and_preserves_its_exact_backup() {
        recovery_preserves_public_state(true).await;
    }

    async fn recovery_preserves_public_state(with_snapshot: bool) {
        let root = crate::test_support::tempdir().unwrap();
        let alice = Identity::create(&root.path().join("alice")).unwrap();
        let bob = Identity::create(&root.path().join("bob")).unwrap();
        let anchor = RootBundle::create(&alice, "public recovery fixture")
            .unwrap()
            .verify(&alice.peer())
            .unwrap();
        let owner = CryptoMember::new(&alice, &anchor, vec![]).unwrap();
        let grant = Grant::issue(&anchor, &[], &alice, &bob.peer(), Rights::READER).unwrap();
        let mut candidate = CryptoMember::new(&bob, &anchor, vec![grant]).unwrap();
        let package = candidate.key_package().unwrap();
        let proposal = Proposal::add_with_welcome(
            &alice,
            anchor.id(),
            0,
            anchor.id(),
            b"synthetic commit",
            &package,
            b"synthetic encrypted Welcome",
        )
        .unwrap();
        let log = LogId::new(
            CommittedLeaderId::new(1, PeerId::from_peer(&alice.peer())),
            0,
        );
        let entry = Entry::<TypeConfig> {
            log_id: log,
            payload: EntryPayload::Normal(proposal),
        };
        let correct_store = LogStore::create(&root.path().join("correct")).unwrap();
        let mut correct =
            StateMachine::create(correct_store, &anchor, owner.public_identity().unwrap())
                .await
                .unwrap();
        let mut admitted = entry.clone();
        if let EntryPayload::Normal(proposal) = &mut admitted.payload {
            *proposal = proposal.clone().with_ordered_decision(true);
        }
        correct.apply([admitted]).await.unwrap();
        let expected = correct
            .store
            .io(ErrorVerb::Read, |database| {
                Ok(read_state(&database.connection)?.position.unwrap().hash)
            })
            .await
            .unwrap();

        let store = LogStore::create(&root.path().join("damaged")).unwrap();
        let mut damaged =
            StateMachine::create(store.clone(), &anchor, owner.public_identity().unwrap())
                .await
                .unwrap();
        // The old expiry rejection advanced its public cursor without the grant.
        damaged
            .apply([Entry {
                log_id: log,
                payload: EntryPayload::Blank,
            }])
            .await
            .unwrap();
        let old_snapshot = if with_snapshot {
            let snapshot = damaged.build_snapshot().await.unwrap();
            Some((snapshot.meta, (*snapshot.snapshot).into_bytes()))
        } else {
            None
        };
        let encoded = encode(&entry).unwrap();
        let saved_entry = encoded.clone();
        let vote = Vote::new(1, PeerId::from_peer(&alice.peer()));
        let saved_vote = encode(&vote).unwrap();
        store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    connection
                        .execute(
                            "INSERT INTO kk_control_logs(idx,entry) VALUES (?1,?2)",
                            rusqlite::params![log.index.to_be_bytes().as_slice(), encoded],
                        )
                        .map_err(|_| failure(ErrorVerb::Write, "fixture log insertion failed"))?;
                    put(connection, "committed", &Some(log))?;
                    put(connection, "vote", &vote)
                })
            })
            .await
            .unwrap();
        assert!(matches!(
            StateMachine::replay_pending(store.clone(), &anchor, [0; 32]).await,
            Err(Error::Authentication)
        ));
        assert_eq!(damaged.public_view().await.unwrap().epoch(), 0);
        if let Some((meta, data)) = &old_snapshot {
            let unchanged = damaged.get_current_snapshot().await.unwrap().unwrap();
            assert_eq!(unchanged.meta, *meta);
            assert_eq!((*unchanged.snapshot).into_bytes(), *data);
        }
        StateMachine::replay_pending(store.clone(), &anchor, expected)
            .await
            .unwrap();
        assert_eq!(damaged.public_view().await.unwrap().epoch(), 1);
        assert!(
            damaged
                .committed_welcome(PeerId::from_peer(&bob.peer()))
                .await
                .unwrap()
                .is_some()
        );
        let (entry_after, vote_after, backups) = store
            .io(ErrorVerb::Read, |database| {
                let entry = database
                    .connection
                    .query_row("SELECT entry FROM kk_control_logs", [], |row| {
                        row.get::<_, Vec<u8>>(0)
                    })
                    .map_err(|_| failure(ErrorVerb::Read, "fixture log absent"))?;
                let vote = blob(&database.connection, "vote")?.unwrap();
                let count = database
                    .connection
                    .query_row(
                        "SELECT count(*) FROM kk_control_meta WHERE name LIKE 'recovery-%'",
                        [],
                        |row| row.get::<_, usize>(0),
                    )
                    .map_err(|_| failure(ErrorVerb::Read, "fixture backups absent"))?;
                Ok((entry, vote, count))
            })
            .await
            .unwrap();
        assert_eq!(entry_after, saved_entry);
        assert_eq!(vote_after, saved_vote);
        assert_eq!(backups, if with_snapshot { 4 } else { 2 });
        if let Some((old_meta, old_data)) = old_snapshot {
            let repaired = damaged.get_current_snapshot().await.unwrap().unwrap();
            assert_eq!(repaired.meta.last_log_id, Some(log));
            assert_ne!(repaired.meta.snapshot_id, old_meta.snapshot_id);
            let old_state = bounded_state(&old_data).unwrap();
            let suffix = old_state
                .position
                .unwrap()
                .hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let (saved_meta, saved_data) = store
                .io(ErrorVerb::Read, move |database| {
                    let saved_meta = blob(
                        &database.connection,
                        &format!("recovery-snapshot-meta-{suffix}"),
                    )?
                    .ok_or_else(|| failure(ErrorVerb::Read, "snapshot metadata backup absent"))?;
                    let saved_data = blob(
                        &database.connection,
                        &format!("recovery-snapshot-data-{suffix}"),
                    )?
                    .ok_or_else(|| failure(ErrorVerb::Read, "snapshot data backup absent"))?;
                    Ok((saved_meta, saved_data))
                })
                .await
                .unwrap();
            assert_eq!(saved_meta, encode(&old_meta).unwrap());
            assert_eq!(saved_data, old_data);
            let mut follower = StateMachine::create(
                LogStore::create(&root.path().join("snapshot-recipient")).unwrap(),
                &anchor,
                owner.public_identity().unwrap(),
            )
            .await
            .unwrap();
            follower
                .install_snapshot(&repaired.meta, repaired.snapshot)
                .await
                .unwrap();
            assert_eq!(follower.public_view().await.unwrap().epoch(), 1);
            assert!(
                follower
                    .public_view()
                    .await
                    .unwrap()
                    .peers()
                    .iter()
                    .any(|peer| peer.fingerprint_bytes() == bob.peer().fingerprint_bytes())
            );
        } else {
            assert!(damaged.get_current_snapshot().await.unwrap().is_none());
        }
    }
}
