//! Background scheduler that drains queued jobs and dispatches runners.

use crate::subagent_runner::SubagentRunner;
use abigail_queue::JobQueue;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration, Instant};

/// Continuously polls `JobQueue` and executes work with bounded concurrency.
#[derive(Clone)]
pub struct JobScheduler {
    queue: Arc<JobQueue>,
    runner: Arc<SubagentRunner>,
    max_concurrency: usize,
    poll_interval: Duration,
    expire_interval: Duration,
}

impl JobScheduler {
    pub fn new(queue: Arc<JobQueue>, runner: Arc<SubagentRunner>) -> Self {
        Self {
            queue,
            runner,
            max_concurrency: 2,
            poll_interval: Duration::from_millis(500),
            expire_interval: Duration::from_secs(30),
        }
    }

    pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.max_concurrency = max_concurrency.max(1);
        self
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            self.run_loop().await;
        });
    }

    async fn run_loop(self: Arc<Self>) {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut last_expire_check = Instant::now()
            .checked_sub(self.expire_interval)
            .unwrap_or_else(Instant::now);

        loop {
            if last_expire_check.elapsed() >= self.expire_interval {
                if let Err(err) = self.queue.expire_stale_jobs().await {
                    tracing::error!("Failed to expire stale jobs: {}", err);
                }
                last_expire_check = Instant::now();
            }

            let permit = match semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    sleep(self.poll_interval).await;
                    continue;
                }
            };

            let next = match self.queue.next_queued_job() {
                Ok(job) => job,
                Err(err) => {
                    drop(permit);
                    tracing::error!("Failed to fetch next queued job: {}", err);
                    sleep(self.poll_interval).await;
                    continue;
                }
            };

            let Some(job) = next else {
                drop(permit);
                sleep(self.poll_interval).await;
                continue;
            };

            let runner = self.runner.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(err) = runner.run_job(job.clone()).await {
                    tracing::error!("Queued job {} execution errored: {}", job.id, err);
                }
            });
        }
    }
}
