//! Browser media is a local capture/render adapter for the SAME authorized RTP
//! SDK. No device names, SDP trust decisions or private identities cross the UI.
use super::*;
use crate::{
    media::{Kind, Packet},
    rooms::{
        control::raft::PeerId,
        local::media::{Publisher, Receiver},
    },
};
use axum::extract::{
    Query, WebSocketUpgrade,
    ws::{Message, WebSocket},
};
use futures_util::SinkExt;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Publish {
    #[serde(default)]
    to: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Listen {
    from: PeerId,
}
fn kind(value: &str) -> Result<Kind> {
    match value {
        "audio" => Ok(Kind::Audio),
        "camera" => Ok(Kind::Camera),
        "screen" => Ok(Kind::Screen),
        _ => Err(Error::Configuration("unknown media kind")),
    }
}
pub(super) async fn publish(
    State(app): State<App>,
    Path((room, name)): Path<(String, String)>,
    Query(options): Query<Publish>,
    websocket: WebSocketUpgrade,
) -> std::result::Result<Response, Failure> {
    let kind = kind(&name)?;
    let peers = options
        .to
        .split(',')
        .filter(|peer| !peer.is_empty())
        .map(|peer| {
            PeerId::try_from(peer.to_owned())
                .map_err(|_| Error::Configuration("media recipients require full fingerprints"))
        })
        .collect::<Result<Vec<_>>>()?;
    let publisher = Publisher::connect(&app.0.profile, &room, kind, peers).await?;
    Ok(websocket
        .protocols(["kk-v1"])
        .max_message_size(config::MEDIA_PACKET_BYTES)
        .max_frame_size(config::MEDIA_PACKET_BYTES)
        .on_upgrade(move |socket| async move {
            if let Err(error) = send(app, socket, publisher, kind).await {
                eprintln!("Browser media publication ended: {}", error.code());
            }
        }))
}
async fn send(app: App, mut socket: WebSocket, mut publisher: Publisher, kind: Kind) -> Result<()> {
    let mut stopped = app.0.stop.subscribe();
    socket.send(Message::Text(json!({"event": "ready", "peers": publisher.recipients(), "meaning": "datagrams-not-delivery-receipts"}).to_string().into())).await.map_err(|_| Error::Cancelled)?;
    let forwarding = async {
        loop {
            let message = tokio::select! {
                changed = stopped.changed() => { let _ = changed; break; },
                message = tokio::time::timeout(config::MEDIA_SOURCE_IDLE, socket.recv()) => message.map_err(|_| Error::Timeout)?,
            };
            match message {
                Some(Ok(Message::Binary(bytes))) => publisher.send(&Packet::parse(kind.codec(), bytes.to_vec())?).await?,
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {},
                _ => return Err(Error::Protocol),
            }
        }
        Ok(())
    }.await;
    let reply = publisher.finish().await;
    if let Ok(reply) = reply {
        let _ = socket
            .send(Message::Text(
                json!({"event": "ended", "result": reply})
                    .to_string()
                    .into(),
            ))
            .await;
    }
    let _ = tokio::time::timeout(config::MEDIA_PACKET_MAX_AGE, socket.close()).await;
    forwarding
}
pub(super) async fn listen(
    State(app): State<App>,
    Path((room, name)): Path<(String, String)>,
    Query(options): Query<Listen>,
    websocket: WebSocketUpgrade,
) -> std::result::Result<Response, Failure> {
    let kind = kind(&name)?;
    let receiver = Receiver::connect(&app.0.profile, &room, kind, options.from).await?;
    Ok(websocket
        .protocols(["kk-v1"])
        .max_message_size(64)
        .max_frame_size(64)
        .on_upgrade(move |socket| async move {
            if let Err(error) = receive(app, socket, receiver).await {
                eprintln!("Browser media subscription ended: {}", error.code());
            }
        }))
}
async fn receive(app: App, mut socket: WebSocket, mut receiver: Receiver) -> Result<()> {
    let mut stopped = app.0.stop.subscribe();
    socket
        .send(Message::Text(json!({"event": "ready"}).to_string().into()))
        .await
        .map_err(|_| Error::Cancelled)?;
    loop {
        let frame = tokio::select! {
            changed = stopped.changed() => { let _ = changed; break; },
            message = socket.recv() => match message { Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue, _ => break },
            frame = receiver.next() => frame?,
        };
        let Some(frame) = frame else {
            break;
        };
        let mut bytes = Vec::with_capacity(frame.packet.bytes().len() + 24);
        bytes.extend(frame.received_ms.to_be_bytes());
        bytes.extend(frame.track);
        bytes.extend(frame.packet.bytes());
        tokio::time::timeout(
            config::MEDIA_PACKET_MAX_AGE,
            socket.send(Message::Binary(bytes.into())),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Cancelled)?;
    }
    let _ = tokio::time::timeout(config::MEDIA_PACKET_MAX_AGE, socket.close()).await;
    Ok(())
}
