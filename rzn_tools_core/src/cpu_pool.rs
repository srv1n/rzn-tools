use crate::error::ConnectorError;
use once_cell::sync::Lazy;
use rayon::ThreadPool;
use std::cmp::max;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::oneshot;
use tracing::{debug, info};

static POOL_SIZE: Lazy<usize> = Lazy::new(|| {
    let fallback = 4usize;
    let available = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(fallback);
    available.clamp(2, 8)
});

static CPU_POOL: Lazy<ThreadPool> = Lazy::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(*POOL_SIZE)
        .thread_name(|idx| format!("datasourcer-cpu-{idx}"))
        .build()
        .expect("failed to build datasourcer CPU pool")
});

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Spawn a CPU-intensive job on the datasourcer-dedicated pool.
pub async fn spawn_cpu<F, R>(job: F) -> Result<R, ConnectorError>
where
    F: FnOnce() -> Result<R, ConnectorError> + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    let queued = IN_FLIGHT.fetch_add(1, Ordering::Relaxed) + 1;
    let threshold = max(*POOL_SIZE, 1) * 2;
    if queued > threshold {
        info!(
            target: "datasourcer.cpu_pool",
            queued,
            threads = *POOL_SIZE,
            "datasourcer CPU pool backlog growing"
        );
    } else {
        debug!(
            target: "datasourcer.cpu_pool",
            queued,
            threads = *POOL_SIZE,
            "datasourcer CPU task queued"
        );
    }
    let start = Instant::now();
    CPU_POOL.spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
            .map_err(|payload| {
                let reason = if let Some(msg) = payload.downcast_ref::<&str>() {
                    (*msg).to_string()
                } else if let Some(msg) = payload.downcast_ref::<String>() {
                    msg.clone()
                } else {
                    "unknown panic".to_string()
                };
                ConnectorError::Other(format!("datasourcer CPU task panicked: {}", reason))
            })
            .and_then(|inner| inner);
        let _ = tx.send(result);
        let finished = IN_FLIGHT.fetch_sub(1, Ordering::Relaxed) - 1;
        let latency_ms = start.elapsed().as_millis();
        if latency_ms > 500 {
            info!(
                target: "datasourcer.cpu_pool",
                queue_after = finished,
                latency_ms,
                "datasourcer CPU task finished (slow)"
            );
        } else {
            debug!(
                target: "datasourcer.cpu_pool",
                queue_after = finished,
                latency_ms,
                "datasourcer CPU task finished"
            );
        }
    });

    rx.await
        .map_err(|err| ConnectorError::Other(format!("datasourcer CPU pool join error: {}", err)))?
}

pub fn queue_depth() -> usize {
    IN_FLIGHT.load(Ordering::Relaxed)
}

pub fn worker_count() -> usize {
    *POOL_SIZE
}
