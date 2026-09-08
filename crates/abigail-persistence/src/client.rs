use crate::remote::{
    BindingWire, PersistenceOp, PersistenceOutcome, PersistenceRequest, RemoteBackend, ScopeWire,
};
use crate::schema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use surrealdb::engine::local::{Db, Mem, SurrealKv};
use surrealdb::types::{Number as SurrealNumber, RecordIdKey, Value as SurrealValue};
use surrealdb::Surreal;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityScope {
    Hive,
    Entity(String),
}

impl EntityScope {
    pub fn database_name(&self) -> String {
        match self {
            Self::Hive => "hive".to_string(),
            Self::Entity(id) => format!("entity_{}", id.replace('-', "_")),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Hive => "hive".to_string(),
            Self::Entity(id) => format!("entity:{}", id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryBinding {
    pub key: String,
    pub value: Value,
}

impl QueryBinding {
    pub fn new(key: impl Into<String>, value: impl Serialize) -> Result<Self> {
        Ok(Self {
            key: key.into(),
            value: serde_json::to_value(value)?,
        })
    }
}

#[derive(Error, Debug)]
pub enum PersistenceError {
    #[error("Persistence query failed: {0}")]
    Query(#[from] surrealdb::Error),
    #[error("Serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Task channel closed")]
    ChannelClosed,
    #[error("Runtime task join failed: {0}")]
    Join(String),
    #[error("I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hive persistence unavailable: {0}")]
    Remote(String),
}

pub type Result<T> = std::result::Result<T, PersistenceError>;

enum Backend {
    /// This handle's own session on the shared file engine.
    ///
    /// Cloning a `Surreal<Db>` forks the session, so each handle keeps its own
    /// namespace/database selection and two scopes on one file cannot redirect
    /// each other. Only the *engine* is shared per path, because SurrealKv takes
    /// an exclusive file lock and one process may open it only once.
    Local(Arc<Surreal<Db>>),
    Remote(RemoteBackend),
}

#[derive(Clone)]
pub struct PersistenceHandle {
    inner: Arc<PersistenceInner>,
}

struct PersistenceInner {
    backend: Backend,
    path: PathBuf,
    scope: EntityScope,
}

impl PersistenceHandle {
    pub fn open_ephemeral(scope: EntityScope) -> Result<Self> {
        let runtime = persistence_runtime()?;
        let init_scope = scope.clone();
        let db = block_on_runtime(runtime, async move {
            let db = Surreal::new::<Mem>(())
                .await
                .map_err(PersistenceError::from)?;
            db.use_ns("abigail")
                .use_db(init_scope.database_name())
                .await
                .map_err(PersistenceError::from)?;
            schema::ensure_schema(&db, &init_scope).await?;
            Ok::<_, PersistenceError>(db)
        })?;

        Ok(Self {
            inner: Arc::new(PersistenceInner {
                backend: Backend::Local(Arc::new(db)),
                path: PathBuf::from(":memory:"),
                scope,
            }),
        })
    }

    /// Open a store file directly.
    ///
    /// This always opens the given path locally, so it stays correct for stores
    /// that are *not* the Hive's shared one — backup archives and import
    /// sources, which must never be redirected to the live family store. Use
    /// [`Self::open_shared`] for the Hive-owned store.
    pub fn open(path: impl AsRef<Path>, scope: EntityScope) -> Result<Self> {
        Self::open_local(path, scope)
    }

    /// Open a scope of the *shared, Hive-owned* store.
    ///
    /// When `ABIGAIL_PERSISTENCE_URL` is set this returns a handle that routes
    /// every operation to the Hive over the authenticated local control plane.
    /// Only the owning process (hive-daemon) opens the file, so the exclusive
    /// SurrealKv lock is never contended and a family Entity can run alongside
    /// the coordinator’s setup assistant.
    pub fn open_shared(path: impl AsRef<Path>, scope: EntityScope) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(remote) = RemoteBackend::from_env() {
            tracing::info!(
                "Persistence scope delegated to Hive: scope={} endpoint={}",
                scope.label(),
                remote.endpoint()
            );
            return Ok(Self {
                inner: Arc::new(PersistenceInner {
                    backend: Backend::Remote(remote),
                    // Reported verbatim so `path()` still names the logical
                    // store even though this process never opens it.
                    path,
                    scope,
                }),
            });
        }

        Self::open_local(path, scope)
    }

    /// Open a scope directly against the on-disk store, ignoring any remote
    /// delegation. Only the store's owner (hive-daemon) should call this.
    pub fn open_local(path: impl AsRef<Path>, scope: EntityScope) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&path)?;
        let path = normalize_store_path(&std::fs::canonicalize(&path)?)?;

        let runtime = persistence_runtime()?;
        let base_db = shared_file_engine(&path)?;
        let init_scope = scope.clone();
        let db = block_on_runtime(runtime, async move {
            // Clone forks the session, so this selection stays private to this
            // handle even when another scope shares the same engine.
            let db = base_db.as_ref().clone();
            db.use_ns("abigail")
                .use_db(init_scope.database_name())
                .await
                .map_err(PersistenceError::from)?;
            schema::ensure_schema(&db, &init_scope).await?;
            Ok::<_, PersistenceError>(db)
        })?;
        tracing::info!(
            "Persistence scope ready: scope={} path={}",
            scope.label(),
            path.display()
        );

        Ok(Self {
            inner: Arc::new(PersistenceInner {
                backend: Backend::Local(Arc::new(db)),
                path,
                scope,
            }),
        })
    }

    /// Separate database-authenticated session for child RPC. Even raw SurrealQL
    /// cannot access another Entity database or namespace/root administration.
    pub fn restricted(&self) -> Result<Self> {
        let Backend::Local(db) = &self.inner.backend else {
            return Err(PersistenceError::Remote("Expected owner handle".into()));
        };
        let db = db.clone();
        let scope = self.inner.scope.clone();
        let database = scope.database_name();
        let session = block_on_runtime(persistence_runtime()?, async move {
            let username = "runtime_rpc".to_string();
            let password = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            db.query(format!(
                "DEFINE USER OVERWRITE {username} ON DATABASE PASSWORD '{password}' ROLES EDITOR"
            ))
            .await?
            .check()?;
            let session = db.as_ref().clone();
            session
                .signin(surrealdb::opt::auth::Database {
                    namespace: "abigail".into(),
                    database,
                    username,
                    password,
                })
                .await?;
            Ok::<_, PersistenceError>(session)
        })?;
        Ok(Self {
            inner: Arc::new(PersistenceInner {
                backend: Backend::Local(Arc::new(session)),
                path: self.inner.path.clone(),
                scope,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn scope(&self) -> &EntityScope {
        &self.inner.scope
    }

    pub fn execute(&self, sql: &str, bindings: &[QueryBinding]) -> Result<()> {
        match &self.inner.backend {
            Backend::Remote(_) => {
                self.send_remote(PersistenceOp::Execute {
                    sql: sql.to_string(),
                    bindings: bindings.iter().map(BindingWire::from).collect(),
                })?;
                Ok(())
            }
            Backend::Local(_) => retry_on_conflict(|| {
                let sql = sql.to_string();
                let bindings = bindings.to_vec();
                self.run(move |db| async move {
                    let mut query = db.query(sql);
                    for binding in bindings {
                        query = query.bind((binding.key, binding.value));
                    }
                    query.await?.check()?;
                    Ok(())
                })
            }),
        }
    }

    pub fn query_vec<T>(&self, sql: &str, bindings: &[QueryBinding]) -> Result<Vec<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match &self.inner.backend {
            Backend::Remote(_) => {
                let outcome = self.send_remote(PersistenceOp::QueryVec {
                    sql: sql.to_string(),
                    bindings: bindings.iter().map(BindingWire::from).collect(),
                })?;
                expect_many(outcome)?
                    .into_iter()
                    .map(serde_json::from_value)
                    .collect::<std::result::Result<Vec<T>, _>>()
                    .map_err(PersistenceError::from)
            }
            Backend::Local(_) => {
                let sql = sql.to_string();
                let bindings = bindings.to_vec();
                self.run(move |db| async move {
                    let mut query = db.query(sql);
                    for binding in bindings {
                        query = query.bind((binding.key, binding.value));
                    }
                    let mut response = query.await?.check()?;
                    let values: Vec<SurrealValue> =
                        response.take(0).map_err(PersistenceError::from)?;
                    values
                        .into_iter()
                        .map(surreal_value_to_json)
                        .map(serde_json::from_value)
                        .collect::<std::result::Result<Vec<T>, _>>()
                        .map_err(PersistenceError::from)
                })
            }
        }
    }

    pub fn query_one<T>(&self, sql: &str, bindings: &[QueryBinding]) -> Result<Option<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match &self.inner.backend {
            Backend::Remote(_) => {
                let outcome = self.send_remote(PersistenceOp::QueryOne {
                    sql: sql.to_string(),
                    bindings: bindings.iter().map(BindingWire::from).collect(),
                })?;
                expect_maybe(outcome)?
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(PersistenceError::from)
            }
            Backend::Local(_) => {
                let sql = sql.to_string();
                let bindings = bindings.to_vec();
                self.run(move |db| async move {
                    let mut query = db.query(sql);
                    for binding in bindings {
                        query = query.bind((binding.key, binding.value));
                    }
                    let mut response = query.await?.check()?;
                    let value: Option<SurrealValue> =
                        response.take(0).map_err(PersistenceError::from)?;
                    value
                        .map(surreal_value_to_json)
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(PersistenceError::from)
                })
            }
        }
    }

    pub fn upsert<T>(&self, table: &str, id: &str, value: &T) -> Result<()>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let value = serde_json::to_value(value)?;
        match &self.inner.backend {
            Backend::Remote(_) => {
                self.send_remote(PersistenceOp::Upsert {
                    table: table.to_string(),
                    id: id.to_string(),
                    value,
                })?;
                Ok(())
            }
            Backend::Local(_) => retry_on_conflict(|| {
                let table = table.to_string();
                let id = id.to_string();
                let value = value.clone();
                self.run(move |db| async move {
                    let _: Option<surrealdb::types::Value> = db
                        .upsert((table.as_str(), id.as_str()))
                        .content(value)
                        .await
                        .map_err(PersistenceError::from)?;
                    Ok(())
                })
            }),
        }
    }

    pub fn create<T>(&self, table: &str, id: &str, value: &T) -> Result<()>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let value = serde_json::to_value(value)?;
        match &self.inner.backend {
            Backend::Remote(_) => {
                self.send_remote(PersistenceOp::Create {
                    table: table.to_string(),
                    id: id.to_string(),
                    value,
                })?;
                Ok(())
            }
            Backend::Local(_) => retry_on_conflict(|| {
                let table = table.to_string();
                let id = id.to_string();
                let value = value.clone();
                self.run(move |db| async move {
                    let _: Option<surrealdb::types::Value> = db
                        .create((table.as_str(), id.as_str()))
                        .content(value)
                        .await
                        .map_err(PersistenceError::from)?;
                    Ok(())
                })
            }),
        }
    }

    pub fn delete_record(&self, table: &str, id: &str) -> Result<()> {
        match &self.inner.backend {
            Backend::Remote(_) => {
                self.send_remote(PersistenceOp::Delete {
                    table: table.to_string(),
                    id: id.to_string(),
                })?;
                Ok(())
            }
            Backend::Local(_) => retry_on_conflict(|| {
                let table = table.to_string();
                let id = id.to_string();
                self.run(move |db| async move {
                    let _: Option<surrealdb::types::Value> = db
                        .delete((table.as_str(), id.as_str()))
                        .await
                        .map_err(PersistenceError::from)?;
                    Ok(())
                })
            }),
        }
    }

    pub fn select_record<T>(&self, table: &str, id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match &self.inner.backend {
            Backend::Remote(_) => {
                let outcome = self.send_remote(PersistenceOp::Select {
                    table: table.to_string(),
                    id: id.to_string(),
                })?;
                expect_maybe(outcome)?
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(PersistenceError::from)
            }
            Backend::Local(_) => {
                let table = table.to_string();
                let id = id.to_string();
                self.run(move |db| async move {
                    let value: Option<SurrealValue> = db
                        .select((table.as_str(), id.clone()))
                        .await
                        .map_err(PersistenceError::from)?;
                    value
                        .map(surreal_value_to_json)
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(PersistenceError::from)
                })
            }
        }
    }

    pub fn record_exists(&self, table: &str, id: &str) -> Result<bool> {
        Ok(self
            .select_record::<serde_json::Value>(table, id)?
            .is_some())
    }

    /// Dispatch one operation to the Hive-owned store. Blocking, to match the
    /// synchronous shape of the local path.
    fn send_remote(&self, op: PersistenceOp) -> Result<PersistenceOutcome> {
        let Backend::Remote(remote) = &self.inner.backend else {
            return Err(PersistenceError::Remote(
                "handle is not remote-backed".to_string(),
            ));
        };
        let remote = remote.clone();
        let request = PersistenceRequest {
            scope: ScopeWire::from(&self.inner.scope),
            op,
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        persistence_runtime()?.handle().spawn(async move {
            let _ = tx.send(remote.send(request).await);
        });
        rx.recv().map_err(|_| PersistenceError::ChannelClosed)?
    }

    /// Run a local operation. This handle's session is already pinned to its
    /// own database, so no per-operation selection — and no cross-scope lock —
    /// is needed, and scopes stay concurrent.
    fn run<T, F, Fut>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Surreal<Db>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let Backend::Local(db) = &self.inner.backend else {
            return Err(PersistenceError::Remote(
                "handle is not locally backed".to_string(),
            ));
        };
        // Fork per operation: USE in a query cannot poison the cached session.
        let db = Arc::new(db.as_ref().clone());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        persistence_runtime()?.handle().spawn(async move {
            let out = f(db).await;
            let _ = tx.send(out);
        });
        rx.recv().map_err(|_| PersistenceError::ChannelClosed)?
    }
}

/// Is this a transaction conflict the store told us to retry?
///
/// SurrealKv uses optimistic concurrency: two writers touching the same keys at
/// once means one loses and is expected to try again. Now that several Entities
/// and the Hive helper write through one owner process, that contention is real
/// — measured at roughly 7 failures per 200 writes when writers target the same
/// scope. Surfacing those to the family as lost writes would be wrong.
fn is_retryable_conflict(error: &PersistenceError) -> bool {
    let PersistenceError::Query(_) = error else {
        return false;
    };
    let text = error.to_string();
    text.contains("Transaction conflict") || text.contains("can be retried")
}

/// Run a write, retrying a bounded number of times on transaction conflicts
/// with a short escalating backoff.
fn retry_on_conflict<T>(mut attempt: impl FnMut() -> Result<T>) -> Result<T> {
    const MAX_ATTEMPTS: u32 = 5;
    let mut last = attempt();
    for retry in 1..MAX_ATTEMPTS {
        match last {
            Err(ref error) if is_retryable_conflict(error) => {
                std::thread::sleep(std::time::Duration::from_millis(4 << retry));
                tracing::debug!("Retrying persistence write after conflict (attempt {retry})");
                last = attempt();
            }
            other => return other,
        }
    }
    last
}

fn expect_many(outcome: PersistenceOutcome) -> Result<Vec<Value>> {
    match outcome {
        PersistenceOutcome::Many { rows } => Ok(rows),
        other => Err(PersistenceError::Remote(format!(
            "expected a row set, got {other:?}"
        ))),
    }
}

fn expect_maybe(outcome: PersistenceOutcome) -> Result<Option<Value>> {
    match outcome {
        PersistenceOutcome::Maybe { row } => Ok(row),
        other => Err(PersistenceError::Remote(format!(
            "expected an optional row, got {other:?}"
        ))),
    }
}

fn block_on_runtime<T, Fut>(runtime: &tokio::runtime::Runtime, future: Fut) -> Result<T>
where
    T: Send + 'static,
    Fut: std::future::Future<Output = Result<T>> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        let handle = runtime.handle().clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let out = handle.block_on(future);
            let _ = tx.send(out);
        });
        rx.recv().map_err(|_| PersistenceError::ChannelClosed)?
    } else {
        runtime.block_on(future)
    }
}

