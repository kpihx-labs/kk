use clap::{Args, Subcommand, ValueEnum};
mod history;
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
    /// Opt-in encrypted future-only text history owned by the foreground room
    History {
        name: String,
        #[command(subcommand)]
        command: history::Command,
    },
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
    /// List bounded outgoing jobs owned by this running foreground room
    Transfers { name: String },
    /// Inspect one exact foreground-owned file job; counters are not receipts
    Transfer { name: String, id: String },
    /// Explicit offline public-log repair for pending enrollment only; never repairs keys
    RecoverControl {
        name: String,
        /// Applied-record hash obtained independently from the verified authority
        #[arg(long, value_parser = parse_pending_id)]
        expected: [u8; 32],
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Request local cancellation, not deletion or rollback of remote storage
    CancelTransfer {
        name: String,
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Inspect safe metadata for this device's exact prepared membership change
    Pending { name: String },
    /// Explicitly retry the inspected prepared change, not an invitation or payload
    RetryPrepared {
        name: String,
        #[arg(long, value_parser = parse_pending_id)]
        expected: [u8; 32],
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Change incoming-file consent on the running room without restarting it
    Receiving {
        name: String,
        #[command(flatten)]
        receive: ReceiveArgs,
        #[arg(long, conflicts_with = "accept_files")]
        disable: bool,
    },
    /// Gracefully stop the foreground room without deleting any state
    Stop { name: String },
    /// Voluntarily remove this profile's own room membership
    Leave {
        name: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
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
        /// Admit a file job without waiting for its receipt; use transfers/transfer
        /// Requires the existing foreground room, never creates a daemon
        #[arg(long, requires = "file")]
        background: bool,
    },
    /// Reply to an authenticated message ID; sensitive input uses stdin, never argv
    Reply {
        name: String,
        #[arg(long)]
        to: kk::rooms::conversation::Target,
        payload: Option<String>,
        #[arg(short = 's', long)]
        sensitive: bool,
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

/// Parse the canonical operation ID shown by `room pending`, without side effects.
///
/// `value` is the lowercase hexadecimal pending-operation digest. Returns its
/// exact bytes or an argument error before profile lookup or network effects.
/// Examples: 32 repetitions of `ab` produce `[0xab; 32]`; uppercase `AB` or a
/// shortened ID returns an error rather than choosing another operation.
fn parse_pending_id(value: &str) -> std::result::Result<[u8; 32], String> {
    let mut id = [0; 32];
    let error = || "expected the full lowercase operation ID from 'room pending'".to_owned();
    if value.len() != id.len() * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(error());
    }
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| error())?;
    }
    Ok(id)
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
        Command::History { name, command } => report(history::run(command, &profile, &name).await?),
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
        Command::Transfers { name } => {
            report(local::request(&profile, &name, &local::Command::FileTransfers).await?)
        }
        Command::Transfer { name, id } => report(
            local::request(&profile, &name, &local::Command::FileTransferStatus { id }).await?,
        ),
        Command::RecoverControl {
            name,
            expected,
            confirm,
        } => {
            if !confirm {
                return Err(Error::Configuration(
                    "explicit recovery confirmation is required",
                ));
            }
            local::recover_pending_control(&profile, &name, expected).await?;
            println!(
                "{}",
                serde_json::json!({"ok":true,"meaning":"public-control-replayed-not-private-join-authority","next":"explicitly resume the existing pending room"})
            );
            Ok(())
        }
        Command::CancelTransfer { name, id, confirm } => {
            if !confirm {
                return Err(Error::Configuration(
                    "explicit cancellation confirmation is required",
                ));
            }
            report(
                local::request(&profile, &name, &local::Command::FileTransferCancel { id }).await?,
            )
        }
        Command::Pending { name } => {
            report(local::request(&profile, &name, &local::Command::PendingMutation).await?)
        }
        Command::RetryPrepared {
            name,
            expected,
            confirm,
        } => report(
            local::request(
                &profile,
                &name,
                &local::Command::RetryPrepared { expected, confirm },
            )
            .await?,
        ),
        Command::Receiving {
            name,
            receive,
            disable,
        } => {
            if !receive.accept_files && !disable {
                return Err(Error::Configuration(
                    "choose --accept-files with --inbox, or --disable",
                ));
            }
            let options = receive.options()?;
            report(
                local::request(
                    &profile,
                    &name,
                    &local::Command::FileReception {
                        inbox: options.inbox,
                        max_bytes: options.max_bytes,
                        budget: options.file_budget,
                    },
                )
                .await?,
            )
        }
        Command::Stop { name } => {
            report(local::request(&profile, &name, &local::Command::Stop).await?)
        }
        Command::Leave { name, confirm } => {
            report(local::request(&profile, &name, &local::Command::LeaveSelf { confirm }).await?)
        }
        Command::Send {
            name,
            payload,
            sensitive,
            file,
            to,
            background,
        } => {
            if background && !file {
                return Err(Error::Configuration(
                    "background admission is only available for files",
                ));
            }
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
                let source = std::path::absolute(source)?;
                let command = if background {
                    local::Command::FileStart {
                        source,
                        target,
                        sensitive,
                    }
                } else {
                    local::Command::File {
                        source,
                        target,
                        sensitive,
                    }
                };
                return report(local::request(&profile, &name, &command).await?);
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
        Command::Reply {
            name,
            to,
            payload,
            sensitive,
        } => {
            let text = kk::input::text(payload.as_deref(), sensitive).await?;
            report(
                local::request(
                    &profile,
                    &name,
                    &local::Command::Reply {
                        target: to,
                        text: text.to_string(),
                        sensitive,
                    },
                )
                .await?,
            )
        }
        Command::Remove { name, member } => {
            let member = PeerId::try_from(member).map_err(|_| {
                Error::Configuration("member requires its canonical full certificate digest")
            })?;
            report(local::request(&profile, &name, &local::Command::Remove { member }).await?)
        }
    }
}

#[cfg(test)]
mod pending_id_tests {
    use super::{Command, parse_pending_id};
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        command: Command,
    }

    #[test]
    fn pending_ids_are_exact_canonical_bytes_not_authorization() {
        assert_eq!(parse_pending_id(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        for value in [
            "ab".repeat(31),
            "AB".repeat(32),
            "é".repeat(32),
            "gg".repeat(32),
        ] {
            assert!(parse_pending_id(&value).is_err());
        }
    }

    #[test]
    fn retry_requires_both_the_exact_id_and_explicit_confirmation() {
        let id = "ab".repeat(32);
        assert!(
            Probe::try_parse_from(["room", "retry-prepared", "study", "--expected", &id]).is_err()
        );
        let parsed = Probe::try_parse_from([
            "room",
            "retry-prepared",
            "study",
            "--expected",
            &id,
            "--confirm",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Command::RetryPrepared { expected, confirm: true, .. } if expected == [0xab; 32])
        );
        assert!(Probe::try_parse_from(["room", "react", "study"]).is_err());
    }

    #[test]
    fn file_job_admission_is_opt_in_and_text_cannot_be_backgrounded() {
        assert!(Probe::try_parse_from(["room", "send", "study", "text", "--background"]).is_err());
        let peer = "a".repeat(64);
        let plain =
            Probe::try_parse_from(["room", "send", "study", "source", "--file", "--to", &peer])
                .unwrap();
        assert!(matches!(
            plain.command,
            Command::Send {
                file: true,
                background: false,
                ..
            }
        ));
        let asynchronous = Probe::try_parse_from([
            "room",
            "send",
            "study",
            "source",
            "--file",
            "--to",
            &peer,
            "--background",
        ])
        .unwrap();
        assert!(matches!(
            asynchronous.command,
            Command::Send {
                file: true,
                background: true,
                ..
            }
        ));
    }

    #[test]
    fn transfer_inspection_and_explicit_cancellation_keep_owner_ids_opaque() {
        assert!(matches!(
            Probe::try_parse_from(["room", "transfers", "study"])
                .unwrap()
                .command,
            Command::Transfers { .. }
        ));
        let parsed =
            Probe::try_parse_from(["room", "transfer", "study", "owner-opaque-id"]).unwrap();
        assert!(matches!(parsed.command, Command::Transfer { id, .. } if id == "owner-opaque-id"));
        assert!(
            Probe::try_parse_from(["room", "cancel-transfer", "study", "owner-opaque-id"]).is_err()
        );
        assert!(matches!(
            Probe::try_parse_from([
                "room",
                "cancel-transfer",
                "study",
                "owner-opaque-id",
                "--confirm"
            ])
            .unwrap()
            .command,
            Command::CancelTransfer { confirm: true, .. }
        ));
    }
}
