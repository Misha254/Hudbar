# Changelog

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