fn persistence_runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<std::result::Result<tokio::runtime::Runtime, String>> =
        OnceLock::new();

    match RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("abigail-persistence")
            .build()
            .map_err(|error| error.to_string())
    }) {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(PersistenceError::Join(error.clone())),
    }
}

fn normalize_store_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    #[cfg(windows)]
    {
        let raw = path.to_string_lossy();
        if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{}", stripped)));
        }
        if let Some(stripped) = raw.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(stripped));
        }
    }

    Ok(path)
}

fn shared_file_engine(path: &Path) -> Result<Arc<Surreal<Db>>> {
    let cache = file_engine_cache();
    {
        let cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = cache.get(path) {
            tracing::debug!("Persistence engine cache hit: {}", path.display());
            return Ok(db.clone());
        }
    }

    tracing::info!(
        "Persistence engine cache miss: opening embedded Surreal store at {}",
        path.display()
    );
    let runtime = persistence_runtime()?;
    let endpoint_path = path.to_path_buf();
    let opened = block_on_runtime(runtime, async move {
        // Persist the embedded store with SurrealKV and sync every committed
        // write because chat history and execution receipts are part of the
        // local trust record, not just a cache.
        Surreal::new::<SurrealKv>((
            endpoint_path,
            surrealdb::opt::Config::new()
                .capabilities(
                    surrealdb::opt::capabilities::Capabilities::default()
                        .with_scripting(false)
                        .with_all_net_targets_denied(),
                )
                .query_timeout(std::time::Duration::from_secs(20)),
        ))
        .sync("every")
        .await
        .map_err(PersistenceError::from)
    })?;
    let opened = Arc::new(opened);

    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    Ok(cache
        .entry(path.to_path_buf())
        .or_insert_with(|| opened.clone())
        .clone())
}

