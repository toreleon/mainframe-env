//! Serial owned-file custody. Safe observations do not prove an atomic hostile
//! pathname cut; the privileged owner must keep namespace changes serialized.
use super::*;
use mainframe_env_store_api::RootCustodyCutoff;
use sqlx::sqlite::SqliteConnectOptions;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct OwnedFileBinding {
    path: PathBuf,
    #[cfg(unix)]
    file: File,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl OwnedFileBinding {
    #[cfg(unix)]
    fn create(path: &Path) -> Result<Self, StoreError> {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::MetadataExt;
        let parent = path.parent().ok_or(StoreError::InvalidTransition)?;
        if !path.is_absolute()
            || path.as_os_str().as_bytes().len() > 4096
            || path.to_str().is_none()
            || parent.canonicalize().map_err(io_error)? != parent
            || path.file_name().is_none()
        {
            return Err(StoreError::InvalidTransition);
        }
        // Authority is born with exclusive creation. An existing/copy/symlink
        // database cannot enroll from a matching filename or retained rows.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        let binding = Self {
            path: path.to_path_buf(),
            file,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        binding.verify_path()?;
        Ok(binding)
    }
    #[cfg(not(unix))]
    fn create(_: &Path) -> Result<Self, StoreError> {
        Err(StoreError::InvalidTransition)
    }

    #[cfg(unix)]
    fn verify_path(&self) -> Result<(), StoreError> {
        use std::os::unix::fs::MetadataExt;
        let held = self.file.metadata().map_err(io_error)?;
        let named = std::fs::symlink_metadata(&self.path).map_err(io_error)?;
        if !held.is_file()
            || !named.is_file()
            || named.file_type().is_symlink()
            || held.dev() != self.device
            || held.ino() != self.inode
            || named.dev() != self.device
            || named.ino() != self.inode
            || held.nlink() != 1
            || named.nlink() != 1
            || self.path.canonicalize().map_err(io_error)? != self.path
        {
            return Err(StoreError::InvalidTransition);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    fn verify_path(&self) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }

    fn options(&self) -> SqliteConnectOptions {
        // Filename is a typed path, never a caller-provided SQLite URL/query.
        SqliteConnectOptions::new()
            .filename(&self.path)
            .create_if_missing(false)
    }
    fn verify_driver(&self, store: &SqliteStateStore) -> Result<(), StoreError> {
        self.verify_path()?;
        if store.pool.is_closed() || store.pool.options().get_max_connections() != 1 {
            return Err(StoreError::InvalidTransition);
        }
        // Observe the actual sole SQLx connection's attached main database.
        // SQLite's main plus optional unnamed temp are the only admitted rows.
        let rows = store.run(
            sqlx::query("SELECT name,file FROM pragma_database_list ORDER BY seq LIMIT 3")
                .fetch_all(&store.pool),
        )?;
        if rows.is_empty() || rows.len() > 2 {
            return Err(StoreError::InvalidTransition);
        }
        let mut main = 0;
        for row in rows {
            let name: String = row.try_get(0).map_err(infrastructure)?;
            let path: String = row.try_get(1).map_err(infrastructure)?;
            match name.as_str() {
                "main" if Path::new(&path) == self.path => main += 1,
                "temp" if path.is_empty() => (),
                _ => return Err(StoreError::InvalidTransition),
            }
        }
        if main != 1 {
            return Err(StoreError::InvalidTransition);
        }
        self.verify_path()
    }
}

fn io_error(error: std::io::Error) -> StoreError {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => StoreError::AlreadyExists,
        std::io::ErrorKind::NotFound => StoreError::NotFound,
        _ => StoreError::Infrastructure(error.to_string()),
    }
}

impl SqliteStateStore {
    /// Create a fresh privately owned file. Existing-file URL open retains its
    /// original behavior and cannot acquire this non-serializable file custody.
    pub fn create_owned_file(
        path: &Path,
        max_payload_bytes: usize,
        max_rows: usize,
    ) -> Result<Self, StoreError> {
        if max_payload_bytes == 0 || max_rows == 0 {
            return Err(StoreError::CapacityExceeded);
        }
        let binding = Arc::new(OwnedFileBinding::create(path)?);
        Self::open_retained_file(
            binding,
            max_payload_bytes,
            max_rows,
            max_rows,
            derived_archive_bytes(max_payload_bytes, max_rows),
        )
    }
    fn open_retained_file(
        binding: Arc<OwnedFileBinding>,
        max_payload_bytes: usize,
        max_rows: usize,
        max_archive_rows: usize,
        max_archive_bytes: u64,
    ) -> Result<Self, StoreError> {
        binding.verify_path()?;
        let runtime = AdapterRuntime::new(
            Builder::new_current_thread()
                .enable_time()
                .build()
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
        );
        let pool = block_on(
            &runtime,
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(binding.options()),
        )?
        .map_err(infrastructure)?;
        // Validate actual driver custody before migration/retention writes.
        let provisional = Self {
            runtime,
            pool,
            max_payload_bytes,
            max_rows,
            max_archive_rows,
            max_archive_bytes,
            owned_file: Some(binding.clone()),
            owned_reopen: AtomicBool::new(false),
        };
        binding.verify_driver(&provisional)?;
        let Self { runtime, pool, .. } = provisional;
        let store = Self::initialize(
            runtime,
            pool,
            max_payload_bytes,
            max_rows,
            max_archive_rows,
            max_archive_bytes,
            Some(binding.clone()),
        )?;
        binding.verify_driver(&store)?;
        Ok(store)
    }
    /// Physically close this actual owned SQLx pool and reopen its held file.
    /// The complete physical fenced graph is compared before/after. A failure
    /// after closure keeps the original pool closed and yields no fresh store;
    /// it does not settle work or authorize another attempt.
    ///
    /// This is trusted serial infrastructure custody, not an application grant
    /// or an atomic proof against concurrent hostile pathname replacement.
    pub fn reopen_owned_root(
        self: &Arc<Self>,
        cutoff: &RootCustodyCutoff,
    ) -> Result<Arc<Self>, StoreError> {
        cutoff.validate_bounds()?;
        let binding = self
            .owned_file
            .as_ref()
            .ok_or(StoreError::InvalidTransition)?
            .clone();
        binding.verify_driver(self)?;
        self.root_verify_cutoff_for_owned_file(cutoff)?;
        // Pool::close is awaited to completion, so any checked-out driver must
        // actually return/close. The original pool acquire timeout bounds this
        // infrastructure drain; timeout yields no successor and keeps fencing.
        self.owned_reopen
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| StoreError::InvalidTransition)?;
        binding.verify_path()?;
        let timeout = self.pool.options().get_acquire_timeout();
        block_on(&self.runtime, async {
            tokio::time::timeout(timeout, self.pool.close()).await
        })?
        .map_err(|_| {
            StoreError::Infrastructure(
                "owned SQLite pool closure exceeded its original acquire timeout".into(),
            )
        })?;
        if !self.pool.is_closed() {
            return Err(StoreError::InvalidTransition);
        }
        binding.verify_path()?;
        let reopened = Arc::new(Self::open_retained_file(
            binding.clone(),
            self.max_payload_bytes,
            self.max_rows,
            self.max_archive_rows,
            self.max_archive_bytes,
        )?);
        reopened.root_verify_cutoff_for_owned_file(cutoff)?;
        binding.verify_driver(&reopened)?;
        Ok(reopened)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    type StoreObservation = (Vec<(String, String, i64, Vec<u8>)>, (i64, i64));
    fn observation(store: &SqliteStateStore) -> StoreObservation {
        (store.run(sqlx::query_as("SELECT namespace,key,version,payload FROM provider_state ORDER BY namespace,key").fetch_all(&store.pool)).unwrap(),
         store.run(sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1").fetch_one(&store.pool)).unwrap())
    }
    #[test]
    fn retained_file_refuses_other_actual_driver_and_extra_attachment_without_row_mutation() {
        let root = std::env::temp_dir().join(format!(
            "mq-owned-driver-control-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let first =
            SqliteStateStore::create_owned_file(&root.join("first.sqlite"), 1 << 20, 1024).unwrap();
        let second =
            SqliteStateStore::create_owned_file(&root.join("second.sqlite"), 1 << 20, 1024)
                .unwrap();
        let before = observation(&first);
        let other = observation(&second);
        assert_eq!(
            before, other,
            "same retained rows are deliberately insufficient"
        );
        let binding = first.owned_file.as_ref().unwrap();
        assert!(binding.verify_driver(&first).is_ok());
        assert_eq!(
            binding.verify_driver(&second),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(observation(&first), before);
        assert_eq!(observation(&second), other);
        first
            .run(
                sqlx::query("ATTACH DATABASE ? AS foreign_custody")
                    .bind(root.join("second.sqlite").to_str().unwrap())
                    .execute(&first.pool),
            )
            .unwrap();
        assert_eq!(
            binding.verify_driver(&first),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(observation(&first), before);
        assert_eq!(observation(&second), other);
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
