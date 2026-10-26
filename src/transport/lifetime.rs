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
        // Hold registration until insertion: a very short task must not remove
        // itself before its owner has recorded the handle.
        let mut children = self
            .owner
            .0
            .children
            .lock()
            .expect("owned driver task registry");
        if self.owner.0.closing.load(Ordering::SeqCst) {
            drop(children);
            return;
        }
        let id = self.owner.0.next.fetch_add(1, Ordering::SeqCst);
        let active = Active {
            owner: self.owner.clone(),
            id,
        };
        let task = tokio::spawn(async move {
            future.await;
            drop(active);
        });
        let abort = task.abort_handle();
        children.insert(
            id,
            Child {
                abort,
                site: std::panic::Location::caller(),
            },
        );
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
    let owner = DriverOwner(Arc::new(Tasks::default()));
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
        runtime.spawn(Box::pin(std::future::pending()));
        assert_eq!(owner.0.children.lock().unwrap().len(), 1);
        assert!(owner.diagnostics().contains("lifetime.rs"));
        owner.terminate();
        owner.released().await.unwrap();
        assert!(owner.0.children.lock().unwrap().is_empty());
        // A late spawn races shutdown but cannot escape its already closed owner.
        runtime.spawn(Box::pin(std::future::pending()));
        owner.released().await.unwrap();
        assert!(owner.0.children.lock().unwrap().is_empty());
    }
}
