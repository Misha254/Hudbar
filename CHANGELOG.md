# Changelog

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
