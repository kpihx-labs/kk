use clap::{Args, Subcommand, ValueEnum};
use kk::{
    Error, Result,
    rooms::{Rights, control::raft::PeerId, local},
};
use std::{
    io::{self, IsTerminal, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, ValueEnum)]
pub enum Role {
    Reader,
    Participant,
    Inviter,
    Moderator,
}
impl Role {
    fn rights(self) -> Rights {
        match self {
            Self::Reader => Rights::READER,
            Self::Participant => Rights::PARTICIPANT,
            Self::Inviter => Rights::INVITER,
            Self::Moderator => Rights::MODERATOR,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Explicit audience-bound replica offers and verified local copy retrieval
    Files {
        name: String,
        #[command(subcommand)]
        command: super::replica::Command,
    },
    /// Explicit live media publication and per-source local playback consent
    Media {
        name: String,
        #[command(subcommand)]
        command: super::media::RoomCommand,
    },
    /// Create a new room with private local keys and a single explicit controller
    Create {
        name: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        bind: SocketAddr,
    },
    /// Resume this room in the foreground, including its private command socket
    Serve {
        name: String,
        /// Bind and publish this device's authenticated new address
        #[arg(long)]
        bind: Option<SocketAddr>,
        #[command(flatten)]
        receive: ReceiveArgs,
    },
    /// Cached durable state, explicitly not a fresh quorum observation
    Status { name: String },
    /// Gracefully stop the foreground room without deleting any state
    Stop { name: String },
    /// Broadcast positional text; '-' selects stdin, '-s' hides local input
    Send {
        name: String,
        payload: Option<String>,
        #[arg(short = 's', long)]
        sensitive: bool,
        #[arg(short = 'f', long, requires_all = ["payload", "to"])]
        file: bool,
        #[arg(long, requires = "file")]
        to: Option<String>,
    },
    /// Single-use scoped PAKE invitation, with confirmation on both devices
    Invite {
        name: String,
        #[arg(long)]
        bind: SocketAddr,
        #[arg(long)]
        invite_out: PathBuf,
        #[arg(long, value_enum, default_value = "participant")]
        role: Role,
        /// Explicitly approve the proved key without an interactive SAS prompt
        #[arg(long)]
        confirm: bool,
    },
    /// Join using an independently received private invitation, then serve
    Join {
        name: String,
        #[arg(long)]
        invitation: PathBuf,
        #[arg(long)]
        bind: SocketAddr,
        #[arg(long, value_enum, default_value = "participant")]
        role: Role,
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        receive: ReceiveArgs,
    },
    /// Explicitly select admitted administrative voters by full certificate digest
    Controllers {
        name: String,
        #[arg(required = true, num_args = 1..)]
        voters: Vec<String>,
    },
    /// Remove data membership; controller retirement is a separate operation
    Remove { name: String, member: String },
}

#[derive(Args)]
pub struct ReceiveArgs {
    #[arg(long)]
    reveal: bool,
    #[arg(long, requires = "inbox")]
    accept_files: bool,
    #[arg(long, requires = "accept_files")]
    inbox: Option<PathBuf>,
    #[arg(long, default_value_t = kk::config::DEFAULT_FILE_LIMIT)]
    max_bytes: u64,
    #[arg(long, default_value_t = kk::config::NODE_FILE_BUDGET)]
    file_budget: u64,
}
impl ReceiveArgs {
    fn options(self) -> Result<local::Options> {
        Ok(local::Options {
            observer: None,
            bind: None,
            reveal: self.reveal,
            inbox: self.inbox.map(std::path::absolute).transpose()?,
            max_bytes: self.max_bytes,
            file_budget: self.file_budget,
        })
    }
}

async fn approve(fingerprint: String, confirmation: String, confirm: bool) -> Result<bool> {
    writeln!(
        io::stderr(),
        "CONFIRM {confirmation} room-peer=sha256:{fingerprint}"
    )?;
    if confirm {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err(Error::Configuration(
            "room confirmation requires a terminal or explicit --confirm",
        ));
    }
    writeln!(
        io::stderr(),
        "Compare CONFIRM on both devices, then type yes to authorize this room-scoped key."
    )?;
    let answer = kk::input::text(None, false).await?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "yes" | "oui"
    ))
}

