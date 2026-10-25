//! Local private storage only. Never serialize this database as a control snapshot.
//! Transactions are a storage primitive; durable Group mutation/reload is a separate
//! owner that must commit before exposing bytes and discard dirty memory on failure.

use crate::{Error, Result, config, profile};
use openmls_libcrux_crypto::CryptoProvider;
use openmls_sqlite_storage::{Codec, SqliteStorageProvider};
use openmls_traits::OpenMlsProvider;
use rusqlite::{Connection, OpenFlags, OptionalExtension, limits::Limit};
use serde::{Serialize, de::DeserializeOwned};
use std::{borrow::Borrow, cell::Cell, fs::File, path::Path, rc::Rc, time::Duration};

const SCHEMA_VERSION: i64 = 1;

#[derive(Default)]
pub struct JsonCodec;
impl Codec for JsonCodec {
    type Error = serde_json::Error;
    fn to_vec<T: Serialize>(value: &T) -> std::result::Result<Vec<u8>, Self::Error> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.len() > config::ROOM_STORE_VALUE_BYTES {
            return Err(<serde_json::Error as serde::ser::Error>::custom(
                "private record is too large",
            ));
        }
        Ok(bytes)
    }
    fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> std::result::Result<T, Self::Error> {
        if bytes.len() > config::ROOM_STORE_VALUE_BYTES {
            return Err(<serde_json::Error as serde::de::Error>::custom(
                "private record is too large",
            ));
        }
        serde_json::from_slice(bytes)
    }
}

// Deliberately thread-confined. A room actor owns the SQLite connection; network
// workers send typed commands rather than concurrently using private state.
struct Session {
    connection: Connection,
    poisoned: Cell<bool>,
    _lease: Option<File>,
}
#[derive(Clone)]
pub struct ConnectionHandle(Rc<Session>);
impl Borrow<Connection> for ConnectionHandle {
    fn borrow(&self) -> &Connection {
        &self.0.connection
    }
}

pub struct Provider {
    crypto: CryptoProvider,
    storage: SqliteStorageProvider<JsonCodec, ConnectionHandle>,
    session: ConnectionHandle,
}
impl OpenMlsProvider for Provider {
    type CryptoProvider = CryptoProvider;
    type RandProvider = CryptoProvider;
    type StorageProvider = SqliteStorageProvider<JsonCodec, ConnectionHandle>;
    fn crypto(&self) -> &CryptoProvider {
        &self.crypto
    }
    fn rand(&self) -> &CryptoProvider {
        &self.crypto
    }
    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }
}

impl Provider {
    pub fn memory() -> Result<Self> {
        Self::initialize(
            Connection::open_in_memory()
                .map_err(|_| Error::Storage("cannot create memory store"))?,
            None,
            true,
        )
    }
    /// Create a new private database. Existing files are never repaired/replaced.
    pub fn create(directory: &Path) -> Result<Self> {
        Self::file(directory, true)
    }
    /// Reopen only a protected, recognized existing database. Never regenerates it.
    pub fn open(directory: &Path) -> Result<Self> {
        Self::file(directory, false)
    }

