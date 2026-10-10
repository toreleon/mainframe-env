//! Serial physical-file controls. Structural roots earn no compiled MQ credit.
use super::*;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;

struct FileFixture {
    root: PathBuf,
    path: PathBuf,
    store: Arc<SqliteStateStore>,
}
impl FileFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "mq-owned-file-custody-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("state.sqlite");
        // Exact original physical storage fixture quotas, not enlarged.
        let store = Arc::new(SqliteStateStore::create_owned_file(&path, 1 << 20, 1024).unwrap());
        Self { root, path, store }
    }
}
impl Drop for FileFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn owned_sqlite_physical_reopen_preserves_full_unknown_graph_and_admits_new_root() {
    let f = FileFixture::new();
    let (original, request) = fixture(&*f.store);
    let before = whole_observation(&*f.store, &original);
    let identity = std::fs::metadata(&f.path).unwrap();
    let fresh = f.store.reopen_owned_root(&request.cutoff).unwrap();
    assert!(!Arc::ptr_eq(&fresh, &f.store));
    let retained = std::fs::metadata(&f.path).unwrap();
    assert_eq!(
        (identity.dev(), identity.ino()),
        (retained.dev(), retained.ino())
    );
    assert_eq!(whole_observation(&*fresh, &original), before);
    assert!(
        f.store
            .get_execution(&original.occurrence.execution.execution_id)
            .is_err()
    );
    assert!(
        f.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "native-root-a".into(),
                    key: "unknown-unit".into(),
                    version: 2,
                    payload: b"old pool writer".to_vec()
                },
                Some(1)
            )
            .is_err()
    );
    assert_eq!(whole_observation(&*fresh, &original), before);
    let claim = fresh.admit_root_successor(request.clone()).unwrap();
    assert_eq!(claim.admission(), &request.admission);
    assert_eq!(
        fresh
            .get_execution(&original.occurrence.execution.execution_id)
            .unwrap(),
        before.0
    );
    assert_eq!(
        fresh.effect(&original.occurrence.effect_key).unwrap(),
        before.1
    );
    assert_eq!(
        fresh
            .get_provider_state(ROOT_DRIVER_NAMESPACE, request.cutoff.fenced.key.as_str())
            .unwrap(),
        Some(request.cutoff.fenced.clone())
    );
    assert_eq!(
        fresh
            .get_provider_state("native-root-a", "unknown-unit")
            .unwrap()
            .unwrap()
            .version,
        1
    );
    let sealed = whole_observation(&*fresh, &original);
    assert!(f.store.reopen_owned_root(&request.cutoff).is_err());
    assert_eq!(whole_observation(&*fresh, &original), sealed);
}
#[test]
fn owned_sqlite_forged_capture_refuses_before_pool_close_with_whole_state_unchanged() {
    for fault in 0..6 {
        let f = FileFixture::new();
        let (original, request) = fixture(&*f.store);
        let before = whole_observation(&*f.store, &original);
        let mut bad = request.cutoff;
        match fault {
            0 => bad.fenced_epoch += 1,
            1 => bad.fenced.version += 1,
            2 => bad.marker.payload.push(b' '),
            3 => bad.preserved.actors[0].execution.attempt += 1,
            4 => bad
                .preserved
                .provider_dependencies
                .pop()
                .map(|_| ())
                .unwrap(),
            5 => bad.preserved.core_records.pop().map(|_| ()).unwrap(),
            _ => unreachable!(),
        }
        assert!(f.store.reopen_owned_root(&bad).is_err(), "{fault}");
        assert_eq!(whole_observation(&*f.store, &original), before);
    }
}
#[test]
fn existing_copied_or_linked_database_cannot_enroll_owned_file_custody() {
    for alias in [false, true] {
        let f = FileFixture::new();
        let (original, request) = fixture(&*f.store);
        let before = whole_observation(&*f.store, &original);
        let target = f.root.join("substitute.sqlite");
        if alias {
            std::fs::hard_link(&f.path, &target).unwrap();
        } else {
            std::fs::copy(&f.path, &target).unwrap();
        }
        let bytes = std::fs::read(&target).unwrap();
        assert!(matches!(
            SqliteStateStore::create_owned_file(&target, 1 << 20, 1024),
            Err(StoreError::AlreadyExists)
        ));
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert_eq!(whole_observation(&*f.store, &original), before);
        if alias {
            assert!(f.store.reopen_owned_root(&request.cutoff).is_err());
            assert_eq!(whole_observation(&*f.store, &original), before);
        }
    }
}
#[test]
fn replaced_path_same_database_bytes_and_symlink_refuse_without_old_state_mutation() {
    for symlink in [false, true] {
        let f = FileFixture::new();
        let (original, request) = fixture(&*f.store);
        let before = whole_observation(&*f.store, &original);
        let held = f.root.join("retained.sqlite");
        std::fs::rename(&f.path, &held).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&held, &f.path).unwrap();
        } else {
            std::fs::copy(&held, &f.path).unwrap();
            assert_eq!(
                std::fs::read(&held).unwrap(),
                std::fs::read(&f.path).unwrap()
            );
        }
        let substitute = std::fs::read(&f.path).unwrap();
        assert!(f.store.reopen_owned_root(&request.cutoff).is_err());
        assert_eq!(whole_observation(&*f.store, &original), before);
        assert_eq!(std::fs::read(&f.path).unwrap(), substitute);
    }
}
#[test]
fn plain_url_open_never_acquires_held_file_authority() {
    let f = FileFixture::new();
    let (original, request) = fixture(&*f.store);
    let plain = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", f.path.display()),
            1 << 20,
            1024,
        )
        .unwrap(),
    );
    let before = whole_observation(&*f.store, &original);
    assert_eq!(whole_observation(&*plain, &original), before);
    assert!(matches!(
        plain.reopen_owned_root(&request.cutoff),
        Err(StoreError::InvalidTransition)
    ));
    assert_eq!(whole_observation(&*plain, &original), before);
    assert_eq!(whole_observation(&*f.store, &original), before);
}