fn report(reply: local::Reply) -> Result<()> {
    writeln!(
        io::stdout(),
        "{}",
        serde_json::to_string(&reply).map_err(|_| Error::Protocol)?
    )?;
    reply.check()
}

pub(super) async fn run(command: Command, selected: Option<&Path>) -> Result<()> {
    let profile = super::profile_directory(selected)?;
    match command {
        Command::Files { name, command } => {
            report(super::replica::run(command, &profile, &name).await?)
        }
        Command::Media { name, command } => super::media::room(command, &profile, &name).await,
        Command::Create { name, title, bind } => {
            let id =
                local::create(&profile, &name, title.as_deref().unwrap_or(&name), bind).await?;
            writeln!(io::stdout(), "ROOM CREATED {name} {id}")?;
            Ok(())
        }
        Command::Serve {
            name,
            bind,
            receive,
        } => {
            let mut options = receive.options()?;
            options.bind = bind;
            local::serve(&profile, &name, options).await
        }
        Command::Join {
            name,
            invitation,
            bind,
            role,
            confirm,
            receive,
        } => {
            local::join(
                &profile,
                &name,
                &invitation,
                bind,
                role.rights(),
                receive.options()?,
                move |fingerprint, confirmation| approve(fingerprint, confirmation, confirm),
            )
            .await
        }
        Command::Invite {
            name,
            bind,
            invite_out,
            role,
            confirm,
        } => {
            let options = local::InvitationOptions {
                listen: bind,
                output: std::path::absolute(invite_out)?,
                rights: role.rights(),
            };
            report(
                local::invite(
                    &profile,
                    &name,
                    options,
                    move |fingerprint, confirmation| approve(fingerprint, confirmation, confirm),
                )
                .await?,
            )
        }
        Command::Status { name } => {
            report(local::request(&profile, &name, &local::Command::Status).await?)
        }
        Command::Stop { name } => {
            report(local::request(&profile, &name, &local::Command::Stop).await?)
        }
        Command::Send {
            name,
            payload,
            sensitive,
            file,
            to,
        } => {
            if file {
                let source = payload.ok_or(Error::Configuration("select a file path"))?;
                if source == "-" {
                    return Err(Error::Configuration(
                        "resumable files require a stable path, not stdin",
                    ));
                }
                let target = PeerId::try_from(
                    to.ok_or(Error::Configuration("select the file recipient with --to"))?,
                )
                .map_err(|_| {
                    Error::Configuration("file recipient requires its full certificate digest")
                })?;
                return report(
                    local::request(
                        &profile,
                        &name,
                        &local::Command::File {
                            source: std::path::absolute(source)?,
                            target,
                            sensitive,
                        },
                    )
                    .await?,
                );
            }
            let text = kk::input::text(payload.as_deref(), sensitive).await?;
            report(
                local::request(
                    &profile,
                    &name,
                    &local::Command::Text {
                        text: text.to_string(),
                        sensitive,
                    },
                )
                .await?,
            )
        }
        Command::Controllers { name, voters } => {
            let voters = voters
                .into_iter()
                .map(|voter| {
                    PeerId::try_from(voter).map_err(|_| {
                        Error::Configuration(
                            "controllers require canonical full certificate digests",
                        )
                    })
                })
                .collect::<Result<_>>()?;
            report(local::request(&profile, &name, &local::Command::Controllers { voters }).await?)
        }
        Command::Remove { name, member } => {
            let member = PeerId::try_from(member).map_err(|_| {
                Error::Configuration("member requires its canonical full certificate digest")
            })?;
            report(local::request(&profile, &name, &local::Command::Remove { member }).await?)
        }
    }
}
