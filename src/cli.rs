use clap::{Args, CommandFactory, Parser, Subcommand};
use kk::contacts::{Action, Registry};
use kk::{
    Error, Identity, Listener, Peer, Result,
    discovery::{Announcement, ServiceKind},
    envelope::{Lock, Unlock},
    files::{FileReceiver, ReceivePolicy},
};
use std::{
    io::{self, IsTerminal, Write},
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
use zeroize::Zeroizing;
mod media;
mod pair;
#[cfg(unix)]
mod replica;
#[cfg(unix)]
mod room;
#[cfg(unix)]
mod serve;

#[derive(Parser)]
#[command(
    name = "kk",
    version,
    about = "Direct encrypted communication; ordinary traffic is never plaintext"
)]
struct Cli {
    #[arg(long, global = true)]
    profile: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args)]
struct TrustArgs {
    #[arg(long)]
    identity: Option<PathBuf>,
    #[arg(long)]
    peer: Option<PathBuf>,
}
impl TrustArgs {
    fn load(
        &self,
        selected: Option<&std::path::Path>,
        alias: Option<&str>,
        action: Action,
        inbound: bool,
    ) -> Result<(Identity, Peer, Option<SocketAddr>, Registry)> {
        let directory = profile_directory(selected)?;
        let registry = Registry::open(directory.clone())?;
        let identity = match &self.identity {
            Some(path) => Identity::load(path)?,
            None => kk::profile::identity(&directory, false)?,
        };
        let (peer, endpoint) = if let Some(alias) = alias {
            if self.peer.is_some() {
                return Err(Error::Configuration(
                    "named contacts cannot override their certificate",
                ));
            }
            let contact = registry.resolve(alias, &identity, action, inbound)?;
            (contact.peer()?, contact.endpoints.first().copied())
        } else {
            let path = self.peer.as_ref().ok_or(Error::Configuration(
                "select a paired contact or an explicit --peer certificate",
            ))?;
            (Peer::load(path)?, None)
        };
        registry.authorize_peer(&peer, &identity, action, inbound)?;
        Ok((identity, peer, endpoint, registry))
    }
}

fn profile_directory(selected: Option<&std::path::Path>) -> Result<PathBuf> {
    let path = match selected {
        Some(path) => path.to_owned(),
        None => kk::profile::base_directory()?,
    };
    kk::profile::private_directory(&path, true)?;
    Ok(path)
}

