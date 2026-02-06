# ao-desktop

CLI installer and launcher for **AO** -- a Desktop AI Agent built with Tauri 2.

## Quick Start

```bash
npx ao-desktop
```

This will:
1. Detect your operating system and architecture
2. Download the latest AO release from GitHub
3. Run the platform-appropriate installer

## Commands

| Command | Description |
|---------|-------------|
| `npx ao-desktop` | Download and install the latest AO release (default) |
| `npx ao-desktop install` | Same as above |
| `npx ao-desktop version` | Show the CLI version |
| `npx ao-desktop help` | Show usage information |

## Supported Platforms

| Platform | Architecture | Installer |
|----------|-------------|-----------|
| Windows | x64 | NSIS setup (.exe) |
| macOS | Intel + Apple Silicon | Universal DMG (.dmg) |
| Linux | x64 | Debian package (.deb) |

## Platform Notes

- **macOS**: The app is not notarized. On first launch, right-click the app and select "Open" to bypass Gatekeeper.
- **Linux**: Requires `libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`. Ubuntu 22.04+ recommended. The `.deb` package will be installed via `dpkg`; you may be prompted for your password.
- **Windows**: The NSIS installer runs with user-level permissions (no admin required).

## Manual Download

You can also download installers directly from the [GitHub Releases](https://github.com/jbcupps/ao/releases/latest) page.

Stable download URLs:
- Windows: `https://github.com/jbcupps/ao/releases/latest/download/AO-windows-x64-setup.exe`
- macOS: `https://github.com/jbcupps/ao/releases/latest/download/AO-macos-universal.dmg`
- Linux: `https://github.com/jbcupps/ao/releases/latest/download/AO-linux-x64.deb`

## License

MIT
