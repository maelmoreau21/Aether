//! The network runs on a small Tokio runtime (reqwest and the WebSocket need
//! it); GPUI tasks await its results through a oneshot channel.
use std::future::Future;

pub struct Net {
    runtime: tokio::runtime::Runtime,
}

impl Net {
    pub fn new() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("aether-net")
            .enable_all()
            .build()?;
        Ok(Self { runtime })
    }
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.runtime.handle()
    }
    /// Runs `future` on the network runtime; the returned future, awaited on
    /// GPUI's executor, yields its output (`None` if the runtime shut down).
    pub fn run<T, F>(&self, future: F) -> impl Future<Output = Option<T>> + use<T, F>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.runtime.spawn(async move {
            let _ = sender.send(future.await);
        });
        async move { receiver.await.ok() }
    }
}

/// The client core's TLS settings (Mozilla roots through ring), shared by the
/// remote-control socket.
pub fn tls() -> rustls::ClientConfig {
    aether_client::tls_config().unwrap_or_else(|_| {
        // Only fails if ring lacks the default protocol versions, which it has.
        unreachable!("ring supports TLS 1.2 and 1.3")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_on_the_runtime_and_comes_back() {
        let net = Net::new().unwrap();
        let answer = net.run(async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            std::thread::current().name().map(str::to_owned)
        });
        let name = futures::executor::block_on(answer).unwrap();
        assert_eq!(name.as_deref(), Some("aether-net"));
        assert!(tls().alpn_protocols.is_empty());
    }
}
