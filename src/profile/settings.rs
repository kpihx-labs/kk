use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Validate presentation text, not identity or trust. Preserve ordinary Unicode,
/// including combining marks and joiners used by international names.
pub(super) fn validate_name(name: &str) -> Result<()> {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub display_name: String,
    #[serde(default)]
    pub appearance: Appearance,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            display_name: "My profile".into(),
            appearance: Appearance::default(),
        }
    }
}
impl Settings {
    pub fn load(directory: &Path) -> Result<Self> {
        let value: Self = super::storage::read(directory, "settings.json")?;
        value.validate()?;
        Ok(value)
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
        super::storage::update(directory, "settings.json", |current: &mut Self| {
            current.validate()?;
            change(current)?;
            current.validate()
        })
    }
    pub(super) fn validate(&self) -> Result<()> {
        validate_name(&self.display_name)?;
        self.appearance.validate()
    }
}

#[cfg(all(test, unix))]
#[path = "settings_tests.rs"]
mod tests;
