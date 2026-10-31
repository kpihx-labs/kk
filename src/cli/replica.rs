//! Thin adapters for owned received copies and explicit replica operations.
use clap::Subcommand;
use kk::files::parse_hash;
use kk::{
    Error, Result,
    rooms::{control::raft::PeerId, local},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Subcommand)]
pub(super) enum Command {
    /// List owned received-copy IDs; metadata does not verify their contents
    Received,
    /// Delete one local saved copy, not remote storage or publication grants
    DeleteCopy {
        #[arg(value_name = "RECEIVED_ID")]
        id: String,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Forward an owned received copy by opaque ID; does not enable reception
    ///
    /// Wait for a verified recipient receipt unless --background is selected.
    ForwardCopy {
        #[arg(value_name = "RECEIVED_ID")]
        id: String,
        #[arg(long, required = true, value_name = "FULL_PEER_ID", value_parser = forward_peer)]
        to: PeerId,
        #[arg(long)]
        sensitive: bool,
        /// Return a foreground-owned job ID; counters are not a verified receipt
        #[arg(long)]
        background: bool,
        /// Explicit retry of a durable pre-handoff failure; never uncertain work
        #[arg(long, requires = "background", value_name = "JOB_ID")]
        retry_of: Option<String>,
    },
    /// Offer this file to an exact audience for this foreground service only
    Share {
        source: PathBuf,
        #[arg(long, required = true, num_args = 1..)]
        to: Vec<String>,
    },
    /// Withdraw an offer, including authorization for active replica transfers
    Unshare { sha256: String },
    /// Ask only these selected peers for a previously shared content digest
    Locate {
        sha256: String,
        #[arg(long, required = true, num_args = 1..)]
        from: Vec<String>,
    },
    /// Fetch and verify a copy; this room must already consent to incoming files
    Fetch {
        sha256: String,
        #[arg(long)]
        from: String,
    },
}

fn peer(value: String) -> Result<PeerId> {
    PeerId::try_from(value).map_err(|_| {
        Error::Configuration("replica peers require their complete certificate digests")
    })
}

fn forward_peer(value: &str) -> std::result::Result<PeerId, String> {
    peer(value.to_owned())
        .map_err(|_| "forwarding requires the complete peer certificate digest".to_owned())
}

fn request(command: Command) -> Result<local::Command> {
    Ok(match command {
        Command::Received => local::Command::ReceivedFiles,
        Command::DeleteCopy { id, confirm } => local::Command::DeleteReceived { id, confirm },
        Command::ForwardCopy {
            id,
            to,
            sensitive,
            background,
            retry_of,
        } => {
            if let Some(previous) = retry_of {
                if !background {
                    return Err(Error::Configuration("--retry-of requires --background"));
                }
                local::Command::ForwardReceivedRetryStart {
                    previous,
                    id,
                    target: to,
                    sensitive,
                }
            } else if background {
                local::Command::ForwardReceivedStart {
                    id,
                    target: to,
                    sensitive,
                }
            } else {
                local::Command::ForwardReceived {
                    id,
                    target: to,
                    sensitive,
                }
            }
        }
        Command::Share { source, to } => local::Command::FileShare {
            source: std::path::absolute(source)?,
            audience: to.into_iter().map(peer).collect::<Result<BTreeSet<_>>>()?,
        },
        Command::Unshare { sha256 } => local::Command::FileUnshare {
            hash: parse_hash(&sha256)?,
        },
        Command::Locate { sha256, from } => local::Command::FileLocate {
            hash: parse_hash(&sha256)?,
            peers: from
                .into_iter()
                .map(peer)
                .collect::<Result<BTreeSet<_>>>()?,
        },
        Command::Fetch { sha256, from } => local::Command::FileFetch {
            hash: parse_hash(&sha256)?,
            peer: peer(from)?,
        },
    })
}

pub(super) async fn run(command: Command, profile: &Path, name: &str) -> Result<local::Reply> {
    let request = request(command)?;
    local::request(profile, name, &request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Files {
        #[command(subcommand)]
        command: Command,
    }

    fn received_id() -> String {
        format!("{}-{}.bin", "17".repeat(32), "cd".repeat(32))
    }

    #[test]
    fn forward_copy_routes_sync_by_default_and_background_only_on_explicit_request() {
        let id = received_id();
        let target = "ab".repeat(32);
        let expected = PeerId::try_from(target.clone()).unwrap();
        for background in [false, true] {
            for sensitive in [false, true] {
                let mut arguments = vec!["files", "forward-copy", &id, "--to", &target];
                if background {
                    arguments.push("--background");
                }
                if sensitive {
                    arguments.push("--sensitive");
                }
                let parsed = Files::try_parse_from(arguments).unwrap();
                let request = request(parsed.command).unwrap();
                match &request {
                    local::Command::ForwardReceived {
                        id: selected,
                        target,
                        sensitive: hidden,
                    } => {
                        assert!(!background);
                        assert_eq!(selected, &id);
                        assert_eq!(*target, expected);
                        assert_eq!(*hidden, sensitive);
                    }
                    local::Command::ForwardReceivedStart {
                        id: selected,
                        target,
                        sensitive: hidden,
                    } => {
                        assert!(background);
                        assert_eq!(selected, &id);
                        assert_eq!(*target, expected);
                        assert_eq!(*hidden, sensitive);
                    }
                    _ => panic!("forward-copy must only use the owned-received IPC commands"),
                }
                // No path, source hash or fabricated provenance crosses this
                // adapter. The room owner validates the opaque received ID.
                let value = serde_json::to_value(&request).unwrap();
                assert_eq!(value["id"], id);
                assert_eq!(value["target"], serde_json::to_value(expected).unwrap());
                assert_eq!(value["sensitive"], sensitive);
                assert_eq!(
                    value["operation"],
                    if background {
                        "ForwardReceivedStart"
                    } else {
                        "ForwardReceived"
                    }
                );
                assert_eq!(value.as_object().unwrap().len(), 4);
                for forbidden in ["source", "inbox", "hash", "sha256", "origin"] {
                    assert!(value.get(forbidden).is_none());
                }
            }
        }
    }

    #[test]
    fn forward_copy_requires_an_id_and_one_complete_explicit_target() {
        let id = received_id();
        let target = "ab".repeat(32);
        assert!(Files::try_parse_from(["files", "forward-copy", &id]).is_err());
        assert!(Files::try_parse_from(["files", "forward-copy", "--to", &target]).is_err());
        assert!(
            Files::try_parse_from([
                "files",
                "forward-copy",
                &id,
                "--to",
                &target,
                "--to",
                &target
            ])
            .is_err()
        );
        for invalid in [
            "alice".to_owned(),
            "ab".repeat(8),
            "g".repeat(64),
            "é".repeat(32),
        ] {
            let error = Files::try_parse_from(["files", "forward-copy", &id, "--to", &invalid])
                .err()
                .unwrap();
            assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn forward_copy_has_no_receive_grant_or_origin_override_options() {
        let id = received_id();
        let target = "ab".repeat(32);
        for forbidden in ["--inbox", "--accept-files", "--sha256", "--origin-hash"] {
            let error =
                Files::try_parse_from(["files", "forward-copy", &id, "--to", &target, forbidden])
                    .err()
                    .unwrap();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn forward_copy_help_does_not_present_background_counters_as_a_receipt() {
        let help = Files::try_parse_from(["files", "forward-copy", "--help"])
            .err()
            .unwrap();
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
        let text = help.to_string();
        for expected in [
            "RECEIVED_ID",
            "--to",
            "--sensitive",
            "--background",
            "counters",
            "receipt",
        ] {
            assert!(text.contains(expected), "help omits {expected}");
        }
    }

    #[test]
    fn existing_replica_subcommands_keep_their_typed_ipc_operations() {
        let target = "ab".repeat(32);
        let hash = "cd".repeat(32);
        for (arguments, operation) in [
            (
                vec!["files", "share", "selected-copy", "--to", &target],
                "FileShare",
            ),
            (vec!["files", "unshare", &hash], "FileUnshare"),
            (
                vec!["files", "locate", &hash, "--from", &target],
                "FileLocate",
            ),
            (
                vec!["files", "fetch", &hash, "--from", &target],
                "FileFetch",
            ),
        ] {
            let parsed = Files::try_parse_from(arguments).unwrap();
            let value = serde_json::to_value(request(parsed.command).unwrap()).unwrap();
            assert_eq!(value["operation"], operation);
        }
    }

    #[test]
    fn content_digest_parser_rejects_partial_and_non_ascii_inputs() {
        assert_eq!(kk::files::parse_hash(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        for invalid in ["a".repeat(63), "g".repeat(64), "\u{e9}".repeat(32)] {
            assert!(kk::files::parse_hash(&invalid).is_err());
        }
    }
}
