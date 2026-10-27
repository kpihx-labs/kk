//! Explicit profile discovery: no home-directory scan or private-key inspection.
use super::Settings;
use crate::{Error, Peer, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub directory: PathBuf,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    active: Option<String>,
    entries: Vec<Entry>,
}
#[derive(Serialize)]
pub struct Summary {
    pub id: String,
    pub directory: PathBuf,
    pub selected: bool,
    pub initialized: bool,
    pub fingerprint: Option<String>,
    pub settings: Settings,
}

fn valid(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 40
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || id == "default"
    {
        return Err(Error::Configuration(
            "profile ID needs 1-40 lowercase letters/digits/hyphens and cannot be default",
        ));
    }
    Ok(())
}
fn load(base: &Path) -> Result<Catalog> {
    let catalog: Catalog = super::storage::read(base, "profiles.json")?;
    validate(&catalog)?;
    Ok(catalog)
}

fn validate(catalog: &Catalog) -> Result<()> {
    if catalog.entries.len() > 32 {
        return Err(Error::Configuration("too many local profiles"));
    }
    for (index, entry) in catalog.entries.iter().enumerate() {
        valid(&entry.id)?;
        if !entry.directory.is_absolute()
            || catalog.entries[..index]
                .iter()
                .any(|e| e.id == entry.id || e.directory == entry.directory)
        {
            return Err(Error::Configuration(
                "profile catalog contains duplicate or unsafe entries",
            ));
        }
    }
    if catalog
        .active
        .as_ref()
        .is_some_and(|id| !catalog.entries.iter().any(|e| &e.id == id))
    {
        return Err(Error::Configuration(
            "selected profile is missing; catalog was not reset",
        ));
    }
    Ok(())
}
pub fn active_directory() -> Result<PathBuf> {
    let base = super::base_directory()?;
    let catalog = load(&base)?;
    Ok(match catalog.active {
        Some(id) => {
            catalog
                .entries
                .into_iter()
                .find(|e| e.id == id)
                .ok_or(Error::Configuration("selected profile is missing"))?
                .directory
        }
        None => base,
    })
}
pub fn resolve(id: &str) -> Result<PathBuf> {
    let base = super::base_directory()?;
    if id == "default" {
        return Ok(base);
    }
    load(&base)?
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.directory)
        .ok_or(Error::Configuration("unknown local profile"))
}
pub fn summary(id: String, directory: PathBuf, selected: bool) -> Result<Summary> {
    let settings = Settings::load(&directory)?;
    let identity = directory.join("identity");
    let fingerprint = match std::fs::symlink_metadata(&identity) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
        Ok(_) => {
            // An existing identity with a missing or corrupt public certificate
            // is broken, not uninitialized. Never inspect or regenerate its key.
            super::private_directory(&identity, false)?;
            Some(Peer::load(&identity.join("certificate.der"))?.fingerprint())
        }
    };
    Ok(Summary {
        id,
        directory,
        selected,
        initialized: fingerprint.is_some(),
        fingerprint,
        settings,
    })
}
pub fn list() -> Result<Vec<Summary>> {
    let base = super::base_directory()?;
    let catalog = load(&base)?;
    let mut profiles = vec![summary("default".into(), base, catalog.active.is_none())?];
    for entry in catalog.entries {
        profiles.push(summary(
            entry.id.clone(),
            entry.directory,
            catalog.active.as_ref() == Some(&entry.id),
        )?);
    }
    Ok(profiles)
}
pub fn create(id: &str, display_name: String, directory: Option<PathBuf>) -> Result<Summary> {
    valid(id)?;
    let base = super::base_directory()?;
    let directory = match directory {
        Some(path) => std::path::absolute(path)?,
        None => base.join("profiles").join(id),
    };
    if directory.exists() {
        return Err(Error::Configuration(
            "profile directory exists; register it explicitly instead",
        ));
    }
    let settings = Settings {
        display_name,
        ..Settings::default()
    };
    // Validate before creating identity material or changing the catalog.
    settings.validate()?;
    super::storage::update(&base, "profiles.json", |catalog: &mut Catalog| {
        validate(catalog)?;
        if catalog.entries.len() >= 32
            || catalog
                .entries
                .iter()
                .any(|e| e.id == id || e.directory == directory)
        {
            return Err(Error::Configuration(
                "profile already registered or catalog full",
            ));
        }
        super::reserve_directory(&directory)?;
        crate::Identity::create(&directory.join("identity"))?;
        Settings::update(&directory, Some(settings.display_name), None)?;
        catalog.entries.push(Entry {
            id: id.into(),
            directory: directory.clone(),
        });
        Ok(())
    })?;
    summary(id.into(), directory, false)
}
pub fn register(id: &str, directory: PathBuf) -> Result<Summary> {
    valid(id)?;
    let directory = std::path::absolute(directory)?;
    super::private_directory(&directory, false)?;
    let directory = std::fs::canonicalize(&directory)?;
    Peer::load(&directory.join("identity/certificate.der"))?;
    let base = super::base_directory()?;
    let canonical_base = if base.try_exists()? {
        std::fs::canonicalize(&base)?
    } else {
        base.clone()
    };
    if directory == canonical_base {
        return Err(Error::Configuration(
            "the default profile is already registered implicitly",
        ));
    }
    super::storage::update(&base, "profiles.json", |catalog: &mut Catalog| {
        validate(catalog)?;
        if catalog.entries.len() >= 32
            || catalog.entries.iter().any(|e| {
                e.id == id
                    || e.directory == directory
                    || std::fs::canonicalize(&e.directory)
                        .is_ok_and(|existing| existing == directory)
            })
        {
            return Err(Error::Configuration(
                "profile already registered or catalog full",
            ));
        }
        catalog.entries.push(Entry {
            id: id.into(),
            directory: directory.clone(),
        });
        Ok(())
    })?;
    summary(id.into(), directory, false)
}
pub fn select(id: &str) -> Result<()> {
    let base = super::base_directory()?;
    select_at(&base, id)
}

fn select_at(base: &Path, id: &str) -> Result<()> {
    super::storage::update(base, "profiles.json", |catalog: &mut Catalog| {
        select_locked(base, id, catalog)
    })?;
    Ok(())
}

fn select_locked(base: &Path, id: &str, catalog: &mut Catalog) -> Result<()> {
    validate(catalog)?;
    // Resolve against the locked value, not a snapshot from before a removal.
    let directory = if id == "default" {
        base
    } else {
        &catalog
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or(Error::Configuration("unknown local profile"))?
            .directory
    };
    super::private_directory(directory, false)?;
    catalog.active = if id == "default" {
        None
    } else {
        Some(id.into())
    };
    Ok(())
}
pub fn remove(id: &str) -> Result<()> {
    valid(id)?;
    let base = super::base_directory()?;
    super::storage::update(&base, "profiles.json", |catalog: &mut Catalog| {
        validate(catalog)?;
        if catalog.active.as_deref() == Some(id) {
            return Err(Error::Configuration(
                "choose another active profile before removing this registration",
            ));
        }
        let count = catalog.entries.len();
        catalog.entries.retain(|e| e.id != id);
        if count == catalog.entries.len() {
            return Err(Error::Configuration("unknown local profile"));
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "catalog_tests.rs"]
mod tests;