#[derive(Subcommand)]
enum Command {
    /// Private local light/dark workspace; foreground SDK rooms, no installed service
    #[cfg(unix)]
    Ui {
        #[arg(long, default_value = "127.0.0.1:0")]
        bind: SocketAddr,
        #[arg(long)]
        no_open: bool,
        /// Optional executable receiving a protected launch path and public base URL
        #[arg(long)]
        opener: Option<PathBuf>,
        #[arg(long = "opener-arg", requires = "opener", allow_hyphen_values = true)]
        opener_args: Vec<String>,
    },
    /// Native media capability checks and explicit local capture preview
    Media {
        #[command(subcommand)]
        command: media::Command,
    },
    /// Persistent encrypted multi-user rooms; no implicit background service
    #[cfg(unix)]
    Room {
        #[command(subcommand)]
        command: room::Command,
    },
    /// Foreground persistent inbox; no service is installed or auto-started
    #[cfg(unix)]
    Serve {
        #[arg(long)]
        bind: SocketAddr,
        #[arg(long, requires = "inbox")]
        accept_files: bool,
        #[arg(long)]
        inbox: Option<PathBuf>,
        #[arg(long, default_value_t = kk::config::DEFAULT_FILE_LIMIT)]
        max_bytes: u64,
        #[arg(long, default_value_t = kk::config::NODE_FILE_BUDGET)]
        file_budget: u64,
        #[arg(long)]
        advertise: Option<String>,
        #[arg(short = 's', long)]
        sensitive: bool,
        #[arg(long)]
        reveal: bool,
    },
    #[cfg(unix)]
    Status,
    /// Initialize a private default identity; never replaces an existing key
    Init,
    /// Single-use PAKE pairing, confirmation, strict TLS proof and contact storage
    Pair(pair::PairArgs),
    Trust {
        #[command(subcommand)]
        command: pair::TrustCommand,
    },
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// Send literal text by default; -f selects a path, '-' selects stdin
    Send {
        destination: String,
        payload: Option<String>,
        #[arg(short = 'f', long, requires = "payload")]
        file: bool,
        /// Hide local input/output; literal sensitive text in argv is refused
        #[arg(short = 's', long)]
        sensitive: bool,
        /// Retain authenticated age ciphertext at the destination; public recipient
        #[arg(long, conflicts_with = "seal")]
        seal_to: Option<String>,
        /// Passphrase encryption, prompted locally without echo
        #[arg(long)]
        seal: bool,
        /// Optional durable ciphertext artifact, reused as a file after interruption
        #[arg(long)]
        envelope: Option<PathBuf>,
        #[command(flatten)]
        trust: TrustArgs,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86400))]
        timeout: u64,
    },
    /// Receive once; -f enables an explicitly consented file destination
    Receive {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        bind: SocketAddr,
        #[arg(short = 'f', long, requires_all = ["output", "accept"])]
        file: bool,
        #[arg(short = 's', long)]
        sensitive: bool,
        /// Explicitly show sensitive text instead of the default redacted receipt
        #[arg(long)]
        reveal: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        accept: bool,
        #[arg(long, default_value_t = kk::config::DEFAULT_FILE_LIMIT)]
        max_bytes: u64,
        #[arg(long)]
        advertise: Option<String>,
        #[command(flatten)]
        trust: TrustArgs,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86400))]
        timeout: u64,
    },
    Peers {
        #[arg(long)]
        interface: Ipv4Addr,
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=15))]
        timeout: u64,
    },
    Inspect {
        source: PathBuf,
    },
    /// Generate an age private identity without printing it; stdout is PUBLIC
    Keygen {
        output: PathBuf,
    },
    /// Decrypt an age artifact to a NEW private file, never automatically
    Unseal {
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, conflicts_with = "passphrase")]
        key_file: Option<PathBuf>,
        #[arg(long, required_unless_present = "key_file")]
        passphrase: bool,
    },
}

#[derive(Subcommand)]
enum IdentityCommand {
    Create { directory: PathBuf },
    Show { certificate: PathBuf },
}

fn advertised(
    name: Option<String>,
    address: SocketAddr,
    identity: &Identity,
    kind: ServiceKind,
) -> Result<Option<Announcement>> {
    match (name, address) {
        (Some(name), SocketAddr::V4(address)) => {
            Ok(Some(Announcement::new(&name, address, identity, kind)?))
        }
        (Some(_), _) => Err(Error::Configuration("advertising requires an IPv4 bind")),
        (None, _) => Ok(None),
    }
}

fn cipher_path(selected: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = selected {
        return Ok(path);
    }
    let directory = kk::profile::data_directory()?.join("envelopes");
    kk::profile::private_directory(&directory, true)?;
    let reservation = tempfile::Builder::new()
        .prefix("sealed-")
        .tempfile_in(directory)?;
    let path = reservation.path().with_extension("age");
    drop(reservation);
    Ok(path)
}

