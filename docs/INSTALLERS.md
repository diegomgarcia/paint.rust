# Installers and CI/CD

Paint.Rust publishes **unsigned development-preview** installers from GitHub Actions. There is
no code signing, notarization, auto-update or public-stable-channel claim yet.

## Generated artifacts

| Platform | Installer | Portable archive |
|---|---|---|
| macOS Apple silicon | `Paint.Rust-<version>-macos-aarch64.dmg` and `.pkg` | `paint-r-<version>-macos-aarch64.zip` |
| Windows x86_64 | `Paint.Rust-<version>-windows-x86_64-setup.exe` | `paint-r-<version>-windows-x86_64.zip` |
| Linux x86_64 | `paint-rust_<version>_amd64.deb` | `paint-r-<version>-linux-x86_64.tar.gz` |

Each installer/archive has a SHA-256 sidecar. The portable archives retain the full
`preview_package.py` payload: license, notices, SBOM, smoke evidence and verification receipt.

## GitHub Actions

`.github/workflows/installers.yml`:

- runs on pushes to `main`, `v*` tags and manual dispatch;
- builds one release binary per platform, then the portable archive and platform installer;
- runs the installer builder tests before packaging;
- uploads portable/installer artifacts for every run (14-day retention);
- on a `v*` tag, downloads the same run's artifacts and creates or updates a **prerelease**
  GitHub Release with all assets and sidecars.

Existing `.github/workflows/ci.yml` remains the correctness gate (format, clippy, tests,
headless smoke, native-gate orchestration tests and reproducible internal packages).

Manual dispatch only publishes when dispatched on a `v*` tag with `publish=true`; a normal
branch dispatch produces artifacts only.

## Local builds

macOS (after `preview_package.py` created the ZIP):

```sh
bash tools/installers/macos/build.sh 0.1.1 \
  dist/paint-r-0.1.1-macos-aarch64.zip dist-installer
```

Windows (NSIS 3.x; run in PowerShell):

```powershell
pwsh -File tools/installers/windows/build.ps1 `
  -Version 0.1.1 -BinaryPath target/release/paint-ui.exe -OutputDir dist-installer
```

Linux:

```sh
bash tools/installers/linux/build.sh 0.1.1 target/release/paint-ui dist-installer
```

## Validation status

- GitHub Actions `ci` and `installers`: **green on macOS, Windows and Linux**.
- `v0.1.2` prerelease published with DMG, PKG, NSIS setup, DEB, all portable archives and
  SHA-256 sidecars.
- macOS DMG/PKG and Linux DEB were also built and verified on real local hosts; packages are
  unsigned.
- Windows NSIS: driver tested with PowerShell 7 and NSIS 3 against a synthetic x86_64 PE
  fixture, and CI built the real `Paint.Rust-0.1.2-windows-x86_64-setup.exe`; a real
  Windows-host install/launch/uninstall remains a host gap.

## Known limitations

- No signing, notarization, Authenticode or package signing.
- No auto-update mechanism.
- macOS installer is Apple-silicon-only (`arm64`).
- Windows installer is x86_64 and requires NSIS 3.x to build.
- Windows byte-for-byte release reproducibility is not asserted; macOS/Linux keep that gate.
- Linux package has no MIME database trigger or desktop-cache trigger.
- Windows install/launch/uninstall and SmartScreen/UAC behavior have not been verified on a
  real Windows host yet.
