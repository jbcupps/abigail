//! Sub-agent job queue system for Abigail.
//!
//! Provides a persistent job queue backed by SQLite (source of truth) with
//! real-time event notification via the `StreamBroker` trait. Enables the
//! Entity to spin up autonomous sub-agents, each with capability-matched
//! LLM models, and collect results asynchronously through topics.
//!
//! ## Architecture
//!
//! - **SQLite** stores job records with full lifecycle (queued → running → completed/failed)
//! - **StreamBroker** publishes `JobEvent`s for real-time consumers (watchers, future Superego)
//! - **Topics** group related jobs for batch result retrieval and auto-synthesis

pub mod queue;
pub mod schema;
pub mod types;

pub use queue::JobQueue;
pub use schema::MIGRATION_V3_JOB_QUEUE;
pub use types::{
    JobEvent, JobId, JobPriority, JobRecord, JobSpec, JobStatus, RequiredCapability, TopicId,
};