pub(crate) async fn run() -> Result<()> {
    let Cli { profile, command } = Cli::parse();
    let command = match command {
        #[cfg(unix)]
        Some(Command::Ui {
            bind,
            no_open,
            opener,
            opener_args,
        }) => {
            let directory = profile_directory(profile.as_deref())?;
            return tokio::task::LocalSet::new()
                .run_until(kk::ui::run(
                    directory,
                    kk::ui::Options {
                        bind,
                        open_browser: !no_open,
                        opener,
                        opener_args,
                    },
                ))
                .await;
        }
        Some(Command::Media { command }) => return media::run(command).await,
        command => command,
    };
    #[cfg(unix)]
    let command = match command {
        Some(Command::Room { command }) => {
            return tokio::task::LocalSet::new()
                .run_until(room::run(command, profile.as_deref()))
                .await;
        }
        command => command,
    };
    tokio::select! {
        biased;
        signal = tokio::signal::ctrl_c() => { signal?; Err(Error::Cancelled) },
        result = execute(profile, command) => result,
    }
}

async fn execute(profile: Option<PathBuf>, command: Option<Command>) -> Result<()> {
    match command {
        #[cfg(unix)]
        Some(Command::Ui { .. }) => unreachable!("the UI owns its foreground SDK lifecycle"),
        Some(Command::Media { .. }) => unreachable!("native media owns its subprocess shutdown"),
        #[cfg(unix)]
        Some(Command::Room { .. }) => unreachable!("room commands own their shutdown boundary"),
        None => Cli::command().print_help()?,
        #[cfg(unix)]
        Some(Command::Status) => {
            let directory = profile_directory(profile.as_deref())?;
            let reply = kk::node::ipc::request(
                &directory,
                &kk::node::ipc::Request::Status,
                kk::config::DEFAULT_TIMEOUT,
            )
            .await?
            .ok_or(Error::Node("no active node"))?;
            reply.result()?;
            writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string(&reply.status.ok_or(Error::Protocol)?)
                    .map_err(|_| Error::Protocol)?
            )?;
        }
        #[cfg(unix)]
        Some(Command::Serve {
            bind,
            accept_files,
            inbox,
            max_bytes,
            file_budget,
            advertise,
            sensitive,
            reveal,
        }) => {
            if inbox.is_some() && !accept_files {
                return Err(Error::Configuration(
                    "an inbox requires explicit --accept-files",
                ));
            }
            let directory = profile_directory(profile.as_deref())?;
            let identity = kk::profile::identity(&directory, false)?;
            serve::run(
                identity,
                kk::node::Options {
                    profile: directory,
                    bind,
                    inbox,
                    max_file_bytes: max_bytes,
                    file_budget,
                },
                advertise,
                sensitive,
                reveal,
            )
            .await?;
        }
        Some(Command::Init) => {
            let directory = profile_directory(profile.as_deref())?;
            let identity = kk::profile::identity(&directory, true)?;
            writeln!(
                io::stdout(),
                "IDENTITY sha256:{}",
                identity.peer().fingerprint()
            )?;
        }
        Some(Command::Pair(arguments)) => pair::run(arguments, profile.as_deref()).await?,
        Some(Command::Trust { command }) => pair::trust(command, profile.as_deref())?,
        Some(Command::Identity { command }) => {
            let peer = match command {
                IdentityCommand::Create { directory } => Identity::create(&directory)?.peer(),
                IdentityCommand::Show { certificate } => Peer::load(&certificate)?,
            };
            writeln!(io::stdout(), "sha256:{}", peer.fingerprint())?;
        }
        Some(Command::Peers { interface, timeout }) => {
            for candidate in
                kk::discovery::discover(interface, Duration::from_secs(timeout)).await?
            {
                for address in candidate.addresses {
                    writeln!(
                        io::stdout(),
                        "UNVERIFIED {:?} {} sha256:{} {}",
                        candidate.name,
                        address,
                        candidate.claimed_fingerprint,
                        candidate.kind.as_str()
                    )?;
                }
            }
        }
        Some(Command::Inspect { source }) => {
            let info = kk::files::inspect(&source, kk::config::DEFAULT_FILE_TIMEOUT).await?;
            writeln!(
                io::stdout(),
                "FILE {} sha256:{}",
                info.bytes,
                info.hash_hex()
            )?;
        }
        Some(Command::Keygen { output }) => {
            writeln!(io::stdout(), "{}", kk::envelope::keygen(&output).await?)?
        }
        Some(Command::Unseal {
            source,
            output,
            key_file,
            passphrase: _,
        }) => {
            let unlock = match key_file {
                Some(path) => Unlock::Identity(path),
                None => Unlock::Passphrase(kk::input::passphrase(false).await?),
            };
            let count =
                kk::envelope::open(&source, &output, unlock, kk::config::MAX_FILE_TIMEOUT).await?;
            writeln!(io::stdout(), "DECRYPTED {count} bytes")?;
        }
        Some(Command::Send {
            destination,
            payload,
            file,
            sensitive,
            seal_to,
            seal,
            envelope,
            trust,
            timeout,
        }) => {
            let text = if file {
                None
            } else {
                Some(kk::input::text(payload.as_deref(), sensitive).await?)
            };
            let sealing = seal || seal_to.is_some();
            let action = if file || sealing {
                Action::Files
            } else {
                Action::Text
            };
            let explicit = destination.parse::<SocketAddr>().ok();
            let alias = if explicit.is_some() {
                None
            } else {
                Some(destination.as_str())
            };
            let (identity, peer, hint, registry) =
                trust.load(profile.as_deref(), alias, action, false)?;
            let destination = explicit
                .or(hint)
                .ok_or(Error::Configuration("contact has no endpoint hint"))?;
            let deadline = Duration::from_secs(timeout);
            if envelope.is_some() && !sealing {
                return Err(Error::Configuration("an envelope path requires encryption"));
            }
            let file_path = if sealing {
                let lock = match seal_to {
                    Some(recipient) => Lock::Recipient(recipient),
                    None => Lock::Passphrase(kk::input::passphrase(true).await?),
                };
                let path = cipher_path(envelope)?;
                if file {
                    kk::envelope::seal_file(
                        std::path::Path::new(payload.as_deref().expect("required file path")),
                        &path,
                        lock,
                        deadline,
                    )
                    .await?;
                } else {
                    kk::envelope::seal_bytes(
                        text.as_ref().expect("text input").as_bytes(),
                        &path,
                        lock,
                        deadline,
                    )
                    .await?;
                }
                writeln!(
                    io::stderr(),
                    "ENVELOPE {} (retain this ciphertext for retry)",
                    path.display()
                )?;
                Some(path)
            } else if file {
                Some(PathBuf::from(payload.expect("required file path")))
            } else {
                None
            };
            if let Some(path) = file_path {
                #[cfg(unix)]
                if let Some(alias) = alias
                    && trust.identity.is_none()
                    && trust.peer.is_none()
                {
                    let source = if path.is_absolute() {
                        path.clone()
                    } else {
                        std::env::current_dir()?.join(&path)
                    };
                    let request = kk::node::ipc::Request::File {
                        target: alias.to_owned(),
                        source,
                        sensitive: sensitive || sealing,
                        seconds: timeout,
                    };
                    if let Some(reply) =
                        kk::node::ipc::request(registry.directory(), &request, deadline).await?
                    {
                        file_receipt("FILE_ACK", &reply.receipt(), sensitive || sealing)?;
                        return Ok(());
                    }
                }
                let receipt = kk::files::send_file_authorized(
                    destination,
                    &identity,
                    &peer,
                    &path,
                    sensitive || sealing,
                    deadline,
                    || registry.authorize_peer(&peer, &identity, Action::Files, false),
                )
                .await?;
                file_receipt("FILE_ACK", &receipt, sensitive || sealing)?;
            } else {
                let text = text.expect("text input");
                #[cfg(unix)]
                if let Some(alias) = alias
                    && trust.identity.is_none()
                    && trust.peer.is_none()
                {
                    let request = kk::node::ipc::Request::Text {
                        target: alias.to_owned(),
                        text: text.to_string(),
                        sensitive,
                        seconds: timeout,
                    };
                    if let Some(reply) =
                        kk::node::ipc::request(registry.directory(), &request, deadline).await?
                    {
                        writeln!(io::stdout(), "ACK {} bytes", reply.bytes)?;
                        return Ok(());
                    }
                }
                kk::send_authorized(
                    destination,
                    &identity,
                    &peer,
                    &text,
                    sensitive,
                    deadline,
                    || registry.authorize_peer(&peer, &identity, Action::Text, false),
                )
                .await?;
                writeln!(io::stdout(), "ACK {} bytes", text.len())?;
            }
        }
        Some(Command::Receive {
            from,
            bind,
            file,
            sensitive,
            reveal,
            output,
            accept: _,
            max_bytes,
            advertise,
            trust,
            timeout,
        }) => {
            let action = if file { Action::Files } else { Action::Text };
            let (identity, peer, _, registry) =
                trust.load(profile.as_deref(), from.as_deref(), action, true)?;
            if file {
                let policy =
                    ReceivePolicy::accept(output.expect("required file destination"), max_bytes)?;
                let receiver = FileReceiver::bind(bind, &identity, &peer, policy)?;
                let advertisement = advertised(
                    advertise,
                    receiver.local_addr()?,
                    &identity,
                    ServiceKind::Files,
                )?;
                writeln!(io::stderr(), "LISTENING {}", receiver.local_addr()?)?;
                let receive = receiver.receive_authorized(Duration::from_secs(timeout), || {
                    registry.authorize_peer(&peer, &identity, Action::Files, true)
                });
                let receipt = if let Some(advertisement) = &advertisement {
                    tokio::select! { result = receive => result?, error = advertisement.failure() => return Err(error) }
                } else {
                    receive.await?
                };
                file_receipt("RECEIVED_FILE", &receipt, sensitive)?;
            } else {
                let listener = Listener::bind(bind, &identity, &peer)?;
                let advertisement = advertised(
                    advertise,
                    listener.local_addr()?,
                    &identity,
                    ServiceKind::Messages,
                )?;
                writeln!(io::stderr(), "LISTENING {}", listener.local_addr()?)?;
                let receive = listener.receive_authorized(Duration::from_secs(timeout), || {
                    registry.authorize_peer(&peer, &identity, Action::Text, true)
                });
                let received = if let Some(advertisement) = &advertisement {
                    tokio::select! { result = receive => result?, error = advertisement.failure() => return Err(error) }
                } else {
                    receive.await?
                };
                let content = Zeroizing::new(received.text().to_owned());
                if let Some(output) = output {
                    kk::files::save_bytes(
                        &output,
                        content.as_bytes(),
                        &peer,
                        sensitive || received.sensitive,
                    )
                    .await?;
                    writeln!(io::stdout(), "SAVED {} bytes", content.len())?;
                } else if (sensitive || received.sensitive) && !reveal {
                    writeln!(
                        io::stdout(),
                        "RECEIVED {} sensitive bytes (not displayed)",
                        content.len()
                    )?;
                } else if io::stdout().is_terminal() {
                    writeln!(io::stdout(), "{}", content.escape_debug())?;
                } else {
                    writeln!(io::stdout(), "{}", content.as_str())?;
                }
            }
        }
    }
    Ok(())
}

fn file_receipt(action: &str, receipt: &kk::files::Receipt, local_sensitive: bool) -> Result<()> {
    if receipt.sensitive || local_sensitive {
        writeln!(
            io::stdout(),
            "{action} {} resumed={} sensitive",
            receipt.info.bytes,
            receipt.resumed_from
        )?;
    } else {
        writeln!(
            io::stdout(),
            "{action} {} resumed={} sha256:{}",
            receipt.info.bytes,
            receipt.resumed_from,
            receipt.info.hash_hex()
        )?;
    }
    Ok(())
}
