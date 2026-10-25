//! Pair/trust adapter. Cryptography and authorization rules live in the library.

use age::secrecy::ExposeSecret;
use clap::{Args, Subcommand, ValueEnum};
use kk::{
    Error, Result,
    contacts::{Permissions, Registry},
    pairing::{Invite, JoinOptions, PairServer, Proposal},
};
use std::{
    io::{self, IsTerminal, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Allowed {
    Text,
    Files,
    Media,
    Rooms,
}
fn permissions(values: &[Allowed]) -> Permissions {
    let mut grants = Permissions::default();
    for value in values {
        match value {
            Allowed::Text => grants.text = true,
            Allowed::Files => grants.files = true,
            Allowed::Media => grants.media = true,
            Allowed::Rooms => grants.rooms = true,
        }
    }
    grants
}

#[derive(Args)]
pub(super) struct PairArgs {
    #[arg(value_name = "ADDRESS", conflicts_with_all = ["listen", "invitation", "qr_image"])]
    address: Option<SocketAddr>,
    /// Issue one invitation on this concrete local IP:port
    #[arg(long, conflicts_with_all = ["invitation", "qr_image"])]
    listen: Option<SocketAddr>,
    /// Local name for the peer; never accepted from untrusted discovery
    #[arg(long = "as")]
    alias: String,
    #[arg(long, value_enum, value_delimiter = ',', default_value = "text")]
    allow: Vec<Allowed>,
    /// Pre-authorize the invitation holder and shown grants (controlled automation)
    #[arg(long)]
    confirm: bool,
    /// Private invitation bundle; no code value appears in argv
    #[arg(long, conflicts_with = "qr_image")]
    invitation: Option<PathBuf>,
    /// Private PNG/JPEG invitation image; decoding does not itself grant trust
    #[arg(long)]
    qr_image: Option<PathBuf>,
    #[arg(long, requires = "listen")]
    invite_out: Option<PathBuf>,
    #[arg(long, requires = "listen")]
    qr_out: Option<PathBuf>,
    #[arg(long, requires = "listen")]
    advertise: Option<String>,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=300))]
    ttl: u64,
    #[arg(long, default_value_t = 45454, value_parser = clap::value_parser!(u16).range(1..))]
    data_port: u16,
}

#[derive(Subcommand)]
pub(super) enum TrustCommand {
    List,
    Revoke {
        alias: String,
    },
    /// Explicitly restore the same previously revoked key, never a replacement key
    Restore {
        alias: String,
    },
    /// Change local incoming rights; peer upgrades require re-pair until session sync
    Allow {
        alias: String,
        #[arg(long, value_enum, value_delimiter = ',', required = true)]
        allow: Vec<Allowed>,
    },
}

pub(super) fn trust(command: TrustCommand, selected: Option<&Path>) -> Result<()> {
    let directory = super::profile_directory(selected)?;
    let registry = Registry::open(directory)?;
    match command {
        TrustCommand::List => {
            for (alias, fingerprint, revoked) in registry.list()? {
                writeln!(
                    io::stdout(),
                    "{alias} sha256:{fingerprint} {}",
                    if revoked { "REVOKED" } else { "active" }
                )?;
            }
        }
        TrustCommand::Revoke { alias } => {
            registry.revoke(&alias)?;
            writeln!(io::stdout(), "REVOKED {alias}")?;
        }
        TrustCommand::Restore { alias } => {
            registry.restore(&alias)?;
            writeln!(io::stdout(), "RESTORED {alias}")?;
        }
        TrustCommand::Allow { alias, allow } => {
            registry.set_incoming(&alias, permissions(&allow))?;
            writeln!(io::stdout(), "PERMISSIONS {alias}")?;
        }
    }
    Ok(())
}

