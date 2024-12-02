#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

mod config;

use chrono::{DateTime, Utc};
pub use config::*;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

static INSTANCE: OnceLock<Time> = OnceLock::new();

#[derive(Clone)]
struct Time {
    config: TimeConfig,
    elapsed_ms: Arc<AtomicU64>,
    ticker_task: Arc<OnceLock<()>>,
}

impl Time {
    fn new(config: TimeConfig) -> Self {
        let time = Self {
            config,
            elapsed_ms: Arc::new(AtomicU64::new(0)),
            ticker_task: Arc::new(OnceLock::new()),
        };
        if !time.config.realtime {
            time.spawn_ticker();
        }
        time
    }

    fn spawn_ticker(&self) {
        let elapsed_ms = self.elapsed_ms.clone();
        let sim_config = self
            .config
            .sim_time
            .as_ref()
            .expect("sim_time required when realtime is false");
        let tick_interval_ms = sim_config.tick_interval_ms;
        let tick_duration = sim_config.tick_duration_secs;
        self.ticker_task.get_or_init(|| {
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(tokio::time::Duration::from_millis(tick_interval_ms));
                loop {
                    interval.tick().await;
                    elapsed_ms.fetch_add(tick_duration.as_millis() as u64, Ordering::Relaxed);
                }
            });
        });
    }

    fn now(&self) -> DateTime<Utc> {
        if self.config.realtime {
            Utc::now()
        } else {
            let sim_config = self
                .config
                .sim_time
                .as_ref()
                .expect("sim_time required when realtime is false");
            let elapsed_ms = self.elapsed_ms.load(Ordering::Relaxed);
            sim_config.start_at + chrono::Duration::milliseconds(elapsed_ms as i64)
        }
    }

    async fn sleep(&self, duration: Duration) {
        if self.config.realtime {
            tokio::time::sleep(duration).await
        } else {
            let sim_config = self
                .config
                .sim_time
                .as_ref()
                .expect("sim_time required when realtime is false");

            // Calculate how many real milliseconds we need to wait based on the simulation speed
            let sim_ms_per_real_ms = sim_config.tick_duration_secs.as_millis() as f64
                / sim_config.tick_interval_ms as f64;

            let real_ms = (duration.as_millis() as f64 / sim_ms_per_real_ms).ceil() as u64;

            tokio::time::sleep(Duration::from_millis(real_ms)).await
        }
    }
}

pub fn init(config: TimeConfig) {
    INSTANCE.get_or_init(|| Time::new(config));
}

pub fn now() -> DateTime<Utc> {
    INSTANCE
        .get_or_init(|| Time::new(TimeConfig::default()))
        .now()
}

pub async fn sleep(duration: Duration) {
    INSTANCE
        .get_or_init(|| Time::new(TimeConfig::default()))
        .sleep(duration)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use std::time::Duration as StdDuration;

    #[tokio::test]
    async fn test_simulated_time() {
        // Configure time where 10ms = 10 days of simulated time
        let config = TimeConfig {
            realtime: false,
            sim_time: Some(SimTimeConfig {
                start_at: Utc::now(),
                tick_interval_ms: 10,
                tick_duration_secs: StdDuration::from_secs(10 * 24 * 60 * 60), // 10 days in seconds
            }),
        };

        init(config);
        let start = now();
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let end = now();
        let elapsed = end - start;

        assert!(
            elapsed >= ChronoDuration::days(19) && elapsed <= ChronoDuration::days(21),
            "Expected ~20 days to pass, but got {} days",
            elapsed.num_days()
        );
    }

    #[test]
    fn test_default_realtime() {
        let t1 = now();
        let t2 = Utc::now();
        assert!(t2 - t1 < ChronoDuration::seconds(1));
    }
}
