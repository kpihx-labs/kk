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
    #[serde(default)]
    background: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Download {
    #[serde(default)]
    preview: bool,
}

fn raster_preview_type(bytes: &[u8], max_pixels: u64) -> Result<&'static str> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::Configuration("cannot inspect image preview"))?;
    let mime = match reader.format() {
        Some(image::ImageFormat::Png) => "image/png",
        Some(image::ImageFormat::Jpeg) => "image/jpeg",
        _ => {
            return Err(Error::Configuration(
                "preview supports PNG/JPEG only; save the original copy instead",
            ));
        }
    };
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(config::UI_FILE_PREVIEW_BYTES);
    if mime == "image/png" {
        let decoder = image::codecs::png::PngDecoder::with_limits(
            std::io::Cursor::new(bytes),
            limits.clone(),
        )
        .map_err(|_| {
            Error::Configuration("PNG preview metadata is invalid; save the copy instead")
        })?;
        if decoder
            .is_apng()
            .map_err(|_| Error::Configuration("PNG animation metadata is unavailable"))?
        {
            return Err(Error::Configuration(
                "animated PNG is outside the single-image preview policy; save the copy instead",
            ));
        }
    }
    reader.limits(limits);
    let (width, height) = reader.into_dimensions().map_err(|_| {
        Error::Configuration("image preview dimensions are unavailable; save the copy instead")
    })?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > max_pixels {
        return Err(Error::Configuration(
            "image exceeds preview pixel bound; save the copy instead",
        ));
    }
    Ok(mime)
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
        let command = if self.options.background {
            Command::FileStart {
                source: self.source.path().to_owned(),
                target: self.options.to,
                sensitive: self.options.sensitive,
            }
        } else {
            Command::File {
                source: self.source.path().to_owned(),
                target: self.options.to,
                sensitive: self.options.sensitive,
            }
        };
        let reply = local::request(&app.0.profile, &self.room, &command).await?;
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
    local::directory(&app.0.profile, &room)?;
    let _slot = app
        .0
        .upload_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Node("UI file lane is full"))?;
    let command = if options.background {
        Command::ForwardReceivedStart {
            id: name,
            target: options.to,
            sensitive: options.sensitive,
        }
    } else {
        Command::ForwardReceived {
            id: name,
            target: options.to,
            sensitive: options.sensitive,
        }
    };
    let reply = local::request(&app.0.profile, &room, &command).await?;
    Ok(Json(json!(reply)))
}

pub(super) async fn transfers(
    State(app): State<App>,
    Path(room): Path<String>,
) -> std::result::Result<Json<Value>, Failure> {
    Ok(Json(json!(
        local::request(&app.0.profile, &room, &Command::FileTransfers).await?
    )))
}

pub(super) async fn transfer_status(
    State(app): State<App>,
    Path((room, id)): Path<(String, String)>,
) -> std::result::Result<Json<Value>, Failure> {
    Ok(Json(json!(
        local::request(&app.0.profile, &room, &Command::FileTransferStatus { id }).await?
    )))
}

pub(super) async fn transfer_cancel(
    State(app): State<App>,
    Path((room, id)): Path<(String, String)>,
) -> std::result::Result<Json<Value>, Failure> {
    Ok(Json(json!(
        local::request(&app.0.profile, &room, &Command::FileTransferCancel { id }).await?
    )))
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
    Query(options): Query<Download>,
) -> std::result::Result<Response, Failure> {
    filename(&name)?;
    let slot = app
        .0
        .upload_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Node("UI file lane is full"))?;
    let inbox = local::directory(&app.0.profile, &room)?.join("inbox");
    let received = crate::files::open_received(
        &inbox,
        &name,
        if options.preview {
            config::UI_FILE_PREVIEW_BYTES
        } else {
            config::DEFAULT_FILE_LIMIT
        },
        config::UI_REQUEST_TIMEOUT,
    )
    .await?;
    let length = received.info.bytes;
    // Retain the verified inode; never reopen an inbox path after verification.
    let mut file = received.into_file();
    if options.preview {
        let mut bytes = Vec::new();
        let mut bounded = (&mut file).take(config::UI_FILE_PREVIEW_BYTES + 1);
        tokio::time::timeout(config::UI_REQUEST_TIMEOUT, bounded.read_to_end(&mut bytes))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::Io)?;
        drop(bounded);
        if bytes.len() as u64 != length || bytes.len() as u64 > config::UI_FILE_PREVIEW_BYTES {
            return Err(Error::Integrity.into());
        }
        let mime = raster_preview_type(&bytes, config::UI_FILE_PREVIEW_PIXELS)?;
        let stream = futures_util::stream::try_unfold(
            (axum::body::Bytes::from(bytes), file, slot),
            |(mut bytes, file, slot)| async move {
                if bytes.is_empty() {
                    return Ok::<_, std::io::Error>(None);
                }
                let chunk = bytes.split_to(bytes.len().min(config::FILE_CHUNK_BYTES));
                Ok(Some((chunk, (bytes, file, slot))))
            },
        );
        let mut response = Body::from_stream(stream).into_response();
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, mime.parse().unwrap());
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, length.into());
        response
            .headers_mut()
            .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, "attachment".parse().unwrap());
        return Ok(response);
    }
    let stream = futures_util::stream::try_unfold((file, slot), |(mut file, slot)| async move {
        let mut bytes = vec![0; config::FILE_CHUNK_BYTES];
        let count = file.read(&mut bytes).await?;
        if count == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        bytes.truncate(count);
        Ok(Some((bytes, (file, slot))))
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

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn preview_sniffs_only_bounded_png_and_jpeg_without_decoding_pixels() {
        for (format, mime) in [
            (image::ImageFormat::Png, "image/png"),
            (image::ImageFormat::Jpeg, "image/jpeg"),
        ] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 2)
                .write_to(&mut bytes, format)
                .unwrap();
            assert_eq!(raster_preview_type(bytes.get_ref(), 4).unwrap(), mime);
            assert!(raster_preview_type(bytes.get_ref(), 3).is_err());
        }
        for bytes in [
            b"<html>active</html>".as_slice(),
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
            b"GIF89a".as_slice(),
            b"\x89PNG\r\n\x1a\n".as_slice(),
        ] {
            assert!(raster_preview_type(bytes, config::UI_FILE_PREVIEW_PIXELS).is_err());
        }
    }
}
