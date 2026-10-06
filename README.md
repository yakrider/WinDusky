
# WinDusky

**WinDusky** is a Windows utility for applying customizable color effects, overlays, gamma/brightness/contrast presets, and screen magnification to any window or the entire desktop. It is designed to help users invert colors, simulate color blindness, reduce eye strain, or simply tweak their display experience with hotkeys and automation.

---

## Features

- **Per-window or Fullscreen Color Effects:** Apply color transformations (e.g., inversion, sepia, grayscale, color blindness simulation) to any window or the whole screen.
- **Gamma/Brightness/Contrast Presets:** Instantly switch between display presets for different lighting conditions or preferences.
- **Screen Magnification:** Zoom into the screen with configurable levels, useful for accessibility or presentations.
- **Auto-Overlay Rules:** Automatically apply overlays based on window luminance, executable name, or window class.
- **System Tray Integration:** Control and monitor WinDusky from the system tray with a rich context menu.
- **Configurable Hotkeys:** All actions are accessible via customizable global hotkeys.
- **Portable & Configurable:** No installation required; configuration is via a simple TOML file.

---

## Getting Started

### 1. Download & Run

- Download the latest release from [GitHub Releases](https://github.com/yakrider/WinDusky/releases).
- Extract and run `WinDusky.exe`. The app will create a default configuration file (`WinDusky.conf.toml`) in the same directory or in `%LOCALAPPDATA%\WinDusky\` if not writeable.

### 2. System Requirements

- Windows 10 or later (x64)
- No installation required; portable binary

### 3. Configuration

All settings are in `WinDusky.conf.toml`. The file is well-commented and supports:

- **Hotkeys:** Customize all actions (toggle overlays, cycle effects, gamma, magnification, etc.).
- **Color Effects:** Add, remove, or reorder color transformation matrices.
- **Gamma Presets:** Define and cycle through gamma/brightness/contrast/color-temp profiles.
- **Auto-Overlay Rules:** Set up rules for auto-applying overlays based on window luminance, exe, or class.
- **Logging:** Enable/disable logging for troubleshooting.

**Edit the config file** directly or use the tray menu's "Edit Config" option.

---

## Usage

### System Tray Menu

- **Auto Overlay Enabled:** Toggle automatic overlays based on rules.
- **Active Overlays:** See and clear overlays currently applied.
- **User Overrides:** Clear manual overlay toggles.
- **Full Screen Effect:** Toggle or change fullscreen color effect.
- **Magnification Level:** Toggle or change screen magnification.
- **Gamma Preset:** Toggle or cycle gamma/brightness/contrast presets.
- **Edit/Reset Config:** Open or reset the config file.
- **Restart/Quit:** Restart or exit WinDusky.

### Hotkeys

All hotkeys are customizable in the config file. Defaults include:

- `Alt+Win+I` — Toggle overlay on the active window
- `Alt+Win+Shift+I` — Toggle fullscreen overlay
- `Alt+Win+Period/Comma` — Cycle color effects
- `Win+Ctrl+I` — Toggle screen magnification
- `Win+Shift+I` — Toggle gamma preset
- ...and more

See the config file for the full list and customization.

---

## Auto-Overlay Rules

WinDusky can automatically apply overlays based on:

- **Luminance:** If a window is too bright, auto-apply a color effect.
- **Executable Name:** Always apply an effect to specific apps.
- **Window Class:** Apply effects to certain window types (e.g., dialogs).

You can fine-tune these rules in the config file, including exclusions and effect selection.

---

## Custom Color Effects

Define your own color transformation matrices in the config file. Each effect is a 5x5 matrix (see examples in the default config). Effects can be cycled and set as default.

---

## Troubleshooting

- **No effect?** Some windows (especially elevated or protected ones) may not allow overlays unless WinDusky is run as administrator.
- **Hotkeys not working?** Ensure no conflicts with other global hotkeys.
- **Logging:** Enable logging in the config for detailed logs in the config directory.

---

## Building from Source

1. Install [Rust](https://rustup.rs/).
2. Clone the repository:
   ```sh
   git clone https://github.com/yakrider/WinDusky.git
   cd WinDusky
   ```
3. Build:
   ```sh
   cargo build --release
   ```
4. The binary will be in `target/release/WinDusky.exe`.

---

## Credits

- Inspired by [NegativeScreen](https://github.com/mlaily/NegativeScreen) and [Gamgee](https://github.com/yakrider/Gamgee).
- Uses [tao](https://github.com/tauri-apps/tao) for tray integration and [tracing](https://github.com/tokio-rs/tracing) for logging.

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

## Feedback & Contributions

Issues and pull requests are welcome! For feature requests, bug reports, or suggestions, please use the [GitHub Issues](https://github.com/yakrider/WinDusky/issues) page.

