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
