use crate::schema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use surrealdb::engine::local::{Db, Mem, SurrealKv};
use surrealdb::types::{
    Number as SurrealNumber, RecordIdKey, Value as SurrealValue,
};
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
}

pub type Result<T> = std::result::Result<T, PersistenceError>;

#[derive(Clone)]
pub struct PersistenceHandle {
    inner: Arc<PersistenceInner>,
}

struct PersistenceInner {
    db: Arc<Surreal<Db>>,
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
                db: Arc::new(db),
                path: PathBuf::from(":memory:"),
                scope,
            }),
        })
    }

    pub fn open(path: impl AsRef<Path>, scope: EntityScope) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;

        let runtime = persistence_runtime()?;
        let init_path = path.clone();
        let init_scope = scope.clone();
        let db = block_on_runtime(runtime, async move {
                let db = Surreal::new::<SurrealKv>(init_path.clone())
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
                db: Arc::new(db),
                path,
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
    }

    pub fn query_vec<T>(&self, sql: &str, bindings: &[QueryBinding]) -> Result<Vec<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let sql = sql.to_string();
        let bindings = bindings.to_vec();
        self.run(move |db| async move {
            let mut query = db.query(sql);
            for binding in bindings {
                query = query.bind((binding.key, binding.value));
            }
            let mut response = query.await?.check()?;
            let values: Vec<SurrealValue> = response.take(0).map_err(PersistenceError::from)?;
            values
                .into_iter()
                .map(|value| surreal_value_to_json(value))
                .map(serde_json::from_value)
                .collect::<std::result::Result<Vec<T>, _>>()
                .map_err(PersistenceError::from)
        })
    }

    pub fn query_one<T>(&self, sql: &str, bindings: &[QueryBinding]) -> Result<Option<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let sql = sql.to_string();
        let bindings = bindings.to_vec();
        self.run(move |db| async move {
            let mut query = db.query(sql);
            for binding in bindings {
                query = query.bind((binding.key, binding.value));
            }
            let mut response = query.await?.check()?;
            let value: Option<SurrealValue> = response.take(0).map_err(PersistenceError::from)?;
            value
                .map(surreal_value_to_json)
                .map(serde_json::from_value)
                .transpose()
                .map_err(PersistenceError::from)
        })
    }

    pub fn upsert<T>(&self, table: &str, id: &str, value: &T) -> Result<()>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let table = table.to_string();
        let id = id.to_string();
        let value = serde_json::to_value(value)?;
        self.run(move |db| async move {
            let _: Option<surrealdb::types::Value> = db
                .upsert((table.as_str(), id.as_str()))
                .content(value)
                .await
                .map_err(PersistenceError::from)?;
            Ok(())
        })
    }

    pub fn create<T>(&self, table: &str, id: &str, value: &T) -> Result<()>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let table = table.to_string();
        let id = id.to_string();
        let value = serde_json::to_value(value)?;
        self.run(move |db| async move {
            let _: Option<surrealdb::types::Value> = db
                .create((table.as_str(), id.as_str()))
                .content(value)
                .await
                .map_err(PersistenceError::from)?;
            Ok(())
        })
    }

    pub fn delete_record(&self, table: &str, id: &str) -> Result<()> {
        let table = table.to_string();
        let id = id.to_string();
        self.run(move |db| async move {
            let _: Option<surrealdb::types::Value> = db
                .delete((table.as_str(), id.as_str()))
                .await
                .map_err(PersistenceError::from)?;
            Ok(())
        })
    }

    pub fn select_record<T>(&self, table: &str, id: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned + Send + 'static,
    {
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

    pub fn record_exists(&self, table: &str, id: &str) -> Result<bool> {
        Ok(self
            .select_record::<serde_json::Value>(table, id)?
            .is_some())
    }

    fn run<T, F, Fut>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Surreal<Db>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let db = self.inner.db.clone();
        persistence_runtime()?.handle().spawn(async move {
            let out = f(db).await;
            let _ = tx.send(out);
        });
        rx.recv().map_err(|_| PersistenceError::ChannelClosed)?
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
        SurrealValue::Array(values) => {
            Value::Array(values.into_iter().map(surreal_value_to_json).collect::<Vec<_>>())
        }
        SurrealValue::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, surreal_value_to_json(value)))
                .collect(),
        ),
        SurrealValue::Set(values) => {
            Value::Array(values.into_iter().map(surreal_value_to_json).collect::<Vec<_>>())
        }
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
        RecordIdKey::Array(value) => {
            Value::Array(value.into_iter().map(surreal_value_to_json).collect::<Vec<_>>())
        }
        RecordIdKey::Object(value) => Value::Object(
            value
                .into_iter()
                .map(|(key, value)| (key, surreal_value_to_json(value)))
                .collect(),
        ),
        RecordIdKey::Range(value) => Value::String(format!("{value:?}")),
    }
}
