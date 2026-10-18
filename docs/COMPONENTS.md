# HUD Components

The repository has one canonical entrypoint for each HUD feature. Files currently present in
`~/.local/bin` from older iterations remain user-owned and are not removed automatically.

| Component | Canonical entrypoint | Implementation | Role |
| --- | --- | --- | --- |
| Top bar | `hudbar` | Rust binary | Wayland layer-shell bar and panels |
| App launcher | `hud-apps` | shell wrapper + `hud-apps-rs` | Native launcher on `Super+D`, replaces the rofi apps menu |
| niri binds | `hud-keybinds` | shell wrapper + `hud-keybinds-rs` | Native keybind viewer |
| yazi binds | `hud-yazibinds` | shell wrapper + `hud-yazibinds-rs` | Native yazi keybind viewer |
| Settings backend | `hud-setting` | Python CLI | Persist settings and restart affected services |

The `CTL` chip in the bar opens the built-in control center. It provides quick actions for Wi-Fi,
Bluetooth, DND, audio, microphone, wallpapers, power menu, and screen lock.

All persistent HUD state is stored in `~/.config/hudbar/settings.json` under the `hudbar`,
`notifications`, `osd`, and `vpn` sections. Key names, ranges, and installer defaults are
described by `docs/settings.schema.json`; the installer drops the schema next to the settings file,
which points at it with a relative `$schema` key, so an editor validates and completes the file. Two
tests keep the schema honest: one compares it with the parser (keys, ranges, enumerations), the other
with the installed default file. The previous `~/.config/hud-settings/settings.json` file
is read only as a fallback until 2027-01-01, and a script says so on stderr when it actually
reads it. `hud-migrate-settings` merges it with any existing flat HUDbar settings, writes a backup
before changing the canonical file, and leaves legacy files untouched. After 2027-01-01 the
fallbacks, the section merge in `hud-setting`, `hud-migrate-settings` and the legacy directory
itself go away (ROADMAP F).

## Removed Legacy Settings Window

The window, its `hud-settings-rs` binary and the `hud-settings` wrapper are gone: the menu and
the native keybind/wallpaper windows cover the settings it used to own, and nothing launched it.
Its leftover layout modules (`settings_ui`, `settings_view`, `settings_snapshot`,
`settings_widgets`, `ui_layout`) are gone too: the menu, wallpaper, VPN and schemes windows only
ever used the drawing primitives, the `Rect` geometry and the font name. Those now live in
`src/hud/canvas.rs`, `src/hud/geom.rs` and `ui_tokens::font_name`. The Wi-Fi, audio and Bluetooth
stages it had covered are complete and live in the menu instead. `src/hud/dunst.rs`, the window's
dunstrc patcher, is gone as well: `hud-setting` (`patch_dunst`) writes the same font, line-height
and origin values into `dunst/dunstrc` and the matugen template, and it is the only writer left.

## Deprecated active routes

`hud-drun.sh` is the previous Rofi implementation of the app launcher. The `hudapps`
mode it served is gone: `hud-apps` reads the same desktop files and the same pinned
list, and moves it to `~/.config/hudbar/apps-pinned` on first run. The script stays
on disk so an existing setup can be rolled back manually.

`keybind.sh` and `yazibind.sh` are the previous Rofi implementations. They are no longer used by
the canonical niri keybinds; they remain on disk so an existing setup can be rolled back manually.

`hud-keybinds-rs` and `hud-yazibinds-rs` are implementation binaries, not user-facing commands. Wrappers own toggle behavior and resolve binaries relative to `$HOME`.