async fn approve(
    proposal: Proposal,
    registry: &Registry,
    alias: &str,
    local: &str,
    confirm: bool,
) -> Result<bool> {
    if registry.is_revoked(&proposal.fingerprint)? {
        return Err(Error::Revoked);
    }
    if let Some(old) = registry.get(alias)?
        && (old.peer()?.fingerprint() != proposal.fingerprint || old.local_identity != local)
    {
        return Err(Error::Identity(
            "alias has another identity; no silent replacement",
        ));
    }
    writeln!(
        io::stderr(),
        "CONFIRM {} peer=sha256:{} incoming={:?} peer-grants={:?}",
        proposal.confirmation,
        proposal.fingerprint,
        proposal.our_grants,
        proposal.their_grants
    )?;
    if confirm {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err(Error::Configuration(
            "pairing confirmation requires a terminal or explicit --confirm",
        ));
    }
    writeln!(
        io::stderr(),
        "Compare CONFIRM on both devices, then type yes to authorize this key."
    )?;
    let answer = kk::input::text(None, false).await?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "yes" | "oui"
    ))
}

pub(super) async fn run(args: PairArgs, selected: Option<&Path>) -> Result<()> {
    let directory = super::profile_directory(selected)?;
    let identity = kk::profile::identity(&directory, true)?;
    let local = identity.peer().fingerprint();
    let registry = Registry::open(directory)?;
    // Validate the local alias and state before issuing an invitation or reading a code.
    if let Some(old) = registry.get(&args.alias)?
        && registry.is_revoked(&old.peer()?.fingerprint())?
    {
        return Err(Error::Revoked);
    }
    let grants = permissions(&args.allow);
    let revision = registry.revision()?;
    let ttl = Duration::from_secs(args.ttl);
    let approve_peer = |proposal| approve(proposal, &registry, &args.alias, &local, args.confirm);
    let commit = |contact| registry.put_if_revision(&args.alias, contact, revision);
    let paired = if let Some(listen) = args.listen {
        if !io::stderr().is_terminal() && args.invite_out.is_none() && args.qr_out.is_none() {
            return Err(Error::Configuration(
                "non-terminal invitations require a private --invite-out or --qr-out",
            ));
        }
        let server = PairServer::bind(listen, ttl).await?;
        let invite = Invite::from_server(&server, &identity)?;
        let advertisement = super::advertised(
            args.advertise,
            server.local_addr()?,
            &identity,
            kk::discovery::ServiceKind::Pairing,
        )?;
        if let Some(path) = &args.invite_out {
            invite.export(path)?;
        }
        if let Some(path) = &args.qr_out {
            invite.qr(path)?;
        }
        if io::stderr().is_terminal() {
            writeln!(
                io::stderr(),
                "PAIR CODE {} (single use, {}s)",
                invite.code().expose_secret(),
                args.ttl
            )?;
        }
        writeln!(io::stderr(), "PAIRING {}", server.local_addr()?)?;
        let ceremony = server.run(&identity, grants, args.data_port, approve_peer, commit);
        if let Some(advertisement) = &advertisement {
            tokio::select! { result = ceremony => result?, error = advertisement.failure() => return Err(error) }
        } else {
            ceremony.await?
        }
    } else {
        let invite = if let Some(path) = args.invitation {
            Some(Invite::load(&path).await?)
        } else if let Some(path) = args.qr_image {
            Some(Invite::load_qr(&path).await?)
        } else {
            None
        };
        let (address, expected_id, expected_fingerprint, code) = if let Some(invite) = invite {
            (
                invite.address,
                Some(invite.id),
                Some(invite.fingerprint.clone()),
                invite.code().clone(),
            )
        } else {
            let address = args.address.ok_or(Error::Configuration(
                "provide an address, --invitation or --qr-image",
            ))?;
            writeln!(
                io::stderr(),
                "Enter the other device's pairing code; it is never sent as a password."
            )?;
            let entered = kk::input::text(None, true).await?;
            let normalized = entered
                .trim()
                .split(|character: char| character == '-' || character.is_whitespace())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join("-")
                .to_ascii_lowercase();
            (
                address,
                None,
                None,
                age::secrecy::SecretString::from(normalized),
            )
        };
        kk::pairing::join(
            JoinOptions {
                address,
                expected_id,
                expected_fingerprint,
                code,
                grants,
                data_port: args.data_port,
                deadline: ttl,
            },
            &identity,
            approve_peer,
            commit,
        )
        .await?
    };
    writeln!(
        io::stdout(),
        "PAIRED {} sha256:{}",
        args.alias,
        paired.fingerprint
    )?;
    Ok(())
}
