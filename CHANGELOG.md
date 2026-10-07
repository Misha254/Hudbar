# Changelog

## [0.4.2] - 2026-10-07

### Fixed
- Menu: a snapshot arriving while a nested submenu of a dynamic section was open no longer left stale rows — the dynamic level and everything stacked above it are rebuilt together.
- Menu: when the node a nested level was opened through disappears from the new snapshot, the menu returns to the last level that still exists instead of showing an empty screen.
- Menu: the "working…" marker on a busy row survives a background snapshot refresh of its section.

## [0.4.1] - 2026-10-07

### Fixed
- Bluetooth: a failed `devices Paired`/`devices Connected` read no longer turns into `false` for every device — state is now `Option<bool>` and stays `unknown`.
- Bluetooth: with unknown paired/connected state the device submenu offers no Pair/Connect/Remove/Trust action, only an explicit "state unknown" row.
- Bluetooth: `Powered: None` no longer implies "off" — the power button is hidden when adapter power state is unknown.
- Bluetooth: ordinary refresh is now exactly four `bluetoothctl` calls; the per-device `info <MAC>` N+1 is gone, so refresh no longer scales with device count.
- Wi-Fi: profile SSID is unescaped with the same `split_nmcli_terse` as the network list, so `My\:Network` and `Дом \\ принтер` match their visible networks.
- Wi-Fi: a failed `nmcli radio` read shows "unknown" instead of "off".
- Secret: `SecretInput` and `Menu` no longer implement `Clone`, so a plaintext password cannot be duplicated through ordinary UI state cloning.
- Docs: corrected the claims that the password disappears after submit and that it travels via stdin — it now passes through `CommandSpec`/`CommandJob` and remains visible in the child process `argv`.

## [0.4.0] - 2026-10-07

### Added
- Live HUD menu window (layer-shell overlay) with keyboard navigation, global search and per-section providers.
- Menu providers: audio devices and volume, Bluetooth, Wi-Fi networks, displays with layouts and presets, storage with safe unmount.
- Wallpaper picker window: thumbnail grid, folders, ten matugen schemes, mouse and keyboard control.
- Volume OSD overlay with PipeWire watcher (disabled by default while a compositor freeze is investigated).
- VPN window (`hud-vpn-rs` on numpad 7): three Mihomo modes over `MODE-RU`/`MODE-REST` selectors plus a filtered server list (Nordics, USA, Japan).
- Cyclic arrow-key navigation in menus, wallpaper grid, folders and scheme chips.
- Single-instance toggle for every overlay window via pid-file, with stale and recycled pid protection.

### Fixed
- VPN mode and server switches re-read the controller state, so partial failures are shown honestly.
- HTTP client rejects truncated bodies, oversized responses and non-loopback controller addresses; bearer secret never reaches logs or argv.

## [0.3.0] - 2026-10-03

### Added
- Wallpaper section: recursive scan of `~/wallpapers` (jpg, jpeg, png, webp), per-page scrolling, ten matugen schemes and one explicit apply button.
- `control_button` setting: the gear slot can be hidden from the panel.
- Panel section with per-module switches, reordering and panel height.
- Notifications section for dunst font size, line height and screen corner.
- Controls section: niri keybinds merged per action, clipped to width, read-only.
- Audit of control center actions without a niri bind.

### Changed
- Module zones are laid out from the real content width instead of a fixed constant.
- Overview shows only a summary of theme, enabled modules, height and language.
- Footer hints are per section; the language switcher became two buttons, `РУС` and `EN`.
- Settings are written through single-field patches that keep unknown keys and key order.
- Wallpaper file labels show `folder/name` instead of the full path.

### Fixed
- Row rebuilds no longer drop the wallpaper list, and focus stays on the row you acted on.
- Applying wallpapers reports the script's stderr instead of a bare exit code.
- The placeholder label for unfinished sections is gone.

## [0.2.0] - 2026-10-02

### Added
- Native Rust windows for HUD settings, niri keybinds, and yazi keybinds.
- Built-in control center for network, audio, notifications, wallpapers, power, and lock actions.
- Safe installer, settings migration, and component documentation.
- RAM icon in the status bar.

### Changed
- Store appearance, HUD modules, and notification preferences in one settings file.
- Use native keybind viewer routes instead of the previous Rofi scripts.

### Fixed
- Keep the normal battery chip background while highlighting critically low battery text.
- Refresh battery estimates after power state changes settle.
