# HUD Components

The repository has one canonical entrypoint for each HUD feature. Files currently present in
`~/.local/bin` from older iterations remain user-owned and are not removed automatically.

| Component | Canonical entrypoint | Implementation | Role |
| --- | --- | --- | --- |
| Top bar | `hudbar` | Rust binary | Wayland layer-shell bar and panels |
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

## Removed Legacy Settings Window

The window, its `hud-settings-rs` binary and the `hud-settings` wrapper are gone: the menu and
the native keybind/wallpaper windows cover the settings it used to own, and nothing launched it.
The shared UI layer (`settings_ui`, `settings_view`, `settings_snapshot`, `settings_widgets`)
stays, because the menu, wallpaper, VPN and schemes windows are built on it. The stale paragraph
The Wi-Fi, audio and Bluetooth stages it had covered are complete and live in the menu instead.

## Deprecated active routes

`keybind.sh` and `yazibind.sh` are the previous Rofi implementations. They are no longer used by
the canonical niri keybinds; they remain on disk so an existing setup can be rolled back manually.

`hud-keybinds-rs` and `hud-yazibinds-rs` are implementation binaries, not user-facing commands. Wrappers own toggle behavior and resolve binaries relative to `$HOME`.
