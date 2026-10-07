# HUD Components

The repository has one canonical entrypoint for each HUD feature. Files currently present in
`~/.local/bin` from older iterations remain user-owned and are not removed automatically.

| Component | Canonical entrypoint | Implementation | Role |
| --- | --- | --- | --- |
| Top bar | `hudbar` | Rust binary | Wayland layer-shell bar and panels |
| Legacy settings window | `hud-settings` | shell wrapper + `hud-settings-rs` | Retained for compatibility; no current UI route |
| niri binds | `hud-keybinds` | shell wrapper + `hud-keybinds-rs` | Native keybind viewer |
| yazi binds | `hud-yazibinds` | shell wrapper + `hud-yazibinds-rs` | Native yazi keybind viewer |
| Settings backend | `hud-setting` | Python CLI | Persist settings and restart affected services |
| Theme switcher | `hud-theme` | Python CLI | Switch pixel/normal font and theme state |

The `CTL` chip in the bar opens the built-in control center. It provides quick actions for Wi-Fi,
Bluetooth, DND, audio, microphone, wallpapers, power menu, and screen lock.

All persistent HUD state is stored in `~/.config/hudbar/settings.json` under the `appearance`,
`hudbar`, and `notifications` sections. The previous `~/.config/hud-settings/settings.json` file
is read only during migration. `hud-migrate-settings` merges it with any existing flat HUDbar
settings, writes a backup before changing the canonical file, and leaves legacy files untouched.

## Legacy Settings Window

`hud-settings-rs` and the `hud-settings` wrapper are retained as legacy code. The current menu and
the native keybind/wallpaper windows cover the old window's user-facing settings, so current menu
and panel routes no longer launch it. The release build and manual wrapper remain available for
compatibility. Remove the legacy binary, wrapper, and old-window modules only after the Wi-Fi,
audio, and Bluetooth stage is complete and those routes are verified in the new menu.

## Deprecated active routes

`keybind.sh` and `yazibind.sh` are the previous Rofi implementations. They are no longer used by
the canonical niri keybinds; they remain on disk so an existing setup can be rolled back manually.

`hud-settings-rs`, `hud-keybinds-rs`, and `hud-yazibinds-rs` are implementation binaries, not
user-facing commands. Wrappers own toggle behavior and resolve binaries relative to `$HOME`.
