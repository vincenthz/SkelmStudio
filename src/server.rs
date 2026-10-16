//! The local OpenAI-compatible API server (the Developer page), serving the
//! installed models with `skelm-api`.

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::logs;
use crate::runtime;

pub struct ServerHandle {
    pub addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ServerHandle {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn log_requests(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let start = Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    let line = format!(
        "{method} {path} → {} ({:.2}s)",
        status.as_u16(),
        start.elapsed().as_secs_f32()
    );
    if status.is_success() {
        logs::info("server", line);
    } else {
        logs::error("server", line);
    }
    response
}

/// Bind and start serving; resolves once the port is bound
pub async fn start(host: String, port: u16) -> anyhow::Result<ServerHandle> {
    runtime::spawn(async move {
        let ip: IpAddr = host
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid address {host}"))?;
        let addr = SocketAddr::new(ip, port);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let addr = listener.local_addr()?;
        let app = skelm_api::router(skelm_api::AppState::new())
            .layer(axum::middleware::from_fn(log_requests));
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let res = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
            match res {
                Ok(()) => logs::info("server", "server stopped"),
                Err(e) => logs::error("server", format!("server failed: {e}")),
            }
        });
        logs::info("server", format!("listening on http://{addr}"));
        Ok(ServerHandle {
            addr,
            shutdown: Some(tx),
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    #[test]
    fn serves_health_and_models() {
        let handle = crate::runtime::rt()
            .block_on(super::start("127.0.0.1".into(), 0))
            .unwrap();
        let base = format!("http://{}", handle.addr);
        let (health, models) = crate::runtime::rt().block_on(async {
            let health = reqwest::get(format!("{base}/health")).await?.text().await?;
            let models = reqwest::get(format!("{base}/v1/models")).await?;
            anyhow::Ok((health, models.status()))
        })
        .unwrap();
        assert_eq!(health, "ok");
        assert!(models.is_success());
        handle.stop();
    }
}
