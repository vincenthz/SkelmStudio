//! The tokio runtime the network side (Hub, registry, API server) runs on.
//!
//! GPUI drives its own executor; futures spawned here are awaited from it
//! through a oneshot channel, which works across executors.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub fn rt() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("skelm-net")
            .enable_all()
            .build()
            .expect("failed to start the tokio runtime")
    })
}

/// Run `fut` on the tokio runtime, returning a future (for any executor) that
/// resolves to its output, or an error if the task panicked or was aborted.
pub fn spawn<F, T>(fut: F) -> impl Future<Output = anyhow::Result<T>> + 'static
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    rt().spawn(async move {
        let _ = tx.send(fut.await);
    });
    async move {
        rx.await
            .map_err(|_| anyhow::anyhow!("background task was cancelled"))
    }
}

/// Like [`spawn`] for blocking work, run on tokio's blocking pool.
pub fn spawn_blocking<F, T>(f: F) -> impl Future<Output = anyhow::Result<T>> + 'static
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    rt().spawn_blocking(move || {
        let _ = tx.send(f());
    });
    async move {
        rx.await
            .map_err(|_| anyhow::anyhow!("background task was cancelled"))
    }
}
