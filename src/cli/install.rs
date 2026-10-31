//! Wiring: `mod install;` then `Install(install::InstallArgs)` and
//! `Rollback(install::RollbackArgs)` top-level variants; dispatch before profiles.
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub(super) struct InstallArgs {
    #[arg(long)]
    pub version: String,
    #[arg(long)]
    pub commit: String,
    #[arg(long)]
    pub prefix: PathBuf,
    /// Local TAR gzip/ZIP input; still requires GitHub provenance verification
    #[arg(long)]
    pub archive: Option<PathBuf>,
    /// Exported gh attestation bundle (not an archive-integrity bypass)
    #[arg(long, requires = "archive")]
    pub bundle: Option<PathBuf>,
    /// Provisioned public Sigstore roots for verification without online root fetching
    #[arg(long, requires = "bundle")]
    pub trusted_root: Option<PathBuf>,
    /// Authenticate and inspect without activating or changing the prefix
    #[arg(long)]
    pub verify_only: bool,
}

#[derive(Debug, Args)]
pub(super) struct RollbackArgs {
    #[arg(long)]
    pub prefix: PathBuf,
    #[arg(long)]
    pub version: String,
    /// Optional exported gh attestation bundle for the retained original archive
    #[arg(long)]
    pub bundle: Option<PathBuf>,
    /// Provisioned public Sigstore roots; never automatically downloaded
    #[arg(long, requires = "bundle")]
    pub trusted_root: Option<PathBuf>,
}

pub(super) async fn install(args: InstallArgs) -> kk::Result<()> {
    let request = kk::install::InstallRequest {
        version: args.version,
        commit: args.commit,
        prefix: args.prefix,
        archive: args.archive,
        bundle: args.bundle,
        trusted_root: args.trusted_root,
    };
    if args.verify_only {
        let verification = kk::install::verify(request)
            .await
            .map_err(|error| crate::Error::Io(std::io::Error::other(error)))?;
        print(&verification).await
    } else {
        let outcome = kk::install::install(request)
            .await
            .map_err(|error| crate::Error::Io(std::io::Error::other(error)))?;
        print(&outcome).await
    }
}

pub(super) async fn rollback(args: RollbackArgs) -> kk::Result<()> {
    let outcome =
        kk::install::rollback_with_trust(args.prefix, args.version, args.bundle, args.trusted_root)
            .await
            .map_err(|error| crate::Error::Io(std::io::Error::other(error)))?;
    print(&outcome).await
}

async fn print(outcome: &impl serde::Serialize) -> kk::Result<()> {
    kk::output::line(serde_json::to_string(outcome).map_err(|_| crate::Error::Protocol)?).await
}
