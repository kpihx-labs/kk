use super::*;
use openraft::storage::RaftLogStorageExt;
use openraft::{CommittedLeaderId, EntryPayload};
use std::time::Duration;

fn blank(index: u64) -> Entry<TypeConfig> {
    Entry {
        log_id: LogId::new(CommittedLeaderId::new(2, PeerId::from(1)), index),
        payload: EntryPayload::Blank,
    }
}

#[test]
fn final_public_owner_releases_before_notification_despite_an_inherited_descriptor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("public-control");
    let store = LogStore::create(&path).unwrap();
    let inherited = store
        .inner
        .lock()
        .unwrap()
        ._lease
        .file()
        .try_clone()
        .unwrap();
    let retained_owner = store.clone();
    let released = store.released();
    drop(store);
    assert!(!*released.borrow());
    assert!(LogStore::open(&path).is_err());
    drop(retained_owner);
    assert!(*released.borrow());
    let reopened = LogStore::open(&path).unwrap();
    drop(inherited);
    assert!(LogStore::open(&path).is_err());
    drop(reopened);
    assert!(LogStore::open(&path).is_ok());
}

#[tokio::test]
async fn public_vote_log_commit_compaction_and_ranges_reopen_durably() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("public-control");
    let mut store = LogStore::create(&path).unwrap();
    let vote = Vote::new_committed(2, PeerId::from(1));
    store.save_vote(&vote).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        store.blocking_append((0..5).map(blank)),
    )
    .await
    .unwrap()
    .unwrap();
    store.save_committed(Some(blank(3).log_id)).await.unwrap();
    assert_eq!(store.try_get_log_entries(1..=3).await.unwrap().len(), 3);
    assert_eq!(
        store.try_get_log_entries(..=u64::MAX).await.unwrap().len(),
        5
    );
    assert!(
        store
            .try_get_log_entries((Bound::Excluded(u64::MAX), Bound::Unbounded))
            .await
            .unwrap()
            .is_empty()
    );
    store.purge(blank(2).log_id).await.unwrap();
    store.truncate(blank(4).log_id).await.unwrap();
    let state = store.get_log_state().await.unwrap();
    assert_eq!(state.last_purged_log_id, Some(blank(2).log_id));
    assert_eq!(state.last_log_id, Some(blank(3).log_id));
    assert!(LogStore::open(&path).is_err());
    drop(store);
    let mut store = LogStore::open(&path).unwrap();
    assert_eq!(store.read_vote().await.unwrap(), Some(vote));
    assert_eq!(store.read_committed().await.unwrap(), Some(blank(3).log_id));
    let entries = store.try_get_log_entries(..).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].log_id.index, 3);
}

#[tokio::test]
async fn broken_append_rolls_back_and_real_commit_failure_never_flushes_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("public-control");
    let mut store = LogStore::create(&path).unwrap();
    store.blocking_append([blank(0)]).await.unwrap();
    store.io(ErrorVerb::Write, |database| {
        database.connection.execute_batch("CREATE TEMP TRIGGER fault_statement BEFORE INSERT ON main.kk_control_logs
            WHEN NEW.idx=x'0000000000000003' BEGIN SELECT RAISE(ABORT,'intentional fixture failure'); END;")
            .map_err(|_| failure(ErrorVerb::Write, "cannot arrange statement failure"))?;
        Ok(())
    }).await.unwrap();
    assert!(
        store
            .blocking_append([blank(1), blank(2), blank(3)])
            .await
            .is_err()
    );
    assert_eq!(store.try_get_log_entries(..).await.unwrap().len(), 1);
    store.io(ErrorVerb::Write, |database| {
        database.connection.execute_batch("DROP TRIGGER fault_statement; CREATE TEMP TABLE fault_parent(id INTEGER PRIMARY KEY);
            CREATE TEMP TABLE fault_child(parent INTEGER REFERENCES fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TEMP TRIGGER fault_log AFTER INSERT ON main.kk_control_logs BEGIN INSERT INTO fault_child(parent) VALUES(99); END;")
            .map_err(|_| failure(ErrorVerb::Write, "cannot arrange SQLite failure test"))?;
        Ok(())
    }).await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), store.blocking_append([blank(1)]))
        .await
        .unwrap();
    assert!(outcome.is_err());
    assert!(store.get_log_state().await.is_err());
    drop(store);
    let mut reopened = LogStore::open(&path).unwrap();
    assert_eq!(reopened.try_get_log_entries(..).await.unwrap().len(), 1);
}
