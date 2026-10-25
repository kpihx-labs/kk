//! Rendering is an adapter task, not the node's scheduling loop.

use kk::{
    Error, Identity, Result,
    discovery::{Announcement, ServiceKind},
    node::{Event, Node, Options},
};
use tokio::{sync::mpsc, task::JoinHandle};

struct AbortTask(JoinHandle<Result<()>>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn run(
    identity: Identity,
    options: Options,
    name: Option<String>,
    sensitive: bool,
    reveal: bool,
) -> Result<()> {
    let public = identity.peer();
    let (node, _, events) = Node::bind(identity, options).await?;
    let address = node.local_addr()?;
    let announcement = match (name, address) {
        (Some(name), std::net::SocketAddr::V4(address)) => Some(Announcement::from_peer(
            &name,
            address,
            &public,
            ServiceKind::Session,
        )?),
        (Some(_), _) => return Err(Error::Configuration("advertising requires IPv4")),
        (None, _) => None,
    };
    use std::io::Write;
    writeln!(std::io::stderr(), "NODE LISTENING {address}")?;
    let mut core = AbortTask(tokio::spawn(node.run()));
    let mut view = AbortTask(tokio::spawn(render(events, sensitive, reveal)));
    tokio::select! {
        result = &mut core.0 => result.map_err(|_| Error::Node("node task failed"))?,
        result = &mut view.0 => result.map_err(|_| Error::Node("event adapter failed"))?,
        error = async { match &announcement { Some(value) => value.failure().await, None => std::future::pending().await } } => Err(error),
    }
}

async fn render(mut events: mpsc::Receiver<Event>, sensitive: bool, reveal: bool) -> Result<()> {
    while let Some(event) = events.recv().await {
        let line = match event {
            Event::Connected { alias } => format!("PEER_CONNECTED {alias}"),
            Event::Disconnected { alias } => format!("PEER_DISCONNECTED {alias}"),
            Event::Refused { alias, code } => format!("REFUSED {alias:?} {code}"),
            Event::Text { alias, content } => {
                if (sensitive || content.sensitive) && !reveal {
                    format!(
                        "TEXT {alias} {} sensitive bytes (not displayed)",
                        content.text().len()
                    )
                } else {
                    format!("TEXT {alias} {}", content.text().escape_debug())
                }
            }
            Event::File {
                alias,
                path,
                receipt,
            } => {
                if sensitive || receipt.sensitive {
                    format!("FILE {alias} {} sensitive bytes", receipt.info.bytes)
                } else {
                    format!(
                        "FILE {alias} {path:?} {} sha256:{}",
                        receipt.info.bytes,
                        receipt.info.hash_hex()
                    )
                }
            }
        };
        kk::output::line(line).await?;
    }
    Ok(())
}