    #[cfg(unix)]
    fn file(directory: &Path, create: bool) -> Result<Self> {
        use std::{
            fs,
            os::unix::fs::{MetadataExt, OpenOptionsExt},
        };
        profile::private_directory(directory, create)?;
        let path = directory.join("private.sqlite3");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(create)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
            || metadata.len() > config::ROOM_STORE_MAX_BYTES as u64
        {
            return Err(Error::Storage(
                "database must be owned, private, regular, single-link and bounded",
            ));
        }
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Storage("database is already owned by another actor"))?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let connection = Connection::open_with_flags(&path, flags)
            .map_err(|_| Error::Storage("cannot open database"))?;
        let after = fs::symlink_metadata(&path)?;
        if after.ino() != metadata.ino() || after.dev() != metadata.dev() {
            return Err(Error::Storage("database changed during open"));
        }
        let provider = Self::initialize(connection, Some(file), create)?;
        if create {
            File::open(directory)?.sync_all()?;
            let parent = directory
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            File::open(parent)?.sync_all()?;
        }
        Ok(provider)
    }
    #[cfg(not(unix))]
    fn file(_directory: &Path, _create: bool) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    fn initialize(mut connection: Connection, lease: Option<File>, create: bool) -> Result<Self> {
        // Validate an existing store before applying mutable pragmas/migrations.
        if !create {
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|_| Error::Storage("cannot read schema version"))?;
            if version != SCHEMA_VERSION {
                return Err(Error::Storage(
                    "unrecognized private state; explicit recovery required",
                ));
            }
            connection
                .prepare("SELECT name, value FROM kk_private_metadata LIMIT 0")
                .map_err(|_| Error::Storage("private metadata is missing"))?;
        }
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(|_| Error::Storage("cannot set lock deadline"))?;
        connection
            .set_limit(
                Limit::SQLITE_LIMIT_LENGTH,
                config::ROOM_STORE_VALUE_BYTES as i32,
            )
            .map_err(|_| Error::Storage("cannot bound private value size"))?;
        connection
            .execute_batch(
                "PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON;
            PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA temp_store=MEMORY;",
            )
            .map_err(|_| Error::Storage("cannot enforce storage policy"))?;
        let page_size: usize = connection
            .pragma_query_value(None, "page_size", |row| row.get(0))
            .map_err(|_| Error::Storage("cannot determine database page size"))?;
        if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(Error::Storage("invalid database page size"));
        }
        connection
            .pragma_update(
                None,
                "max_page_count",
                config::ROOM_STORE_MAX_BYTES / page_size,
            )
            .map_err(|_| Error::Storage("cannot bound database pages"))?;
        if create {
            let mut storage = SqliteStorageProvider::<JsonCodec, _>::new(&mut connection);
            storage
                .run_migrations()
                .map_err(|_| Error::Storage("cannot initialize MLS schema"))?;
            connection
                .execute_batch(
                    "CREATE TABLE kk_private_metadata (name TEXT PRIMARY KEY, value BLOB NOT NULL);
                PRAGMA user_version=1;",
                )
                .map_err(|_| Error::Storage("cannot initialize private metadata"))?;
        }
        let crypto = CryptoProvider::new().map_err(|_| Error::Authentication)?;
        let session = ConnectionHandle(Rc::new(Session {
            connection,
            poisoned: Cell::new(false),
            _lease: lease,
        }));
        let storage = SqliteStorageProvider::new(session.clone());
        Ok(Self {
            crypto,
            storage,
            session,
        })
    }
    fn healthy(&self) -> Result<()> {
        if self.session.0.poisoned.get() {
            return Err(Error::Storage(
                "uncertain transaction; reopen and verify state",
            ));
        }
        Ok(())
    }
    pub fn transaction(&self) -> Result<Transaction> {
        self.healthy()?;
        self.session
            .0
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| Error::Storage("cannot begin private transaction"))?;
        Ok(Transaction {
            session: self.session.clone(),
            finished: false,
        })
    }
    pub fn metadata<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>> {
        self.healthy()?;
        let bytes: Option<Vec<u8>> = self
            .session
            .0
            .connection
            .query_row(
                "SELECT value FROM kk_private_metadata WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| Error::Storage("cannot read private metadata"))?;
        bytes
            .map(|value| {
                JsonCodec::from_slice(&value)
                    .map_err(|_| Error::Storage("invalid private metadata"))
            })
            .transpose()
    }
    pub fn set_metadata(&self, name: &str, value: &impl Serialize) -> Result<()> {
        self.healthy()?;
        if self.session.0.connection.is_autocommit() {
            return Err(Error::Storage(
                "private metadata requires an explicit transaction",
            ));
        }
        let bytes = JsonCodec::to_vec(value)
            .map_err(|_| Error::Storage("cannot encode private metadata"))?;
        self.session
            .0
            .connection
            .execute(
                "INSERT INTO kk_private_metadata(name,value) VALUES(?1,?2)
            ON CONFLICT(name) DO UPDATE SET value=excluded.value",
                rusqlite::params![name, bytes],
            )
            .map_err(|_| Error::Storage("cannot save private metadata"))?;
        Ok(())
    }

    #[cfg(all(test, unix))]
    pub(crate) fn inject_checkpoint_commit_failure(&self) {
        // Real deferred SQLite constraint failure, after crypto rows and the
        // checkpoint have been written. No production failure switch exists.
        self.session.0.connection.execute_batch("CREATE TEMP TABLE kk_fault_parent(id INTEGER PRIMARY KEY);
            CREATE TEMP TABLE kk_fault_child(parent INTEGER REFERENCES kk_fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TEMP TRIGGER kk_fault_checkpoint AFTER UPDATE ON main.kk_private_metadata
            WHEN NEW.name='group' BEGIN INSERT INTO kk_fault_child(parent) VALUES(99); END;").unwrap();
    }
}

/// Keeps the connection and file lease alive. No nested transactions or implicit
/// commits. Commit failure quarantines the handle; Drop rolls back without success.
pub struct Transaction {
    session: ConnectionHandle,
    finished: bool,
}
impl Transaction {
    pub fn commit(mut self) -> Result<()> {
        if self.session.0.connection.execute_batch("COMMIT").is_err() {
            self.session.0.poisoned.set(true);
            return Err(Error::Storage(
                "private commit failed; state must be reopened",
            ));
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished && self.session.0.connection.execute_batch("ROLLBACK").is_err() {
            self.session.0.poisoned.set(true);
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
