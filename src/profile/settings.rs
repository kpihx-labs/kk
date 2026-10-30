use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Validate presentation text, not identity or trust. Preserve ordinary Unicode,
/// including combining marks and joiners used by international names.
pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.chars().count() > 64
        || name.chars().any(|c| {
            c.is_control()
                || matches!(c, '\u{061c}' | '\u{200b}' | '\u{200e}' | '\u{200f}'
                    | '\u{202a}'..='\u{202e}' | '\u{2060}' | '\u{2066}'..='\u{206f}'
                    | '\u{feff}')
        })
    {
        return Err(Error::Configuration(
            "choose a name of 1-64 characters without control or direction-format characters",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub hue: Option<u16>,
    pub clarity: u8,
    pub saturation: u8,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            hue: None,
            clarity: 50,
            saturation: 20,
        }
    }
}
impl Appearance {
    pub fn validate(&self) -> Result<()> {
        if self.hue.is_some_and(|h| h >= 360) || self.clarity > 100 || self.saturation > 100 {
            return Err(Error::Configuration(
                "appearance needs hue 0-359 or neutral, clarity/saturation 0-100",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SidebarMode {
    #[default]
    Expanded,
    Compact,
    Auto,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Storage-owned monotonic revision, encoded as a canonical decimal string
    /// so JSON clients never round values through a JavaScript number.
    #[serde(default, with = "revision")]
    pub revision: u64,
    pub display_name: String,
    /// Explicit consent to disclose the display name to authorized room peers.
    #[serde(default)]
    pub share_display_name: bool,
    /// Local opt-in to content-free UI message alerts; never grants device permission.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub message_notifications: bool,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub sidebar_mode: SidebarMode,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            display_name: "My profile".into(),
            share_display_name: false,
            message_notifications: false,
            appearance: Appearance::default(),
            sidebar_mode: SidebarMode::default(),
        }
    }
}
impl Settings {
    pub fn parse_revision(value: &str) -> Result<u64> {
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(Error::Configuration(
                "settings revision must be a canonical u64 decimal string",
            ));
        }
        value.parse().map_err(|_| {
            Error::Configuration("settings revision must be a canonical u64 decimal string")
        })
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let value: Self = super::storage::read(directory, "settings.json")?;
        value.validate()?;
        Ok(value)
    }
    /// Prepare off the async runtime. No lock remains held after return. Missing
    /// or opted-out settings yield None without creating files or locks.
    pub fn publication(directory: &Path) -> Result<Option<SettingsPublication>> {
        if !Self::load(directory)?.share_display_name {
            return Ok(None);
        }
        let (gate, settings) = super::storage::publication::<Self>(directory)?;
        settings.validate()?;
        Ok(settings.share_display_name.then_some(SettingsPublication {
            name: settings.display_name,
            gate,
            #[cfg(all(test, unix))]
            before_callback: std::sync::Mutex::new(None),
        }))
    }
    /// Apply supplied fields to the latest settings under one writer lock.
    /// Absent fields are preserved; invalid changes leave storage untouched.
    pub fn update(
        directory: &Path,
        display_name: Option<String>,
        appearance: Option<Appearance>,
    ) -> Result<Self> {
        Self::update_with(directory, |current| {
            if let Some(display_name) = display_name {
                current.display_name = display_name;
            }
            if let Some(appearance) = appearance {
                current.appearance = appearance;
            }
            Ok(())
        })
    }
    /// Mutate the latest settings while holding the writer lock, then validate.
    /// Apply only intended changes here, never replace a previously read snapshot.
    /// The callback must be short and must not re-enter settings storage.
    /// Callback errors or invalid settings leave the stored bytes unchanged.
    pub fn update_with(
        directory: &Path,
        change: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<Self> {
        Self::mutate(directory, None, change)
    }
    /// Compare the expected revision inside the writer lock, before invoking the
    /// callback. Conflicts never invoke it and leave all saved bytes unchanged.
    pub fn update_with_revision(
        directory: &Path,
        expected_revision: u64,
        change: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<Self> {
        Self::mutate(directory, Some(expected_revision), change)
    }
    fn mutate(
        directory: &Path,
        expected: Option<u64>,
        change: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<Self> {
        super::storage::update(directory, "settings.json", |current: &mut Self| {
            current.validate()?;
            if expected.is_some_and(|expected| expected != current.revision) {
                return Err(Error::SettingsConflict);
            }
            let before = current.clone();
            change(current)?;
            if current.revision != before.revision {
                return Err(Error::Configuration(
                    "settings revision is managed by storage",
                ));
            }
            current.validate()?;
            if *current != before {
                current.revision = before
                    .revision
                    .checked_add(1)
                    .ok_or(Error::Configuration("settings revision exhausted"))?;
            }
            Ok(())
        })
    }
    pub(super) fn validate(&self) -> Result<()> {
        validate_name(&self.display_name)?;
        self.appearance.validate()
    }
}

mod revision {
    use super::*;
    pub(super) fn serialize<S: serde::Serializer>(
        value: &u64,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        Settings::parse_revision(&value).map_err(serde::de::Error::custom)
    }
}

/// A validated, opted-in name and retained settings inode. This is not a lease:
/// every publication callback must reacquire and validate the writer's lock.
pub struct SettingsPublication {
    name: String,
    gate: super::storage::Publication,
    #[cfg(all(test, unix))]
    before_callback: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl SettingsPublication {
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Invoke exactly one short nonblocking write poll, under a nonblocking
    /// shared lease on the settings writer lock. The callback must not await,
    /// block, or re-enter settings storage. Changed paths/settings fail closed.
    /// Bytes accepted before an opt-out cannot be recalled.
    /// Nonblocking applies to lease acquisition and the caller's write poll;
    /// path verification still performs synchronous OS metadata operations.
    /// Stable private paths and cooperating writers are required: these checks
    /// are not protection against a malicious actor with the same OS account.
    pub fn try_publish<T>(&self, publish: impl FnOnce() -> T) -> Result<T> {
        self.gate.try_publish(|| {
            #[cfg(all(test, unix))]
            {
                let hook = self.before_callback.lock().unwrap().take();
                if let Some(hook) = hook {
                    hook();
                }
            }
            publish()
        })
    }
    #[cfg(all(test, unix))]
    pub(crate) fn before_publication_callback(&self, hook: impl FnOnce() + Send + 'static) {
        *self.before_callback.lock().unwrap() = Some(Box::new(hook));
    }
}

#[cfg(all(test, unix))]
#[path = "publication_tests.rs"]
mod publication_tests;
#[cfg(all(test, unix))]
#[path = "revision_tests.rs"]
mod revision_tests;
#[cfg(all(test, unix))]
#[path = "settings_tests.rs"]
mod tests;
