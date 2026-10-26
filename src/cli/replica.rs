//! Thin positional adapter for explicit replica availability and verified fetches.
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

pub(super) async fn run(command: Command, profile: &Path, name: &str) -> Result<local::Reply> {
    let request = match command {
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
    };
    local::request(profile, name, &request).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn content_digest_parser_rejects_partial_and_non_ascii_inputs() {
        assert_eq!(kk::files::parse_hash(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        for invalid in ["a".repeat(63), "g".repeat(64), "\u{e9}".repeat(32)] {
            assert!(kk::files::parse_hash(&invalid).is_err());
        }
    }
}
