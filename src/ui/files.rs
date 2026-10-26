//! Explicit uploads and owned inbox downloads, never arbitrary filesystem paths.
use super::*;
use crate::rooms::{
    control::raft::PeerId,
    local::{self, Command},
};
use axum::{body::Body, extract::Query};
use futures_util::StreamExt;
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Upload {
    to: PeerId,
    #[serde(default)]
    sensitive: bool,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum CopyAction {
    Share { id: String, peers: Vec<PeerId> },
    Unshare { sha256: String },
    Locate { sha256: String, peers: Vec<PeerId> },
    Fetch { sha256: String, peer: PeerId },
}
impl CopyAction {
    pub(super) async fn run(self, app: App, room: &str) -> Result<Value> {
        // A local withdrawal must remain available when bulk slots are occupied.
        let _slot = if matches!(&self, Self::Unshare { .. }) {
            None
        } else {
            Some(
                app.0
                    .upload_slots
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Error::Node("UI file lane is full"))?,
            )
        };
        let command = match self {
            Self::Share { id, peers } => {
                if peers.is_empty() || peers.len() >= config::ROOM_MAX_MEMBERS {
                    return Err(Error::Denied);
                }
                filename(&id)?;
                let inbox = local::directory(&app.0.profile, room)?.join("inbox");
                crate::profile::private_directory(&inbox, false)?;
                let source = inbox.join(id);
                let file = crate::files::source_file(&source)?;
                private(&file.metadata().await?)?;
                drop(file);
                Command::FileShare {
                    source,
                    audience: peers.into_iter().collect(),
                }
            }
            Self::Unshare { sha256 } => Command::FileUnshare {
                hash: crate::files::parse_hash(&sha256)?,
            },
            Self::Locate { sha256, peers } => {
                if peers.is_empty() || peers.len() >= config::ROOM_MAX_MEMBERS {
                    return Err(Error::Denied);
                }
                Command::FileLocate {
                    hash: crate::files::parse_hash(&sha256)?,
                    peers: peers.into_iter().collect(),
                }
            }
            Self::Fetch { sha256, peer } => Command::FileFetch {
                hash: crate::files::parse_hash(&sha256)?,
                peer,
            },
        };
        Ok(json!(local::request(&app.0.profile, room, &command).await?))
    }
}
struct Reservation {
    _slot: tokio::sync::OwnedSemaphorePermit,
    budget: Arc<AtomicU64>,
    bytes: u64,
}
pub(super) struct Transfer {
    source: tempfile::NamedTempFile,
    _reservation: Reservation,
    room: String,
    options: Upload,
}
impl Transfer {
    pub(super) async fn run(self, app: App) -> Result<Value> {
        let reply = local::request(
            &app.0.profile,
            &self.room,
            &Command::File {
                source: self.source.path().to_owned(),
                target: self.options.to,
                sensitive: self.options.sensitive,
            },
        )
        .await?;
        Ok(json!(reply))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.fetch_add(self.bytes, Ordering::SeqCst);
    }
}

pub(super) async fn upload(
    State(app): State<App>,
    Path(room): Path<String>,
    Query(options): Query<Upload>,
    body: Body,
) -> std::result::Result<Json<Value>, Failure> {
    local::directory(&app.0.profile, &room)?;
    let slot = app
        .0
        .upload_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Node("UI transfer capacity exhausted"))?;
    let directory = app.0.profile.join("ui/uploads");
    crate::profile::private_directory(&directory, true)?;
    let source = tempfile::NamedTempFile::new_in(&directory).map_err(Error::Io)?;
    let mut file = tokio::fs::File::from_std(source.reopen().map_err(Error::Io)?);
    let mut reservation = Reservation {
        _slot: slot,
        budget: app.0.upload_budget.clone(),
        bytes: 0,
    };
    let mut chunks = body.into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| {
            Error::Configuration("file upload interrupted; no automatic network retry")
        })?;
        let bytes = reservation
            .bytes
            .checked_add(chunk.len() as u64)
            .ok_or(Error::FileTooLarge)?;
        if bytes > config::DEFAULT_FILE_LIMIT {
            return Err(Error::FileTooLarge.into());
        }
        reservation
            .budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(chunk.len() as u64)
            })
            .map_err(|_| Error::FileTooLarge)?;
        reservation.bytes = bytes;
        file.write_all(&chunk).await.map_err(Error::Io)?;
    }
    file.flush().await.map_err(Error::Io)?;
    file.sync_all().await.map_err(Error::Io)?;
    drop(file);
    let (reply, result) = oneshot::channel();
    let transfer = Transfer {
        source,
        _reservation: reservation,
        room,
        options,
    };
    app.0
        .requests
        .try_send(driver::Request {
            action: driver::Action::Transfer(transfer),
            reply,
        })
        .map_err(|_| Error::Node("UI transfer queue is full"))?;
    let result = result.await.map_err(|_| Error::Cancelled)??;
    Ok(Json(result))
}