fn file_engine_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Surreal<Db>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Surreal<Db>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn surreal_value_to_json(value: SurrealValue) -> Value {
    match value {
        SurrealValue::None | SurrealValue::Null => Value::Null,
        SurrealValue::Bool(value) => Value::Bool(value),
        SurrealValue::Number(number) => surreal_number_to_json(number),
        SurrealValue::String(value) => Value::String(value),
        SurrealValue::Bytes(value) => Value::String(format!("{value}")),
        SurrealValue::Duration(value) => Value::String(format!("{value}")),
        SurrealValue::Datetime(value) => Value::String(format!("{value}")),
        SurrealValue::Uuid(value) => Value::String(format!("{value}")),
        SurrealValue::Geometry(value) => Value::String(format!("{value}")),
        SurrealValue::Table(value) => Value::String(value.to_string()),
        SurrealValue::RecordId(value) => record_id_to_json(value.key),
        SurrealValue::File(value) => Value::String(format!("{value:?}")),
        SurrealValue::Range(value) => Value::String(format!("{value:?}")),
        SurrealValue::Regex(value) => Value::String(format!("{value}")),
        SurrealValue::Array(values) => Value::Array(
            values
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
        SurrealValue::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, surreal_value_to_json(value)))
                .collect(),
        ),
        SurrealValue::Set(values) => Value::Array(
            values
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
    }
}

