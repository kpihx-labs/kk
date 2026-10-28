//! Path-free history CLI; all authorization and storage work stays in the actor.
use clap::{Args, Subcommand};
use kk::{
    Error, Result,
    rooms::{
        local,
        runtime::history::{Policy, Search},
    },
};
use std::path::Path;

#[derive(Args)]
pub struct Limits {
    #[arg(long, default_value_t = Policy::default().rows)]
    rows: usize,
    #[arg(long, default_value_t = Policy::default().encrypted_bytes)]
    encrypted_bytes: usize,
    #[arg(long, default_value_t = Policy::default().retention_seconds)]
    retention_seconds: u64,
}
impl Limits {
    fn policy(self) -> Result<Policy> {
        let policy = Policy {
            rows: self.rows,
            encrypted_bytes: self.encrypted_bytes,
            retention_seconds: self.retention_seconds,
        };
        policy.validate()?;
        Ok(policy)
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Explicitly enroll a key and capture future non-sensitive messages only
    Enable {
        #[command(flatten)]
        limits: Limits,
    },
    /// Load an already enrolled key; never generate or adopt a key
    Open,
    /// Stop future capture without deleting retained rows or the key
    Disable,
    /// Public cached status; --refresh requests a quorum-authorized fresh view
    Status {
        #[arg(long)]
        refresh: bool,
    },
    /// Literal substring search; page using next_after from the preceding reply
    Search {
        #[arg(default_value = "")]
        text: String,
        #[arg(long, default_value_t = 0)]
        after_id: u64,
        #[arg(long, default_value_t = kk::rooms::store::history::MAX_RESULTS)]
        limit: usize,
        #[arg(long, default_value_t = kk::rooms::store::history::MAX_RESPONSE_BYTES)]
        response_bytes: usize,
    },
    /// Irreversibly clear retained history rows, not the enrolled key
    Clear {
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Explicitly enroll an orphan key matching this PUBLIC age recipient
    Recover {
        #[arg(long)]
        expected_recipient: String,
        #[arg(long, required = true)]
        confirm: bool,
        #[command(flatten)]
        limits: Limits,
    },
}

pub async fn run(command: Command, profile: &Path, name: &str) -> Result<local::Reply> {
    let command = match command {
        Command::Enable { limits } => local::Command::HistoryEnable {
            policy: limits.policy()?,
        },
        Command::Open => local::Command::HistoryOpen {},
        Command::Disable => local::Command::HistoryDisable {},
        Command::Status { refresh: false } => local::Command::HistoryStatus {},
        Command::Status { refresh: true } => local::Command::HistoryRefresh {},
        Command::Clear { confirm } => {
            if !confirm {
                return Err(Error::Configuration("history clear requires --confirm"));
            }
            local::Command::HistoryClear {}
        }
        Command::Recover {
            expected_recipient,
            confirm,
            limits,
        } => {
            if !confirm {
                return Err(Error::Configuration("history recovery requires --confirm"));
            }
            if expected_recipient.len() > 128
                || expected_recipient
                    .parse::<age::x25519::Recipient>()
                    .is_err()
            {
                return Err(Error::Configuration(
                    "expected recipient must be a public age recipient",
                ));
            }
            local::Command::HistoryRecover {
                expected_recipient,
                policy: limits.policy()?,
            }
        }
        Command::Search {
            text,
            after_id,
            limit,
            response_bytes,
        } => {
            let query = Search {
                text,
                after_id,
                limit,
                response_bytes,
            };
            query.validate()?;
            local::Command::HistorySearch {
                text: query.text.clone(),
                after_id,
                limit,
                response_bytes,
            }
        }
    };
    local::request(profile, name, &command).await
}
