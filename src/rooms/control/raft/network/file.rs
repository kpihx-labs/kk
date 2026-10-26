//! Consented direct room files reuse the existing verified-prefix file protocol.
use super::*;
use crate::files::{self, Inbox, Receipt};
use std::path::{Path, PathBuf};

impl Factory {
    pub(crate) fn enable_files(
        &self,
        directory: PathBuf,
        max_bytes: u64,
        budget: u64,
    ) -> Result<()> {
        let inbox = Inbox::new(directory, max_bytes, budget)?;
        let mut selected = self
            .shared
            .room_files
            .write()
            .map_err(|_| Error::Protocol)?;
        if selected.is_some() {
            return Err(Error::Configuration("room inbox is already configured"));
        }
        *selected = Some(Arc::new(inbox));
        Ok(())
    }
    pub(crate) async fn room_file(
        &self,
        target: PeerId,
        node: &Node,
        source: &Path,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<Receipt> {
        transport::validate_duration(deadline, config::DEFAULT_FILE_TIMEOUT)?;
        let shared = &self.shared;
        let _permit = shared
            .file_data
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Node("room file lane is full"))?;
        let handle = shared
            .room_port
            .read()
            .map_err(|_| Error::Protocol)?
            .clone()
            .ok_or(Error::Denied)?;
        let peer = shared
            .peers
            .read()
            .map_err(|_| Error::Protocol)?
            .get(&target)
            .cloned()
            .ok_or(Error::Denied)?;
        let client = Client {
            shared: Arc::downgrade(shared),
            target,
            node: node.clone(),
        };
        let mut used = None;
        let result = tokio::time::timeout(deadline, async {
            handle
                .authorize_file(peer.clone(), false, config::DEFAULT_TIMEOUT)
                .await?;
            let mut file = files::source_file(source)?;
            let info = files::info(&mut file).await?;
            let connection =
                tokio::time::timeout(config::DEFAULT_TIMEOUT, client.connection(shared))
                    .await
                    .map_err(|_| Error::Timeout)??;
            used = Some(connection.stable_id());
            if !authorized(shared, target)? {
                return Err(Error::Denied);
            }
            let (mut send, mut recv) = connection.open_bi().await.map_err(|_| Error::Network)?;
            send.set_priority(-2).map_err(|_| Error::Network)?;
            send_frame(
                &mut send,
                &Envelope {
                    room: shared.room,
                    request: Request::RoomFile,
                },
            )
            .await?;
            let response = tokio::time::timeout(config::DEFAULT_TIMEOUT, read_prefix(&mut recv))
                .await
                .map_err(|_| Error::Timeout)??;
            match response {
                Response::RoomFile(DataReply::Accepted(room)) if room == shared.room => {}
                Response::RoomFile(DataReply::Denied) => return Err(Error::Denied),
                Response::RoomFile(DataReply::Busy) => {
                    return Err(Error::Node("remote room inbox is busy"));
                }
                _ => return Err(Error::Protocol),
            }
            files::stream::send_with_async(&mut file, info, sensitive, send, recv, || {
                handle.authorize_file(peer.clone(), false, config::DEFAULT_TIMEOUT)
            })
            .await
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if matches!(&result, Err(Error::Timeout | Error::Network)) {
            client.retire_unresponsive(shared, used);
        }
        result
    }
}

pub(super) async fn receive(
    shared: &Shared,
    id: PeerId,
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
) -> Result<()> {
    let inbox = shared
        .room_files
        .read()
        .map_err(|_| Error::Protocol)?
        .clone();
    let handle = shared
        .room_port
        .read()
        .map_err(|_| Error::Protocol)?
        .clone();
    let permit = shared.file_data.clone().try_acquire_owned();
    let (Some(inbox), Some(handle), Ok(_permit)) = (inbox, handle, permit) else {
        send_frame(&mut send, &Response::RoomFile(DataReply::Denied)).await?;
        send.finish().map_err(|_| Error::Network)?;
        return Err(Error::Denied);
    };
    let peer = shared
        .peers
        .read()
        .map_err(|_| Error::Protocol)?
        .get(&id)
        .cloned()
        .ok_or(Error::Denied)?;
    if handle
        .authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT)
        .await
        .is_err()
    {
        send_frame(&mut send, &Response::RoomFile(DataReply::Denied)).await?;
        send.finish().map_err(|_| Error::Network)?;
        return Err(Error::Denied);
    }
    send_frame(
        &mut send,
        &Response::RoomFile(DataReply::Accepted(shared.room)),
    )
    .await?;
    let receipt = files::stream::receive_with_async(
        &peer,
        recv,
        send,
        || handle.authorize_file(peer.clone(), true, config::DEFAULT_TIMEOUT),
        |info| inbox.choose(&peer, info),
    )
    .await?;
    inbox.complete(&receipt);
    Ok(())
}
