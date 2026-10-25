//! Local trust and authorization. Discovery never writes this registry.

use crate::{Error, Identity, Peer, Result, profile};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
};

const MAX_REGISTRY: u64 = 1_048_576;
const MAX_CONTACTS: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    pub text: bool,
    pub files: bool,
    pub media: bool,
    pub rooms: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Text,
    Files,
    Media,
    Rooms,
}
impl Permissions {
    pub fn allows(self, action: Action) -> bool {
        match action {
            Action::Text => self.text,
            Action::Files => self.files,
            Action::Media => self.media,
            Action::Rooms => self.rooms,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub certificate: Vec<u8>,
    pub local_identity: String,
    pub incoming: Permissions,
    pub outgoing: Permissions,
    pub endpoints: Vec<SocketAddr>,
}
impl Contact {
    pub fn peer(&self) -> Result<Peer> {
        Peer::from_der(self.certificate.clone())
    }
    pub fn validate(&self) -> Result<()> {
        self.peer()?;
        if self.local_identity.len() != 64
            || !self
                .local_identity
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.endpoints.len() > 8
            || self.endpoints.iter().any(|address| {
                address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast()
            })
        {
            return Err(Error::Configuration("invalid contact metadata"));
        }
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    revision: u64,
    contacts: BTreeMap<String, Contact>,
    revoked: BTreeSet<String>,
}

pub struct Registry {
    directory: PathBuf,
}
impl Registry {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn open(directory: PathBuf) -> Result<Self> {
        profile::private_directory(&directory, true)?;
        Ok(Self { directory })
    }
    pub fn resolve(
        &self,
        alias: &str,
        local: &Identity,
        action: Action,
        inbound: bool,
    ) -> Result<Contact> {
        valid_alias(alias)?;
        let state = self.read()?;
        let contact = state
            .contacts
            .get(alias)
            .ok_or(Error::Configuration("unknown contact"))?;
        if state.revoked.contains(&contact.peer()?.fingerprint()) {
            return Err(Error::Revoked);
        }
        if contact.local_identity != local.peer().fingerprint() {
            return Err(Error::Identity("contact belongs to another local identity"));
        }
        let permissions = if inbound {
            contact.incoming
        } else {
            contact.outgoing
        };
        if !permissions.allows(action) {
            return Err(Error::Denied);
        }
        Ok(contact.clone())
    }
    pub fn list(&self) -> Result<Vec<(String, String, bool)>> {
        let state = self.read()?;
        state
            .contacts
            .iter()
            .map(|(alias, contact)| {
                let fingerprint = contact.peer()?.fingerprint();
                let revoked = state.revoked.contains(&fingerprint);
                Ok((alias.clone(), fingerprint, revoked))
            })
            .collect()
    }
    pub fn active(&self, local: &Identity) -> Result<Vec<(String, Contact)>> {
        let state = self.read()?;
        let fingerprint = local.peer().fingerprint();
        state
            .contacts
            .into_iter()
            .filter_map(|(alias, contact)| {
                if contact.local_identity != fingerprint {
                    return None;
                }
                match contact.peer() {
                    Ok(peer) if !state.revoked.contains(&peer.fingerprint()) => {
                        Some(Ok((alias, contact)))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect()
    }
    pub fn find_peer(&self, peer: &Peer, local: &Identity) -> Result<(String, Contact)> {
        let state = self.read()?;
        let fingerprint = peer.fingerprint();
        if state.revoked.contains(&fingerprint) {
            return Err(Error::Revoked);
        }
        for (alias, contact) in state.contacts {
            if contact.peer()?.fingerprint() == fingerprint {
                if contact.local_identity != local.peer().fingerprint() {
                    return Err(Error::Identity("contact belongs to another local identity"));
                }
                return Ok((alias, contact));
            }
        }
        Err(Error::Authentication)
    }
    pub fn get(&self, alias: &str) -> Result<Option<Contact>> {
        valid_alias(alias)?;
        Ok(self.read()?.contacts.get(alias).cloned())
    }
    pub fn is_revoked(&self, fingerprint: &str) -> Result<bool> {
        Ok(self.read()?.revoked.contains(fingerprint))
    }
    pub fn authorize_peer(
        &self,
        peer: &Peer,
        local: &Identity,
        action: Action,
        inbound: bool,
    ) -> Result<()> {
        let state = self.read()?;
        let fingerprint = peer.fingerprint();
        if state.revoked.contains(&fingerprint) {
            return Err(Error::Revoked);
        }
        for contact in state.contacts.values() {
            if contact.peer()?.fingerprint() == fingerprint {
                if contact.local_identity != local.peer().fingerprint() {
                    return Err(Error::Identity("contact belongs to another local identity"));
                }
                let permissions = if inbound {
                    contact.incoming
                } else {
                    contact.outgoing
                };
                if !permissions.allows(action) {
                    return Err(Error::Denied);
                }
                return Ok(());
            }
        }
        // A manually selected certificate is explicit authority for this operation.
        // It cannot bypass a known contact's restrictions or fingerprint revocation.
        Ok(())
    }
    pub fn revision(&self) -> Result<u64> {
        Ok(self.read()?.revision)
    }
    pub fn put(&self, alias: &str, contact: Contact) -> Result<()> {
        self.put_checked(alias, contact, None)
    }
    pub fn put_if_revision(&self, alias: &str, contact: Contact, expected: u64) -> Result<()> {
        self.put_checked(alias, contact, Some(expected))
    }
    fn put_checked(&self, alias: &str, contact: Contact, expected: Option<u64>) -> Result<()> {
        valid_alias(alias)?;
        contact.validate()?;
        self.update(|state| {
            if expected.is_some_and(|expected| expected != state.revision) {
                return Err(Error::Configuration(
                    "trust policy changed during pairing; retry with fresh confirmation",
                ));
            }
            let fingerprint = contact.peer()?.fingerprint();
            for (existing_alias, existing) in &state.contacts {
                if existing_alias != alias && existing.peer()?.fingerprint() == fingerprint {
                    return Err(Error::Configuration("peer already has another alias"));
                }
            }
            if state.revoked.contains(&fingerprint) {
                return Err(Error::Revoked);
            }
            if let Some(old) = state.contacts.get(alias) {
                if old.peer()?.fingerprint() != fingerprint
                    || old.local_identity != contact.local_identity
                {
                    return Err(Error::Identity(
                        "alias already has a different identity; no silent replacement",
                    ));
                }
            } else if state.contacts.len() == MAX_CONTACTS {
                return Err(Error::Configuration("contact limit reached"));
            }
            state.contacts.insert(alias.to_owned(), contact);
            Ok(())
        })
    }
    pub fn revoke(&self, alias: &str) -> Result<()> {
        valid_alias(alias)?;
        self.update(|state| {
            let contact = state
                .contacts
                .get(alias)
                .ok_or(Error::Configuration("unknown contact"))?;
            state.revoked.insert(contact.peer()?.fingerprint());
            Ok(())
        })
    }
    pub fn restore(&self, alias: &str) -> Result<()> {
        valid_alias(alias)?;
        self.update(|state| {
            let contact = state
                .contacts
                .get(alias)
                .ok_or(Error::Configuration("unknown contact"))?;
            state.revoked.remove(&contact.peer()?.fingerprint());
            Ok(())
        })
    }
    pub fn set_incoming(&self, alias: &str, permissions: Permissions) -> Result<()> {
        valid_alias(alias)?;
        self.update(|state| {
            state
                .contacts
                .get_mut(alias)
                .ok_or(Error::Configuration("unknown contact"))?
                .incoming = permissions;
            Ok(())
        })
    }
    fn read(&self) -> Result<State> {
        let path = self.directory.join("contacts.json");
        let Some(bytes) = read_private(&path)? else {
            if self.directory.join("contact-history").exists() {
                return Err(Error::Identity("registry is missing but history exists"));
            }
            return Ok(State::default());
        };
        let state: State = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Identity("invalid contacts registry"))?;
        if state.contacts.len() > MAX_CONTACTS || state.revoked.len() > 4096 {
            return Err(Error::Configuration("registry limits exceeded"));
        }
        for (alias, contact) in &state.contacts {
            valid_alias(alias)?;
            contact.validate()?;
        }
        Ok(state)
    }
    #[cfg(unix)]
    fn update(&self, change: impl FnOnce(&mut State) -> Result<()>) -> Result<()> {
        use rustix::fs::{self, Mode, OFlags};
        let lock = File::from(fs::open(
            self.directory.join("contacts.lock"),
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        owned_private(&lock)?;
        fs::flock(&lock, fs::FlockOperation::LockExclusive)?;
        let mut state = self.read()?;
        let previous = serde_json::to_vec(&state)
            .map_err(|_| Error::Configuration("cannot encode registry"))?;
        change(&mut state)?;
        let old_revision = state.revision;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(Error::Configuration("registry revision exhausted"))?;
        let bytes = serde_json::to_vec(&state)
            .map_err(|_| Error::Configuration("cannot encode registry"))?;
        if bytes.len() as u64 > MAX_REGISTRY {
            return Err(Error::Configuration("registry size limit reached"));
        }
        let history = self.directory.join("contact-history");
        profile::private_directory(&history, true)?;
        let archive = history.join(format!("revision-{old_revision}.json"));
        if let Some(existing) = read_private(&archive)? {
            if existing != previous {
                return Err(Error::Identity("registry history conflict"));
            }
        } else {
            write_new(&archive, &previous)?;
        }
        File::open(&history)?.sync_all()?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.directory.join("contacts.json"))
            .map_err(|_| Error::Destination("cannot publish contacts registry"))?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    fn update(&self, _change: impl FnOnce(&mut State) -> Result<()>) -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }
}

fn valid_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.len() > 63
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        || alias == "."
        || alias == ".."
    {
        return Err(Error::Configuration("invalid contact alias"));
    }
    Ok(())
}
#[cfg(unix)]
fn owned_private(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(Error::Identity(
            "registry files must be private, owned regular files",
        ));
    }
    Ok(())
}
#[cfg(unix)]
fn read_private(path: &Path) -> Result<Option<Vec<u8>>> {
    use rustix::{
        fs::{self, Mode, OFlags},
        io::Errno,
    };
    let mut file = match fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    owned_private(&file)?;
    if file.metadata()?.len() > MAX_REGISTRY {
        return Err(Error::Identity("registry file too large"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_REGISTRY + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REGISTRY {
        return Err(Error::Identity("registry file grew beyond limit"));
    }
    Ok(Some(bytes))
}
#[cfg(not(unix))]
fn read_private(_path: &Path) -> Result<Option<Vec<u8>>> {
    Err(Error::UnsupportedPlatform)
}
#[cfg(unix)]
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn permissions_changed_keys_and_revocation_are_enforced_and_history_is_retained() {
        let root = tempfile::tempdir().unwrap();
        let registry = Registry::open(root.path().join("profile")).unwrap();
        let local = Identity::create(&root.path().join("local")).unwrap();
        let remote = Identity::create(&root.path().join("remote")).unwrap();
        let contact = Contact {
            certificate: remote.peer().certificate.to_vec(),
            local_identity: local.peer().fingerprint(),
            incoming: Permissions {
                text: true,
                ..Default::default()
            },
            outgoing: Permissions {
                text: true,
                ..Default::default()
            },
            endpoints: vec![],
        };
        registry.put("friend", contact.clone()).unwrap();
        let replacement = Identity::create(&root.path().join("replacement")).unwrap();
        let mut substitute = contact;
        substitute.certificate = replacement.peer().certificate.to_vec();
        assert!(matches!(
            registry.put("friend", substitute),
            Err(Error::Identity(_))
        ));
        registry
            .resolve("friend", &local, Action::Text, true)
            .unwrap();
        assert!(matches!(
            registry.resolve("friend", &local, Action::Files, true),
            Err(Error::Denied)
        ));
        let stale = registry.get("friend").unwrap().unwrap();
        let revision = registry.revision().unwrap();
        registry
            .set_incoming("friend", Permissions::default())
            .unwrap();
        assert!(matches!(
            registry.put_if_revision("friend", stale, revision),
            Err(Error::Configuration(_))
        ));
        assert!(
            registry
                .resolve("friend", &local, Action::Text, true)
                .is_err()
        );
        registry.revoke("friend").unwrap();
        assert!(matches!(
            registry.resolve("friend", &local, Action::Text, false),
            Err(Error::Revoked)
        ));
        assert!(registry.list().unwrap()[0].2);
        assert!(
            registry
                .directory
                .join("contact-history/revision-0.json")
                .exists()
        );
        assert!(
            registry
                .directory
                .join("contact-history/revision-2.json")
                .exists()
        );
    }
}
