use super::*;

fn record() -> Record {
    Record {
        id: Id::default(),
        data: HashMap::new(),
        expiry_date: OffsetDateTime::now_utc() + Duration::hours(1),
    }
}

#[tokio::test]
async fn removed_session_cannot_be_revived_by_an_inflight_save() {
    let store = BoundedStore::default();
    let mut record = record();
    store.create(&mut record).await.unwrap();
    store.delete(&record.id).await.unwrap();
    assert!(store.save(&record).await.is_err());
    assert!(store.load(&record.id).await.unwrap().is_none());
}

#[tokio::test]
async fn capacity_is_bounded_and_expired_entries_are_reclaimed() {
    let store = BoundedStore::default();
    for _ in 0..1024 {
        store.create(&mut record()).await.unwrap();
    }
    assert!(store.create(&mut record()).await.is_err());
    for record in store.0.lock().await.values_mut() {
        record.expiry_date = OffsetDateTime::now_utc() - Duration::seconds(1);
    }
    store.create(&mut record()).await.unwrap();
    assert_eq!(store.0.lock().await.len(), 1);
}

#[tokio::test]
async fn collisions_rotate_ids_and_expired_loads_fail_closed() {
    let store = BoundedStore::default();
    let mut first = record();
    store.create(&mut first).await.unwrap();
    let mut second = first.clone();
    store.create(&mut second).await.unwrap();
    assert_ne!(first.id, second.id);
    first.expiry_date = OffsetDateTime::now_utc() - Duration::seconds(1);
    store.save(&first).await.unwrap();
    assert!(store.load(&first.id).await.unwrap().is_none());
    assert!(store.load(&second.id).await.unwrap().is_some());
}
