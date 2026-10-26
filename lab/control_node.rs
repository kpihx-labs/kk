//! Internal real-process lab runner, NOT a user-facing room command. Only public
//! bootstrap/proposal data crosses the lab controller; identities stay on guests.
use kk::{
    Error, Identity, Peer, Result, config, profile,
    rooms::{
        Anchor, Grant, Rights, RootBundle,
        control::{
            Proposal,
            raft::{
                Node, PeerId,
                network::{self, Factory, Submission},
                service::{ControlNode, Mode, Settings},
            },
        },
        mls::{CryptoMember, PublicIdentity},
    },
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
mod room_fixture;
mod scoped_pairing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    identity: PathBuf,
    genesis: PathBuf,
    creator_certificate: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Genesis {
    root: RootBundle,
    creator: PublicIdentity,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerFile {
    certificate: PathBuf,
    endpoint: SocketAddr,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeConfig {
    context: Context,
    peers: Vec<PeerFile>,
    listen: SocketAddr,
    state: PathBuf,
    status: PathBuf,
    initialize: bool,
    resume: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    pid: u32,
    id: PeerId,
    address: SocketAddr,
    leader_hint: Option<PeerId>,
    epoch: u64,
    head: [u8; 32],
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
enum Command {
    EnrollCandidate {
        config: scoped_pairing::Candidate,
    },
    ScopeListen {
        config: scoped_pairing::Listen,
    },
    ScopeJoin {
        config: scoped_pairing::Join,
    },
    GenesisRoom {
        identity: PathBuf,
        state: PathBuf,
        out: PathBuf,
    },
    ServeRoom {
        config: NodeConfig,
        private_state: PathBuf,
        profile: PathBuf,
        commands: PathBuf,
        member: bool,
        #[serde(default)]
        enrollment: Option<room_fixture::EnrollmentInvite>,
    },
    Genesis {
        identity: PathBuf,
        out: PathBuf,
    },
    Grant {
        context: Context,
        subject_certificate: PathBuf,
        parents: PathBuf,
        role: String,
        out: PathBuf,
    },
    Package {
        context: Context,
        authority: PathBuf,
        state: PathBuf,
        out: PathBuf,
    },
    Serve {
        config: NodeConfig,
    },
    AddProposal {
        context: Context,
        status: PathBuf,
        package: PathBuf,
        out: PathBuf,
    },
    RemoveProposal {
        context: Context,
        status: PathBuf,
        subject_certificate: PathBuf,
        out: PathBuf,
    },
    Submit {
        config: NodeConfig,
        proposal: PathBuf,
        out: PathBuf,
    },
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((config::ROOM_CONTROL_SNAPSHOT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
        return Err(Error::Limit {
            resource: "lab public fixture",
            max: config::ROOM_CONTROL_SNAPSHOT_BYTES,
        });
    }
    Ok(bytes)
}
fn json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read(path)?).map_err(|_| Error::Protocol)
}
fn output(path: &Path, bytes: &[u8]) -> Result<()> {
    profile::private_directory(path.parent().ok_or(Error::Protocol)?, false)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn output_json(path: &Path, value: &impl Serialize) -> Result<()> {
    output(
        path,
        &serde_json::to_vec(value).map_err(|_| Error::Protocol)?,
    )
}
fn role_rights(role: &str) -> Result<Rights> {
    match role {
        "reader" => Ok(Rights::READER),
        "participant" => Ok(Rights::PARTICIPANT),
        "moderator" => Ok(Rights::MODERATOR),
        _ => Err(Error::Protocol),
    }
}
fn publish_json(path: &Path, value: &impl Serialize, replace: bool) -> Result<()> {
    let parent = path.parent().ok_or(Error::Protocol)?;
    profile::private_directory(parent, false)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(file.as_file_mut(), value).map_err(|_| Error::Protocol)?;
    let published = if replace {
        file.persist(path)
    } else {
        file.persist_noclobber(path)
    };
    published.map_err(|_| Error::Storage("cannot publish lab status"))?;
    Ok(())
}
fn anchor(context: &Context) -> Result<(Anchor, Genesis)> {
    let genesis: Genesis = json(&context.genesis)?;
    let expected = Peer::load(&context.creator_certificate)?;
    Ok((genesis.root.verify(&expected)?, genesis))
}
fn peers(config: &NodeConfig) -> Result<(BTreeMap<PeerId, Peer>, BTreeMap<PeerId, Node>)> {
    if config.peers.len() > config::ROOM_MAX_MEMBERS {
        return Err(Error::Protocol);
    }
    let mut trusted = BTreeMap::new();
    let mut nodes = BTreeMap::new();
    for entry in &config.peers {
        let peer = Peer::load(&entry.certificate)?;
        let id = PeerId::from_peer(&peer);
        let node = Node::for_peer(&peer, Some(entry.endpoint));
        if trusted.insert(id, peer).is_some() {
            return Err(Error::Protocol);
        }
        nodes.insert(id, node);
    }
    Ok((trusted, nodes))
}

async fn start(config: &NodeConfig) -> Result<ControlNode> {
    let identity = Arc::new(Identity::load(&config.context.identity)?);
    let (anchor, genesis) = anchor(&config.context)?;
    let (trusted, voters) = peers(config)?;
    let settings = Settings {
        anchor,
        listen: config.listen,
        peers: trusted,
        timing: network::configuration()?,
    };
    let mode = if config.resume {
        Mode::Resume
    } else {
        Mode::Create(genesis.creator)
    };
    let node = ControlNode::start(identity, &config.state, settings, mode).await?;
    if config.initialize
        && let Err(error) = node.initialize(voters).await
    {
        if let Err(cleanup) = node.shutdown().await {
            eprintln!("Control fixture initialization cleanup failed: {cleanup}");
        }
        return Err(error);
    }
    Ok(node)
}
async fn serve(config: NodeConfig) -> Result<()> {
    let node = start(&config).await?;
    let result = async {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => { signal?; break; },
                _ = tick.tick() => {
                    let view = node.public_view().await?;
                    let status = Status { pid: std::process::id(), id: node.id(), address: node.local_addr()?, leader_hint: node.current_leader().await,
                        epoch: view.epoch(), head: view.head() };
                    publish_json(&config.status, &status, true)?;
                }
            }
        }
        Ok(())
    }.await;
    node.shutdown().await?;
    result
}

async fn execute(command: Command) -> Result<()> {
    match command {
        Command::EnrollCandidate { config } => scoped_pairing::enroll_candidate(config).await,
        Command::ScopeListen { config } => scoped_pairing::listen(config).await,
        Command::ScopeJoin { config } => scoped_pairing::join(config).await,
        Command::GenesisRoom {
            identity,
            state,
            out,
        } => room_fixture::genesis(&identity, &state, &out),
        Command::ServeRoom {
            config,
            private_state,
            profile,
            commands,
            member,
            enrollment,
        } => {
            room_fixture::serve(config, private_state, profile, commands, member, enrollment).await
        }
        Command::Genesis { identity, out } => {
            let identity = Identity::load(&identity)?;
            let root = RootBundle::create(&identity, "Cross-guest PUBLIC control fixture")?;
            let anchor = root.verify(&identity.peer())?;
            let creator = CryptoMember::new(&identity, &anchor, vec![])?.public_identity()?;
            output_json(&out, &Genesis { root, creator })
        }
        Command::Grant {
            context,
            subject_certificate,
            parents,
            role,
            out,
        } => {
            let (anchor, _) = anchor(&context)?;
            let identity = Identity::load(&context.identity)?;
            let mut chain = Grant::decode_chain(&read(&parents)?)?;
            let rights = role_rights(&role)?;
            let grant = Grant::issue(
                &anchor,
                &chain,
                &identity,
                &Peer::load(&subject_certificate)?,
                rights,
            )?;
            chain.push(grant);
            output(&out, &Grant::encode_chain(&chain)?)
        }
        Command::Package {
            context,
            authority,
            state,
            out,
        } => {
            let (anchor, _) = anchor(&context)?;
            let mut actor = CryptoMember::create_private(
                &Identity::load(&context.identity)?,
                &anchor,
                Grant::decode_chain(&read(&authority)?)?,
                &state,
            )?;
            output(&out, &actor.key_package()?)
        }
        Command::Serve { config } => serve(config).await,
        Command::AddProposal {
            context,
            status,
            package,
            out,
        } => {
            let (anchor, _) = anchor(&context)?;
            let status: Status = json(&status)?;
            // Deliberately PUBLIC-control-only fixture: no private room effect is
            // claimed for these opaque payload bytes.
            let record = Proposal::add(
                &Identity::load(&context.identity)?,
                anchor.id(),
                status.epoch,
                status.head,
                b"lab public-control-only payload; not an MLS admission",
                &read(&package)?,
            )?;
            output(&out, &record.encode()?)
        }
        Command::RemoveProposal {
            context,
            status,
            subject_certificate,
            out,
        } => {
            let (anchor, _) = anchor(&context)?;
            let status: Status = json(&status)?;
            let record = Proposal::remove(
                &Identity::load(&context.identity)?,
                anchor.id(),
                status.epoch,
                status.head,
                b"lab public-control-only removal; not a private room acknowledgement",
                &Peer::load(&subject_certificate)?,
            )?;
            output(&out, &record.encode()?)
        }
        Command::Submit {
            config,
            proposal,
            out,
        } => {
            let identity = Arc::new(Identity::load(&config.context.identity)?);
            let own = PeerId::from_peer(&identity.peer());
            let (anchor, _) = anchor(&config.context)?;
            let (trusted, voters) = peers(&config)?;
            let (factory, _server) = Factory::bind(
                anchor.id(),
                identity,
                trusted,
                "0.0.0.0:0".parse().map_err(|_| Error::Protocol)?,
            )?;
            let proposal = Proposal::decode(&read(&proposal)?)?;
            let mut target = own;
            let mut node = voters[&target].clone();
            tokio::time::timeout(Duration::from_secs(8), async {
                for _ in 0..config::ROOM_MAX_MEMBERS {
                    match factory
                        .submit(target, &node, proposal.clone(), Duration::from_secs(6))
                        .await?
                    {
                        Submission::Forward {
                            leader: Some(next),
                            node: hint,
                        } if factory.allows(next)? => {
                            target = next;
                            node = hint
                                .or_else(|| voters.get(&next).cloned())
                                .ok_or(Error::Protocol)?;
                        }
                        result => return output_json(&out, &result),
                    }
                }
                Err(Error::Network)
            })
            .await
            .map_err(|_| Error::Timeout)?
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = async {
        let mut args = std::env::args_os().skip(1);
        let path = PathBuf::from(args.next().ok_or(Error::Protocol)?);
        if args.next().is_some() {
            return Err(Error::Protocol);
        }
        execute(json(&path)?).await
    }
    .await;
    if let Err(error) = result {
        eprintln!("{}", error);
        std::process::exit(1);
    }
}
