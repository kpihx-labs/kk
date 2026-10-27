//! Explicit local room lifecycle. Configuration contains public bootstrap data
//! only; MLS keys and consensus state stay in separate private stores.
use super::{
    RootBundle,
    control::raft::{
        self,
        service::{ControlNode, Mode, Settings},
    },
    group::Group,
    mls::{CryptoMember, PublicIdentity},
    runtime::Room,
};
use crate::{Error, Identity, Peer, Result, contacts::Registry, profile};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::SocketAddr,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
};
pub mod media;
mod session;
pub use session::{Command, InvitationOptions, Reply, ReplyEvent, invite, join, request, serve};

pub struct Options {
    pub bind: Option<SocketAddr>,
    pub reveal: bool,
    pub inbox: Option<PathBuf>,
    pub max_bytes: u64,
    pub file_budget: u64,
    pub observer: Option<Observer>,
}
/// A foreground adapter owns readiness, bounded events and structured stop.
/// None preserves the CLI console adapter; it never installs a background service.
pub struct Observer {
    pub ready: Option<tokio::sync::oneshot::Sender<()>>,
    pub events: tokio::sync::mpsc::Sender<super::runtime::Event>,
    pub stop: tokio::sync::watch::Receiver<bool>,
}
#[derive(Serialize)]
pub struct Summary {
    pub name: String,
    pub title: Option<String>,
    pub room: Option<raft::PeerId>,
    pub problem: Option<String>,
}

pub fn list(profile: &Path) -> Result<Vec<Summary>> {
    profile::private_directory(profile, false)?;
    let peer = Peer::load(&profile.join("identity/certificate.der"))?;
    let rooms = profile.join("rooms");
    if !rooms.exists() {
        return Ok(Vec::new());
    }
    profile::private_directory(&rooms, false)?;
    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(&rooms)? {
        if summaries.len() >= crate::config::UI_MAX_ROOMS {
            return Err(Error::Limit {
                resource: "local room list",
                max: crate::config::UI_MAX_ROOMS,
            });
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Storage("room directory name is not UTF-8"))?;
        let summary = match room_path(profile, &name, false)
            .and_then(|directory| Definition::load_for_peer(&directory, &peer))
        {
            Ok(definition) => Summary {
                name,
                title: Some(
                    definition
                        .root
                        .verify_id(definition.room.0)?
                        .title()
                        .to_owned(),
                ),
                room: Some(definition.room),
                problem: None,
            },
            Err(error) => Summary {
                name,
                title: None,
                room: None,
                problem: Some(error.to_string()),
            },
        };
        summaries.push(summary);
    }
    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(summaries)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    version: u8,
    owner: raft::PeerId,
    room: raft::PeerId,
    root: RootBundle,
    origin: PublicIdentity,
    listen: SocketAddr,
    #[serde(default)]
    inviter: Option<Vec<u8>>,
    #[serde(default)]
    seeds: Vec<super::enrollment::Seed>,
}
#[derive(Serialize)]
struct RootDraft {
    version: u8,
    root: RootBundle,
    listen: SocketAddr,
}
#[derive(Serialize)]
struct AddressRecord {
    version: u8,
    room: raft::PeerId,
    owner: raft::PeerId,
    previous: SocketAddr,
    endpoint: SocketAddr,
    configuration: openraft::LogId<raft::PeerId>,
}

pub fn directory(profile: &Path, name: &str) -> Result<PathBuf> {
    room_path(profile, name, true)
}

fn room_path(profile: &Path, name: &str, create_parent: bool) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 40
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(Error::Configuration(
            "room name must contain 1..40 lowercase letters, digits or hyphens",
        ));
    }
    profile::private_directory(profile, false)?;
    let rooms = profile.join("rooms");
    profile::private_directory(&rooms, create_parent)?;
    Ok(rooms.join(name))
}

fn reserve(profile: &Path, name: &str, listen: SocketAddr) -> Result<PathBuf> {
    if listen.port() == 0 || listen.ip().is_unspecified() || listen.ip().is_multicast() {
        return Err(Error::Configuration(
            "rooms require an explicit reachable IP and nonzero port",
        ));
    }
    let directory = directory(profile, name)?;
    // Exclusive creation: interrupted enrollment is retained, never regenerated.
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    Ok(directory)
}

