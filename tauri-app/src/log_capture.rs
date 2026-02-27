use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

const MAX_LOG_ENTRIES: usize = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub index: usize,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

pub type LogBuffer = Arc<Mutex<LogRing>>;

pub fn new_log_buffer() -> LogBuffer {
    Arc::new(Mutex::new(LogRing::new()))
}

pub struct LogRing {
    entries: VecDeque<LogEntry>,
    next_index: usize,
}

impl LogRing {
    fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            next_index: 0,
        }
    }

    fn push(&mut self, level: String, target: String, message: String) {
        let entry = LogEntry {
            index: self.next_index,
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level,
            target,
            message,
        };
        self.next_index += 1;
        if self.entries.len() >= MAX_LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn since(&self, index: usize) -> (&[LogEntry], &[LogEntry], usize) {
        let (a, b) = self.entries.as_slices();
        let first_index = self.entries.front().map_or(self.next_index, |e| e.index);
        if index >= self.next_index {
            return (&[], &[], self.next_index);
        }
        let skip = index.saturating_sub(first_index);
        let total_len = a.len() + b.len();
        if skip >= total_len {
            return (&[], &[], self.next_index);
        }
        if skip < a.len() {
            (&a[skip..], b, self.next_index)
        } else {
            (&[], &b[skip - a.len()..], self.next_index)
        }
    }

    pub fn all(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn next_index(&self) -> usize {
        self.next_index
    }
}

struct RingBufferLayer {
    buffer: LogBuffer,
}

impl<S> Layer<S> for RingBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        let level = meta.level().to_string();
        let target = meta.target().to_string();

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);

        if let Ok(mut ring) = self.buffer.lock() {
            ring.push(level, target, visitor.0);
        }
    }
}

struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value);
        } else if !self.0.is_empty() {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        } else {
            self.0 = format!("{}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        } else if !self.0.is_empty() {
            self.0.push_str(&format!(" {}={}", field.name(), value));
        } else {
            self.0 = format!("{}={}", field.name(), value);
        }
    }
}

static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();
static CURRENT_FILTER: OnceLock<Mutex<String>> = OnceLock::new();

pub fn init_tracing(log_buffer: LogBuffer) {
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    let env_filter = EnvFilter::try_new(&default_filter).unwrap_or_else(|_| EnvFilter::new("info"));

    let (filter_layer, filter_handle) = reload::Layer::new(env_filter);

    FILTER_HANDLE.set(filter_handle).ok();
    CURRENT_FILTER.set(Mutex::new(default_filter.clone())).ok();

    let capture_layer = RingBufferLayer { buffer: log_buffer };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true)
        .compact();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(capture_layer)
        .with(fmt_layer)
        .init();
}

pub fn reload_filter(directive: &str) -> Result<(), String> {
    let handle = FILTER_HANDLE
        .get()
        .ok_or_else(|| "Tracing not initialized".to_string())?;

    let new_filter = EnvFilter::try_new(directive)
        .map_err(|e| format!("Invalid filter '{}': {}", directive, e))?;

    handle
        .reload(new_filter)
        .map_err(|e| format!("Failed to reload filter: {}", e))?;

    if let Some(current) = CURRENT_FILTER.get() {
        if let Ok(mut guard) = current.lock() {
            *guard = directive.to_string();
        }
    }

    Ok(())
}

pub fn current_filter() -> String {
    CURRENT_FILTER
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_else(|| "info".to_string())
}
