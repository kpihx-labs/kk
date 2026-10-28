//! Track the actual QUIC driver lifetime, not just connection/stream closure.
//! Quinn's endpoint driver exits AFTER its last handle drops; wait_idle alone
//! does not mean the bound UDP socket has been released for immediate restart.
use crate::{Error, Result, config};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct Tasks {
    runtime: Option<tokio::runtime::Handle>,
    next: AtomicUsize,
    closing: AtomicBool,
    children: Mutex<BTreeMap<usize, Child>>,
    changed: Notify,
}
#[derive(Debug)]
struct Child {
    abort: tokio::task::AbortHandle,
    site: &'static std::panic::Location<'static>,
}
#[derive(Debug, Clone)]
pub(crate) struct DriverOwner(Arc<Tasks>);
impl DriverOwner {
    /// Register application work with the same cancellation/destruction barrier
    /// as QUIC drivers. A sealed owner drops captures before returning None.
    #[track_caller]
    pub(crate) fn spawn(
        &self,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> Option<tokio::task::AbortHandle> {
        // Hold registration until insertion: even an immediately completed task
        // must not remove itself before its handle has been recorded.
        let mut children = self.0.children.lock().expect("owned driver task registry");
        if self.0.closing.load(Ordering::SeqCst) {
            return None;
        }
        let runtime = self
            .0
            .runtime
            .clone()
            .or_else(|| tokio::runtime::Handle::try_current().ok())?;
        let id = self.0.next.fetch_add(1, Ordering::SeqCst);
        let active = Active {
            owner: self.clone(),
            id,
        };
        let task = runtime.spawn(async move {
            future.await;
            drop(active);
        });
        let abort = task.abort_handle();
        children.insert(
            id,
            Child {
                abort: abort.clone(),
                site: std::panic::Location::caller(),
            },
        );
        Some(abort)
    }
    pub(crate) fn terminate(&self) {
        self.0.closing.store(true, Ordering::SeqCst);
        let children = self.0.children.lock().expect("owned driver task registry");
        for child in children.values() {
            child.abort.abort();
        }
    }
    pub(crate) fn diagnostics(&self) -> String {
        let children = self.0.children.lock().expect("owned driver task registry");
        children
            .iter()
            .map(|(id, child)| format!("{id}@{}:{}", child.site.file(), child.site.line()))
            .collect::<Vec<_>>()
            .join(", ")
    }
    pub(crate) async fn released(&self) -> Result<()> {
        tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            loop {
                let changed = self.0.changed.notified();
                if self
                    .0
                    .children
                    .lock()
                    .expect("owned driver task registry")
                    .is_empty()
                {
                    return;
                }
                changed.await;
            }
        })
        .await
        .map_err(|_| Error::Node("QUIC driver did not release its owned socket"))
    }
}
struct Active {
    owner: DriverOwner,
    id: usize,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.owner
            .0
            .children
            .lock()
            .expect("owned driver task registry")
            .remove(&self.id);
        self.owner.0.changed.notify_waiters();
    }
}
#[derive(Debug)]
struct OwnedRuntime {
    runtime: quinn::TokioRuntime,
    owner: DriverOwner,
}
impl quinn::Runtime for OwnedRuntime {
    fn new_timer(&self, time: Instant) -> Pin<Box<dyn quinn::AsyncTimer>> {
        self.runtime.new_timer(time)
    }
    #[track_caller]
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        self.owner.spawn(future);
    }
    fn wrap_udp_socket(
        &self,
        socket: std::net::UdpSocket,
    ) -> std::io::Result<Arc<dyn quinn::AsyncUdpSocket>> {
        self.runtime.wrap_udp_socket(socket)
    }
}
pub(crate) fn server(
    config: quinn::ServerConfig,
    address: std::net::SocketAddr,
) -> Result<(quinn::Endpoint, DriverOwner)> {
    tokio::runtime::Handle::try_current()
        .map_err(|_| Error::Configuration("QUIC endpoint requires an active Tokio runtime"))?;
    let owner = DriverOwner(Arc::new(Tasks {
        runtime: Some(tokio::runtime::Handle::current()),
        ..Tasks::default()
    }));
    let runtime = Arc::new(OwnedRuntime {
        runtime: quinn::TokioRuntime,
        owner: owner.clone(),
    });
    let socket = std::net::UdpSocket::bind(address)?;
    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(config),
        socket,
        runtime,
    )?;
    Ok((endpoint, owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quinn::Runtime;
    #[tokio::test]
    async fn application_spawn_reuses_the_recorded_runtime_and_joins_capture_destruction() {
        let owner = DriverOwner(Arc::new(Tasks {
            runtime: Some(tokio::runtime::Handle::current()),
            ..Tasks::default()
        }));
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let borrowed = owner.clone();
        let abort = std::thread::spawn(move || {
            borrowed.spawn(async move {
                let _socket = socket;
                entered.send(()).unwrap();
                std::future::pending::<()>().await;
            })
        })
        .join()
        .unwrap()
        .unwrap();
        waiting.await.unwrap();
        abort.abort();
        owner.released().await.unwrap();
        assert!(std::net::UdpSocket::bind(address).is_ok());
        assert!(owner.diagnostics().is_empty());
        owner.terminate();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        assert!(
            owner
                .spawn(async move {
                    let _socket = socket;
                    std::future::pending::<()>().await;
                })
                .is_none()
        );
        assert!(
            std::net::UdpSocket::bind(address).is_ok(),
            "sealed application spawn must drop captures synchronously"
        );
    }
    #[tokio::test]
    async fn close_grace_can_expire_during_peer_drain_without_leaking_the_owned_socket() {
        let directory = tempfile::tempdir().unwrap();
        let identity = crate::Identity::create(&directory.path().join("owner")).unwrap();
        let peer = identity.peer();
        let server_config = crate::transport::server_config(
            &identity,
            std::slice::from_ref(&peer),
            &[b"drain-test"],
            crate::transport::Policy::Room,
        )
        .unwrap();
        let (mut endpoint, owner) = server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let address = endpoint.local_addr().unwrap();
        endpoint.set_default_client_config(
            crate::transport::client_config(
                &identity,
                &peer,
                b"drain-test",
                crate::transport::Policy::Room,
            )
            .unwrap(),
        );
        // An owned loopback sink receives the Initial but never replies. There
        // is deliberately no RTT sample: Quinn's three-PTO close timer exceeds
        // our best-effort grace even though application closure is immediate.
        let sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let connecting = endpoint
            .connect(sink.local_addr().unwrap(), "localhost")
            .unwrap();
        let mut packet = [0; 2048];
        tokio::time::timeout(config::DEFAULT_TIMEOUT, sink.recv_from(&mut packet))
            .await
            .unwrap()
            .unwrap();
        endpoint.close(0u32.into(), b"");
        assert!(matches!(
            connecting.await,
            Err(quinn::ConnectionError::LocallyClosed)
        ));
        assert!(
            tokio::time::timeout(config::QUIC_SHUTDOWN_GRACE, endpoint.wait_idle())
                .await
                .is_err(),
            "a silent peer's close timer should outlive the bounded grace"
        );
        assert!(owner.diagnostics().contains("connection.rs"));
        // Distinguish ordinary protocol drain from a stuck driver: without
        // cancellation, this same connection finishes on its finite PTO timer.
        tokio::time::timeout(config::DEFAULT_TIMEOUT, endpoint.wait_idle())
            .await
            .unwrap();
        assert!(!owner.diagnostics().contains("connection.rs"));
        drop(endpoint);
        owner.released().await.unwrap();
        assert!(owner.diagnostics().is_empty());
        assert!(
            std::net::UdpSocket::bind(address).is_ok(),
            "completed owned shutdown must release the socket despite peer drain"
        );
    }
    #[tokio::test]
    async fn a_sealed_owner_drops_late_driver_resources_before_spawn_returns() {
        let owner = DriverOwner(Arc::new(Tasks::default()));
        let runtime = OwnedRuntime {
            runtime: quinn::TokioRuntime,
            owner: owner.clone(),
        };
        owner.terminate();
        owner.released().await.unwrap();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        runtime.spawn(Box::pin(async move {
            let _socket = socket;
            std::future::pending::<()>().await;
        }));
        assert!(
            std::net::UdpSocket::bind(address).is_ok(),
            "a late driver retained the stopped owner's socket"
        );
        assert!(owner.0.children.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn owner_terminates_and_joins_unfinished_transport_children_without_waiting_on_peer_timers()
     {
        let owner = DriverOwner(Arc::new(Tasks::default()));
        let runtime = OwnedRuntime {
            runtime: quinn::TokioRuntime,
            owner: owner.clone(),
        };
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        runtime.spawn(Box::pin(async move {
            let _socket = socket;
            std::future::pending::<()>().await;
        }));
        assert_eq!(owner.0.children.lock().unwrap().len(), 1);
        assert!(owner.diagnostics().contains("lifetime.rs"));
        owner.terminate();
        owner.released().await.unwrap();
        assert!(owner.0.children.lock().unwrap().is_empty());
        assert!(
            std::net::UdpSocket::bind(address).is_ok(),
            "release must join resource destruction, not just request cancellation"
        );
        // A late spawn races shutdown but cannot escape its already closed owner.
        runtime.spawn(Box::pin(std::future::pending()));
        owner.released().await.unwrap();
        assert!(owner.0.children.lock().unwrap().is_empty());
    }
}