fn filename(name: &str) -> Result<()> {
    let stem = name
        .strip_suffix(".bin")
        .ok_or(Error::Destination("not an inbox object"))?;
    let (source, digest) = stem
        .split_once('-')
        .ok_or(Error::Destination("invalid inbox object"))?;
    PeerId::try_from(source.to_owned()).map_err(|_| Error::Destination("invalid inbox sender"))?;
    PeerId::try_from(digest.to_owned()).map_err(|_| Error::Destination("invalid inbox digest"))?;
    Ok(())
}
fn private(metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(Error::Destination("unsafe inbox object"));
    }
    Ok(())
}
pub(super) async fn forward(
    State(app): State<App>,
    Path((room, name)): Path<(String, String)>,
    Query(options): Query<Upload>,
) -> std::result::Result<Json<Value>, Failure> {
    filename(&name)?;
    let inbox = local::directory(&app.0.profile, &room)?.join("inbox");
    crate::profile::private_directory(&inbox, false)?;
    let source = inbox.join(name);
    let file = crate::files::source_file(&source)?;
    private(&file.metadata().await.map_err(Error::Io)?)?;
    drop(file);
    let reply = local::request(
        &app.0.profile,
        &room,
        &Command::File {
            source,
            target: options.to,
            sensitive: options.sensitive,
        },
    )
    .await?;
    Ok(Json(json!(reply)))
}
pub(super) async fn list(
    State(app): State<App>,
    Path(room): Path<String>,
) -> std::result::Result<Json<Value>, Failure> {
    let inbox = local::directory(&app.0.profile, &room)?.join("inbox");
    if !inbox.exists() {
        return Ok(Json(json!([])));
    }
    crate::profile::private_directory(&inbox, false)?;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(inbox).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Destination("invalid inbox filename"))?;
        if filename(&name).is_err() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(Error::Io)?;
        private(&metadata)?;
        if files.len() >= config::NODE_FILE_COUNT_BUDGET as usize {
            return Err(Error::Node("inbox view limit reached; stored files are intact").into());
        }
        files.push(json!({"id": name, "bytes": metadata.len()}));
    }
    Ok(Json(json!(files)))
}
pub(super) async fn download(
    State(app): State<App>,
    Path((room, name)): Path<(String, String)>,
) -> std::result::Result<Response, Failure> {
    filename(&name)?;
    let inbox = local::directory(&app.0.profile, &room)?.join("inbox");
    crate::profile::private_directory(&inbox, false)?;
    let file = crate::files::source_file(&inbox.join(&name))?;
    let metadata = file.metadata().await.map_err(Error::Io)?;
    private(&metadata)?;
    let length = metadata.len();
    let stream = futures_util::stream::try_unfold(file, |mut file| async move {
        let mut bytes = vec![0; config::FILE_CHUNK_BYTES];
        let count = file.read(&mut bytes).await?;
        if count == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        bytes.truncate(count);
        Ok(Some((bytes, file)))
    });
    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length.into());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{name}\"")
            .parse()
            .map_err(|_| Error::Protocol)?,
    );
    Ok(response)
}
