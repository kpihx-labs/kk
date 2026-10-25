use clap::{Args, CommandFactory, Parser, Subcommand};
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

#[derive(Parser)]
#[command(
    name = "kk",
    version,
    about = "Direct encrypted communication; ordinary traffic is never plaintext"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args)]
struct TrustArgs {
    #[arg(long)]
    identity: PathBuf,
    #[arg(long)]
    peer: PathBuf,
}
impl TrustArgs {
    fn load(&self) -> Result<(Identity, Peer)> {
        Ok((Identity::load(&self.identity)?, Peer::load(&self.peer)?))
    }
}

#[derive(Subcommand)]
enum Command {
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// Send literal text by default; -f selects a path, '-' selects stdin
    Send {
        destination: SocketAddr,
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
    match Cli::parse().command {
        None => Cli::command().print_help()?,
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
            let (identity, peer) = trust.load()?;
            let deadline = Duration::from_secs(timeout);
            let sealing = seal || seal_to.is_some();
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
                let receipt = kk::files::send_file_with_policy(
                    destination,
                    &identity,
                    &peer,
                    &path,
                    sensitive || sealing,
                    deadline,
                )
                .await?;
                file_receipt("FILE_ACK", &receipt, sensitive || sealing)?;
            } else {
                let text = text.expect("text input");
                kk::send_with_policy(destination, &identity, &peer, &text, sensitive, deadline)
                    .await?;
                writeln!(io::stdout(), "ACK {} bytes", text.len())?;
            }
        }
        Some(Command::Receive {
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
            let (identity, peer) = trust.load()?;
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
                let receive = receiver.receive(Duration::from_secs(timeout));
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
                let receive = listener.receive(Duration::from_secs(timeout));
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
