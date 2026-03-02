//! Background scheduler that drains queued jobs and dispatches runners.
//!
//! Also handles recurring (cron-based) job templates: evaluates cron expressions
//! and spawns one-shot instances when due.

use crate::subagent_runner::SubagentRunner;
use abigail_queue::JobQueue;
use chrono::Utc;
use std::str::FromStr;
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
        let mut last_cron_check = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap_or_else(Instant::now);

        loop {
            if last_expire_check.elapsed() >= self.expire_interval {
                if let Err(err) = self.queue.expire_stale_jobs().await {
                    tracing::error!("Failed to expire stale jobs: {}", err);
                }
                last_expire_check = Instant::now();
            }

            // Check cron-based recurring templates every 60 seconds.
            if last_cron_check.elapsed() >= Duration::from_secs(60) {
                self.check_cron_jobs().await;
                last_cron_check = Instant::now();
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

    /// Evaluate recurring job templates and spawn instances for those whose
    /// cron expression is due.
    async fn check_cron_jobs(&self) {
        let templates = match self.queue.get_recurring_templates() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Failed to load recurring templates: {}", e);
                return;
            }
        };

        if templates.is_empty() {
            return;
        }

        let now = Utc::now();

        for template in &templates {
            let cron_expr = match &template.cron_expression {
                Some(expr) => expr,
                None => continue,
            };

            let schedule = match cron::Schedule::from_str(cron_expr) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        "Invalid cron expression '{}' on template {}: {}",
                        cron_expr,
                        template.id,
                        e
                    );
                    continue;
                }
            };

            // Determine the reference time: last_scheduled_at or created_at.
            let reference = template
                .last_scheduled_at
                .as_deref()
                .or(Some(template.created_at.as_str()))
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);

            // Check if there's a scheduled time between reference and now.
            let next_after_ref = schedule.after(&reference).next();
            if let Some(next_time) = next_after_ref {
                if next_time <= now {
                    // Interpolate goal_template if present, otherwise use goal.
                    let goal = template
                        .goal_template
                        .as_ref()
                        .map(|tmpl| {
                            tmpl.replace("{date}", &now.format("%Y-%m-%d").to_string())
                                .replace("{time}", &now.format("%H:%M UTC").to_string())
                        })
                        .unwrap_or_else(|| template.goal.clone());

                    match self
                        .queue
                        .spawn_recurring_instance(template, Some(goal))
                        .await
                    {
                        Ok(id) => {
                            tracing::info!(
                                "Spawned recurring instance {} from template {}",
                                id,
                                template.id
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to spawn recurring instance for {}: {}",
                                template.id,
                                e
                            );
                        }
                    }
                }
            }
        }
    }
}
