use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;

#[derive(Debug, Clone)]
pub struct UsageContext {
    pub run_id: String,
}

impl UsageContext {
    pub fn new(run_id: String) -> Self {
        Self { run_id }
    }

    pub fn new_random() -> Self {
        Self {
            run_id: new_id("run"),
        }
    }

    pub async fn scope<F, Fut, T>(self, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        USAGE_CONTEXT.scope(self, f()).await
    }
}

tokio::task_local! {
    static USAGE_CONTEXT: UsageContext;
}

pub fn current_context() -> Option<UsageContext> {
    USAGE_CONTEXT.try_with(|ctx| ctx.clone()).ok()
}

fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let ts = Utc::now().timestamp_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{}-{}-{}-{}", prefix, ts, pid, seq)
}