impl Definition {
    fn load(directory: &Path, identity: &Identity) -> Result<Self> {
        Self::load_for_peer(directory, &identity.peer())
    }

    fn load_for_peer(directory: &Path, peer: &Peer) -> Result<Self> {
        use rustix::fs::{Mode, OFlags, open};
        profile::private_directory(directory, false)?;
        let mut file = std::fs::File::from(open(
            directory.join("manifest.json"),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let stat = file.metadata()?;
        if !stat.is_file()
            || stat.uid() != rustix::process::geteuid().as_raw()
            || stat.mode() & 0o777 != 0o600
            || stat.nlink() != 1
            || stat.len() > super::MAX_BUNDLE as u64 * 2
        {
            return Err(Error::Storage("unsafe or oversized room manifest"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take((super::MAX_BUNDLE * 2 + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > super::MAX_BUNDLE * 2 {
            return Err(Error::Protocol);
        }
        let value: Self =
            serde_json::from_slice(&bytes).map_err(|_| Error::Storage("invalid room manifest"))?;
        if value.version != 1 || value.owner != raft::PeerId::from_peer(peer) {
            return Err(Error::Authentication);
        }
        let anchor = value.root.verify_id(value.room.0)?;
        let origin = value.origin.verify(&anchor)?;
        if origin.grant_id() != anchor.id()
            || origin.peer().fingerprint_bytes()
                != anchor.verify_chain(&[])?.peer().fingerprint_bytes()
        {
            return Err(Error::Authentication);
        }
        Ok(value)
    }
    fn save(&self, directory: &Path) -> Result<()> {
        save_public(&directory.join("manifest.json"), self)
    }
    async fn resume(
        &self,
        identity: Arc<Identity>,
        directory: &Path,
        profile: &Path,
        bind: Option<SocketAddr>,
    ) -> Result<(Room, Arc<ControlNode>)> {
        let anchor = self.root.verify_id(self.room.0)?;
        let creator = anchor.verify_chain(&[])?.peer();
        let candidate = CryptoMember::open_private(&identity, &anchor, &directory.join("private"))?;
        let initialized = Group::has_membership(&candidate)?;
        let mut peers = BTreeMap::new();
        if self.seeds.len() > crate::config::ROOM_MAX_MEMBERS {
            return Err(Error::Protocol);
        }
        for seed in &self.seeds {
            let peer = crate::Peer::from_der(seed.certificate.clone())?;
            if peers.insert(raft::PeerId::from_peer(&peer), peer).is_some() {
                return Err(Error::Protocol);
            }
        }
        // Existing pre-checkpoint manifests were written only after activation.
        // Their committed control store restores the actual scoped roster.
        if peers.is_empty() {
            peers.insert(raft::PeerId::from_peer(&creator), creator);
        }
        peers.insert(self.owner, identity.peer());
        let control = Arc::new(
            ControlNode::start(
                identity.clone(),
                &directory.join("control"),
                Settings {
                    anchor,
                    listen: bind.unwrap_or(self.listen),
                    peers,
                    timing: raft::network::configuration()?,
                },
                Mode::Resume,
            )
            .await?,
        );
        let opened = async {
            if bind.is_some() {
                let endpoint = control.local_addr()?;
                if endpoint.port() == 0
                    || endpoint.ip().is_unspecified()
                    || endpoint.ip().is_multicast()
                {
                    return Err(Error::Configuration(
                        "publish an explicit concrete room address",
                    ));
                }
                let configuration = control
                    .update_address(&self.seeds, crate::config::MAX_TIMEOUT)
                    .await?;
                if endpoint != self.listen {
                    use ring::rand::{SecureRandom, SystemRandom};
                    let history = directory.join("address-history");
                    profile::private_directory(&history, true)?;
                    let mut nonce = [0; 16];
                    SystemRandom::new()
                        .fill(&mut nonce)
                        .map_err(|_| Error::Authentication)?;
                    let record = AddressRecord {
                        version: 1,
                        room: self.room,
                        owner: self.owner,
                        previous: self.listen,
                        endpoint,
                        configuration,
                    };
                    save_public(
                        &history.join(format!(
                            "{:020}-{:032x}.json",
                            configuration.index,
                            u128::from_be_bytes(nonce)
                        )),
                        &record,
                    )?;
                    let mut next = self.clone();
                    next.listen = endpoint;
                    let destination = directory.join("manifest.json");
                    stage_public(&destination, &next)?
                        .persist(&destination)
                        .map_err(|error| Error::Io(error.error))?;
                    std::fs::File::open(directory)?.sync_all()?;
                }
            }
            let registry = Registry::open(profile.to_owned())?;
            if initialized {
                Room::new(
                    identity,
                    control.clone(),
                    Group::restore(candidate)?,
                    registry,
                )
            } else {
                let inviter =
                    crate::Peer::from_der(self.inviter.clone().ok_or(Error::Storage(
                        "candidate has no approved enrollment context; verified rejoin required",
                    ))?)?;
                Room::join(
                    identity,
                    control.clone(),
                    candidate,
                    &inviter,
                    registry,
                    crate::config::MAX_TIMEOUT,
                )
                .await
            }
        }
        .await;
        match opened {
            Ok(room) => Ok((room, control)),
            Err(error) => {
                match Arc::try_unwrap(control) {
                    Ok(node) => {
                        if let Err(cleanup) = node.shutdown().await {
                            eprintln!("Room recovery cleanup failed: {cleanup}");
                        }
                    }
                    Err(_) => eprintln!("Room recovery cleanup failed: active owner remains"),
                }
                Err(error)
            }
        }
    }
}

fn save_public(path: &Path, value: &impl Serialize) -> Result<()> {
    stage_public(path, value)?
        .persist_noclobber(path)
        .map_err(|error| Error::Io(error.error))?;
    std::fs::File::open(profile::parent(path)?)?.sync_all()?;
    Ok(())
}
fn stage_public(path: &Path, value: &impl Serialize) -> Result<tempfile::NamedTempFile> {
    let directory = profile::parent(path)?;
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Protocol)?;
    if bytes.len() > super::MAX_BUNDLE * 2 {
        return Err(Error::Protocol);
    }
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    Ok(file)
}

pub async fn create(
    profile: &Path,
    name: &str,
    title: &str,
    listen: SocketAddr,
) -> Result<raft::PeerId> {
    let identity = Arc::new(profile::identity(profile, false)?);
    let root = RootBundle::create(&identity, title)?;
    let anchor = root.verify(&identity.peer())?;
    let directory = reserve(profile, name, listen)?;
    save_public(
        &directory.join("root.json"),
        &RootDraft {
            version: 1,
            root: root.clone(),
            listen,
        },
    )?;
    let member =
        CryptoMember::create_private(&identity, &anchor, vec![], &directory.join("private"))?;
    let origin = member.public_identity()?;
    drop(Group::create(member)?);
    let owner = raft::PeerId::from_peer(&identity.peer());
    let room = raft::PeerId(anchor.id());
    let control = ControlNode::start(
        identity.clone(),
        &directory.join("control"),
        Settings {
            anchor,
            listen,
            peers: BTreeMap::from([(owner, identity.peer())]),
            timing: raft::network::configuration()?,
        },
        Mode::Create(origin.clone()),
    )
    .await?;
    let result = async {
        control
            .initialize(BTreeMap::from([(
                owner,
                raft::Node::for_peer(&identity.peer(), Some(listen)),
            )]))
            .await?;
        Definition {
            version: 1,
            owner,
            room,
            root,
            origin,
            listen,
            inviter: None,
            seeds: vec![super::enrollment::Seed {
                certificate: identity.peer().certificate_der().to_vec(),
                endpoint: listen,
            }],
        }
        .save(&directory)
    }
    .await;
    let stopped = control.shutdown().await;
    result?;
    stopped?;
    Ok(room)
}