fn surreal_number_to_json(number: SurrealNumber) -> Value {
    match number {
        SurrealNumber::Int(value) => Value::Number(value.into()),
        SurrealNumber::Float(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        SurrealNumber::Decimal(value) => Value::String(value.to_string()),
    }
}

fn record_id_to_json(key: RecordIdKey) -> Value {
    match key {
        RecordIdKey::Number(value) => Value::Number(value.into()),
        RecordIdKey::String(value) => Value::String(value),
        RecordIdKey::Uuid(value) => Value::String(value.to_string()),
        RecordIdKey::Array(value) => Value::Array(
            value
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
        RecordIdKey::Object(value) => Value::Object(
            value
                .into_iter()
                .map(|(key, value)| (key, surreal_value_to_json(value)))
                .collect(),
        ),
        RecordIdKey::Range(value) => Value::String(format!("{value:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestDoc {
        id: String,
        value: String,
    }

    #[test]
    fn file_backed_handles_can_share_one_embedded_store_across_scopes() {
        let root = std::env::temp_dir().join(format!("abigail-persistence-{}", Uuid::new_v4()));
        let path = root.join("memory.db");

        let hive = PersistenceHandle::open(&path, EntityScope::Hive).unwrap();
        let entity =
            PersistenceHandle::open(&path, EntityScope::Entity("entity-a".to_string())).unwrap();

        hive.upsert(
            "hive_meta",
            "primary",
            &TestDoc {
                id: "primary".to_string(),
                value: "hive".to_string(),
            },
        )
        .unwrap();
        entity
            .upsert(
                "memory_entry",
                "entity-doc",
                &TestDoc {
                    id: "entity-doc".to_string(),
                    value: "entity".to_string(),
                },
            )
            .unwrap();

        let hive_doc = hive
            .select_record::<TestDoc>("hive_meta", "primary")
            .unwrap()
            .unwrap();
        let entity_doc = entity
            .select_record::<TestDoc>("memory_entry", "entity-doc")
            .unwrap()
            .unwrap();

        assert_eq!(hive_doc.value, "hive");
        assert_eq!(entity_doc.value, "entity");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_turn_survives_uncached_reopen() {
        let root =
            std::env::temp_dir().join(format!("abigail-persistence-reopen-{}", Uuid::new_v4()));
        let path = root.join("memory.db");

        let test_bin = std::env::current_exe().unwrap();
        for action in ["write", "read"] {
            let status = std::process::Command::new(&test_bin)
                .current_dir(if action == "read" {
                    root.clone()
                } else {
                    std::env::temp_dir()
                })
                .env("ABIGAIL_PERSISTENCE_REOPEN_CHILD", action)
                .env("ABIGAIL_PERSISTENCE_REOPEN_PATH", &path)
                .arg("--exact")
                .arg("client::tests::conversation_turn_reopen_child")
                .arg("--nocapture")
                .status()
                .unwrap();
            assert!(status.success(), "child {action} failed: {status}");
            assert!(
                path.join("manifest").is_dir(),
                "database must be stored at the requested absolute path"
            );
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_turn_reopen_child() {
        let Some(action) = std::env::var_os("ABIGAIL_PERSISTENCE_REOPEN_CHILD") else {
            return;
        };
        let path = PathBuf::from(std::env::var_os("ABIGAIL_PERSISTENCE_REOPEN_PATH").unwrap());

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        struct TurnDoc {
            id: String,
            session_id: String,
            role: String,
            content: String,
            created_at: String,
        }

        match action.to_string_lossy().as_ref() {
            "write" => {
                let handle =
                    PersistenceHandle::open(&path, EntityScope::Entity("entity-a".to_string()))
                        .unwrap();
                handle
                    .create(
                        "conversation_turn",
                        "turn-1",
                        &TurnDoc {
                            id: "turn-1".to_string(),
                            session_id: "session-1".to_string(),
                            role: "user".to_string(),
                            content: "hello durable store".to_string(),
                            created_at: "2026-01-01T00:00:00Z".to_string(),
                        },
                    )
                    .unwrap();
            }
            "read" => {
                let reopened =
                    PersistenceHandle::open(&path, EntityScope::Entity("entity-a".to_string()))
                        .unwrap();
                let turns = reopened
                    .query_vec::<TurnDoc>(
                        "SELECT * FROM conversation_turn WHERE session_id = $session_id",
                        &[QueryBinding::new("session_id", "session-1").unwrap()],
                    )
                    .unwrap();
                assert_eq!(turns.len(), 1);
                assert_eq!(turns[0].content, "hello durable store");
            }
            other => panic!("unknown child action {other}"),
        }
    }
}
