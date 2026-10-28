# Local Profiles And Settings

Profiles contain independent device identities. Display names are human labels,
not certificates or invitation tokens. Changing a name never changes a key.

```sh
kk profile list
kk profile create work --name "My work profile"
kk profile select work
kk profile show
kk profile rename "My new display name"
kk profile appearance --hue 210 --clarity 70 --saturation 20
kk profile appearance --neutral --clarity 50 --saturation 20
kk status
kk status --json
```

Use `kk profile label ROOM MEMBER_DIGEST "Morgan"` for an explicit local nickname,
`kk profile labels ROOM` to list them, or append `--clear` instead of a name to
remove one. These labels are bound to the exact room and full member ID. They do
not prove a remote profile name, pair a contact or grant any permission.

No selection means the saved active profile, initially `default`. Global
`--profile-name ID` selects a registration for one command; `--profile PRIVATE_DIR`
selects a path instead. The two options are mutually exclusive.

Selection affects future commands, never running rooms or their keys. `profile
register ID PRIVATE_DIR` registers existing state without copying it; `profile
unregister ID` forgets only its registration, never deletes data. Select another
profile before unregistering the active one. IDs and display names are different.

Status reports the profile, public identity condition and node state even while
offline. It does not create missing profiles, inspect private keys or start a
service. Invalid saved metadata is reported and preserved, not silently reset.

The UI's Profiles and Settings actions use the same backend. Switching the saved
active profile requires restarting that UI; its current room owners cannot
silently switch identity. Hue is neutral or a quantified color; clarity and
saturation are bounded. Neutral, clarity 50 and saturation 20 restore the original
dark-grey midpoint. Settings are private and local to each profile.

Settings carry a storage-owned monotonic `revision`, serialized as a decimal
string to avoid JavaScript integer rounding. The UI settings action requires
`expected_revision`; the backend compares it under the same writer lock as the
update. A stale save returns `SETTINGS_CONFLICT` without partial writes. This also
detects intervening off/on/off consent changes whose final boolean looks unchanged.
The form reloads current sharing consent, retains nonsecret edits and requires a
fresh review/save. It never automatically repeats a stale opt-in.
SDK updates remain locked field-level operations; clients editing a loaded
snapshot should use `Settings::update_with_revision` for the same conflict check.

## Explicit Name Sharing

Display-name sharing is off by default, including for existing settings without
the option. Your own member row still shows your local profile name while sharing
is off; that does not mean another member has received it.

```sh
kk profile share-name --enable
kk profile share-name --disable
```

These commands change only the sharing option atomically. Renaming or changing
appearance does not implicitly enable sharing. UI Settings exposes the equivalent
unchecked-by-default **Share this display name with room members** control.

An enabled name is fetched as optional metadata through the authenticated private
room service, not broadcast into public consensus or chat. CLI status and the UI
use the same presentation resolver: your own local profile, then local overrides
for other members, then their self-announcements, then an unnamed fallback.
`self_announcement` means sender-bound text, not a verified human identity.
Duplicate or confusable names remain separate full certificate IDs; names never
route messages, select permissions or grant membership.

Status reads a nonblocking, memory-only cache (`profile_names` in local room JSON).
Names can appear or change after a bounded background refresh; a names error is
reported separately without making the basic room status offline. Disabling stops
future disclosure, not other people's memory: previously shared names cannot be
erased, cached observations may remain until refresh, and local nicknames remain
until explicitly cleared. See the cache bounds in `src/config.rs`.
