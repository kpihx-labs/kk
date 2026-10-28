//! Thin local profile/settings adapter; private identity material is never read.
use super::*;
use kk::profile::{Settings, SidebarMode, catalog};
use serde::Serialize;
use std::path::Path;

#[derive(Subcommand)]
pub(super) enum Command {
    /// List registered profiles; no filesystem scan or profile creation
    List,
    /// Show the selected profile's public identity and settings
    Show,
    /// Create a new independent identity, without replacing an existing path
    Create {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    /// Register an existing initialized profile without copying its keys
    Register { id: String, directory: PathBuf },
    /// Choose the profile used by future commands; running sessions stay unchanged
    Select { id: String },
    /// Forget a registration only; all identity and message data is retained
    Unregister { id: String },
    /// Change this profile's human-readable local name
    Rename { name: String },
    /// Set the sidebar layout for this profile
    Sidebar {
        #[arg(value_parser = ["expanded", "compact", "auto"])]
        mode: String,
    },
    /// Explicitly share this display name with room members, or stop sharing it
    ShareName {
        #[arg(long, required_unless_present = "disable", conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    /// Set a local room nickname; it never changes identity or trust
    #[cfg(unix)]
    Label {
        room: String,
        member: String,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// List local room nicknames, not verified remote profile announcements
    #[cfg(unix)]
    Labels { room: String },
    /// Set quantified appearance; clarity 50 is the default neutral dark midpoint
    Appearance {
        #[arg(long, conflicts_with = "neutral", value_parser = clap::value_parser!(u16).range(0..360))]
        hue: Option<u16>,
        #[arg(long)]
        neutral: bool,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        clarity: Option<u8>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        saturation: Option<u8>,
    },
}

fn print(value: &impl Serialize) -> Result<()> {
    writeln!(
        io::stdout(),
        "{}",
        serde_json::to_string_pretty(value).map_err(|_| Error::Protocol)?
    )?;
    Ok(())
}

pub(super) fn run(command: Command, selected: Option<&Path>) -> Result<()> {
    match command {
        Command::List => print(&catalog::list()?),
        Command::Show => {
            let directory = profile_directory(selected)?;
            print(&catalog::summary("current".into(), directory, true)?)
        }
        Command::Create {
            id,
            name,
            directory,
        } => print(&catalog::create(&id, name, directory)?),
        Command::Register { id, directory } => print(&catalog::register(&id, directory)?),
        Command::Select { id } => {
            catalog::select(&id)?;
            print(&serde_json::json!({"selected": id, "running_sessions_changed": false}))
        }
        Command::Unregister { id } => {
            catalog::remove(&id)?;
            print(&serde_json::json!({"unregistered": id, "data_deleted": false}))
        }
        Command::Rename { name } => {
            let directory = profile_directory(selected)?;
            kk::profile::private_directory(&directory, false)?;
            print(&Settings::update(&directory, Some(name), None)?)
        }
        Command::Sidebar { mode } => {
            let directory = profile_directory(selected)?;
            kk::profile::private_directory(&directory, false)?;
            print(&Settings::update_with(&directory, |settings| {
                settings.sidebar_mode = match mode.as_str() {
                    "expanded" => SidebarMode::Expanded,
                    "compact" => SidebarMode::Compact,
                    "auto" => SidebarMode::Auto,
                    _ => return Err(Error::Configuration("invalid sidebar mode")),
                };
                Ok(())
            })?)
        }
        Command::ShareName { enable, disable: _ } => {
            let directory = profile_directory(selected)?;
            kk::profile::private_directory(&directory, false)?;
            print(&Settings::update_with(&directory, |settings| {
                settings.share_display_name = enable;
                Ok(())
            })?)
        }
        #[cfg(unix)]
        Command::Label {
            room,
            member,
            name,
            clear: _,
        } => {
            let directory = profile_directory(selected)?;
            let room = room_id(&directory, &room)?;
            let member = kk::rooms::control::raft::PeerId::try_from(member)
                .map_err(|_| Error::Configuration("member requires its full certificate digest"))?;
            kk::profile::labels::set(&directory, room, member, name)?;
            print(
                &serde_json::json!({"source": "local_override", "labels": kk::profile::labels::list(&directory, room)?}),
            )
        }
        #[cfg(unix)]
        Command::Labels { room } => {
            let directory = profile_directory(selected)?;
            print(&kk::profile::labels::list(
                &directory,
                room_id(&directory, &room)?,
            )?)
        }
        Command::Appearance {
            hue,
            neutral,
            clarity,
            saturation,
        } => {
            let directory = profile_directory(selected)?;
            kk::profile::private_directory(&directory, false)?;
            print(&Settings::update_with(&directory, |settings| {
                if neutral {
                    settings.appearance.hue = None;
                } else if let Some(hue) = hue {
                    settings.appearance.hue = Some(hue);
                }
                if let Some(clarity) = clarity {
                    settings.appearance.clarity = clarity;
                }
                if let Some(saturation) = saturation {
                    settings.appearance.saturation = saturation;
                }
                Ok(())
            })?)
        }
    }
}

#[cfg(unix)]
fn room_id(directory: &Path, name: &str) -> Result<[u8; 32]> {
    let summary = kk::rooms::local::list(directory)?
        .into_iter()
        .find(|room| room.name == name)
        .ok_or(Error::Configuration("unknown local room"))?;
    summary.room.map(|id| id.0).ok_or(Error::Configuration(
        "room metadata is invalid; it was not reset",
    ))
}

#[cfg(unix)]
pub(super) async fn status(directory: &Path, json: bool) -> Result<()> {
    // Public-certificate inspection distinguishes absent identity from damage;
    // neither status nor profile resolution creates directories or keys.
    let certificate = directory.join("identity/certificate.der");
    let (identity_state, fingerprint, identity_error) = match certificate.try_exists()? {
        false => ("not_initialized", None, None),
        true => match Peer::load(&certificate) {
            Ok(peer) => ("ready", Some(peer.fingerprint()), None),
            Err(error) => ("invalid", None, Some(error.to_string())),
        },
    };
    let settings = Settings::load(directory)?;
    let mut node = serde_json::json!({"state": "offline"});
    if directory.try_exists()? {
        kk::profile::private_directory(directory, false)?;
        node = match kk::node::ipc::request(
            directory,
            &kk::node::ipc::Request::Status,
            kk::config::DEFAULT_TIMEOUT,
        )
        .await
        {
            Ok(Some(reply)) => match reply.result() {
                Ok(()) => serde_json::json!({"state": "online", "status": reply.status}),
                Err(error) => {
                    serde_json::json!({"state": "error", "code": error.code(), "error": error.to_string()})
                }
            },
            Ok(None) => serde_json::json!({"state": "offline"}),
            Err(error) => {
                serde_json::json!({"state": "unavailable", "code": error.code(), "error": error.to_string()})
            }
        };
    }
    let mut rooms = Vec::new();
    let mut rooms_error = None;
    let self_peer = fingerprint
        .as_ref()
        .map(|id| kk::rooms::control::raft::PeerId::try_from(id.clone()))
        .transpose()
        .map_err(|_| Error::Protocol)?;
    if identity_state == "ready" {
        match kk::rooms::local::list(directory) {
            Ok(summaries) => {
                // Bound each cached observation independently; this is local
                // inspection, not a quorum query or automatic network startup.
                rooms = futures_util::future::join_all(summaries.into_iter().map(|summary| async move {
                    let observation = if summary.problem.is_none() {
                        match tokio::time::timeout(Duration::from_millis(500), kk::rooms::local::request(directory, &summary.name, &kk::rooms::local::Command::Status)).await {
                            Ok(Ok(reply)) if reply.ok => serde_json::json!({"state": "online", "status": reply.result}),
                            Ok(Err(Error::Node("no active room; run kk room serve"))) => serde_json::json!({"state": "offline"}),
                            Ok(Err(error)) => serde_json::json!({"state": "unavailable", "error": error.to_string()}),
                            Ok(Ok(reply)) => serde_json::json!({"state": "error", "reply": reply}),
                            Err(_) => serde_json::json!({"state": "unavailable", "error": "local status deadline"}),
                        }
                    } else { serde_json::json!({"state": "invalid"}) };
                    let presentation = (|| -> Result<Vec<kk::profile::labels::MemberPresentation>> {
                        let Some(raw) = observation["status"].get("members") else { return Ok(Vec::new()); };
                        let members: Vec<kk::rooms::control::raft::PeerId> = serde_json::from_value(raw.clone()).map_err(|_| Error::Protocol)?;
                        let announcements = observation["status"].get("profile_names").cloned().map(serde_json::from_value).transpose().map_err(|_| Error::Protocol)?.unwrap_or_default();
                        kk::profile::labels::present_with_announcements(directory, summary.room.ok_or(Error::Protocol)?.0,
                            self_peer.ok_or(Error::Protocol)?, &members, &announcements)
                    })();
                    let (presentation, presentation_error) = match presentation {
                        Ok(members) => (members, None), Err(error) => (Vec::new(), Some(error.to_string())),
                    };
                    serde_json::json!({"name": summary.name, "title": summary.title, "id": summary.room, "problem": summary.problem,
                        "observation": observation, "presentation": presentation, "presentation_error": presentation_error})
                })).await;
            }
            Err(error) => rooms_error = Some(error.to_string()),
        }
    }
    let (profiles, profiles_error) = match catalog::list() {
        Ok(profiles) => (Some(profiles), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let value = serde_json::json!({"version": kk::VERSION, "profile": {"directory": directory, "settings": settings,
        "identity": identity_state, "fingerprint": fingerprint, "error": identity_error},
        "profiles": profiles, "profiles_error": profiles_error, "node": node, "rooms": rooms, "rooms_error": rooms_error});
    if json {
        return print(&value);
    }
    writeln!(
        io::stdout(),
        "{}\n  Profile: {}\n  Identity: {}\n  Node: {}\n  Registered profiles: {}",
        settings.display_name,
        directory.display(),
        identity_state,
        value["node"]["state"].as_str().unwrap_or("unknown"),
        value["profiles"]
            .as_array()
            .map(|items| items.len().to_string())
            .unwrap_or_else(|| "unavailable".into())
    )?;
    writeln!(
        io::stdout(),
        "  Display name sharing: {}",
        if settings.share_display_name {
            "enabled for room members; not a verified human identity"
        } else {
            "off; your own name is local-only (previously learned names cannot be erased)"
        }
    )?;
    if let Some(fingerprint) = fingerprint {
        writeln!(io::stdout(), "  Fingerprint: {fingerprint}")?;
    }
    if let Some(error) = identity_error {
        writeln!(io::stdout(), "  Identity problem: {error}")?;
    }
    if let Some(error) = value["node"]["error"].as_str() {
        writeln!(io::stdout(), "  Node problem: {error}")?;
    }
    if let Some(error) = profiles_error {
        writeln!(io::stdout(), "  Profile inventory problem: {error}")?;
    }
    if let Some(status) = value["node"].get("status") {
        print(status)?;
    }
    for room in &rooms {
        writeln!(
            io::stdout(),
            "  Room: {} ({}) / {}",
            room["title"].as_str().unwrap_or("Untitled room"),
            room["name"].as_str().unwrap_or("?"),
            room["observation"]["state"].as_str().unwrap_or("unknown")
        )?;
        if let Some(problem) = room["problem"]
            .as_str()
            .or_else(|| room["observation"]["error"].as_str())
        {
            writeln!(io::stdout(), "    Problem: {problem}")?;
        }
        if let Some(members) = room["presentation"].as_array() {
            for member in members {
                writeln!(
                    io::stdout(),
                    "    {} / {}",
                    member["display_name"].as_str().unwrap_or("Unnamed member"),
                    match member["source"].as_str() {
                        Some("self_announcement") =>
                            "self_announcement (not a verified human identity)",
                        Some(source) => source,
                        None => "unknown",
                    }
                )?;
            }
        }
        if let Some(error) = room["presentation_error"].as_str() {
            writeln!(io::stdout(), "    Names problem: {error}")?;
        }
        if let Some(error) = room["observation"]["status"]["profile_names_error"].as_str() {
            writeln!(io::stdout(), "    Remote names problem: {error}")?;
        }
    }
    if let Some(error) = rooms_error {
        writeln!(io::stdout(), "  Rooms problem: {error}")?;
    }
    Ok(())
}
