# Windows x86_64 development-preview installer

Unsigned NSIS 3 / MUI2 installer definition and PowerShell build driver for
Paint.Rust. This lane adds the installer sources only; it does not publish, sign,
or upload anything.

## Status and limitations

This is an **unsigned Windows x86_64 development-preview installer**. It has not
been compiled or tested on a Windows host in this checkout: the standard macOS
development environment does not have `pwsh` or `makensis` installed. **No setup
executable is checked in or produced by these tests**, and no Windows install,
uninstall, SmartScreen, signing, notarization, or public-release claim is made.

- No Authenticode signature, certificate, timestamp, or reputation is provided.
  No signing step or signing claim exists in these sources.
- The installer is per-machine and requires administrator approval. A standard
  user cannot install it.
- The target is x86_64 Windows only. ARM64, 32-bit, MSI, portable-ZIP, and store
  packages are out of scope for this lane. NSIS itself produces the usual 32-bit
  installer stub; the installed `paint-ui.exe` is the x86_64 payload.
- The installer does not add file associations, services, background updaters,
  telemetry, or store integration.
- The installed payload contains no proprietary or third-party assets. It
  installs only the supplied `paint-ui.exe`, this lane's `INSTALL-WINDOWS.txt`,
  the repository MIT `LICENSE`, and the NSIS uninstaller.
- The installer has no icon or branding image; NSIS/MUI2 defaults are used.
- The installer does not test Paint.NET parity or guarantee every file format.

## Prerequisites

- A 64-bit Windows build of `paint-ui.exe` (PE machine `0x8664`). Build it from
  the repository root with
  `bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui`,
  or the equivalent `cargo build --release --locked -p paint-ui` on Windows.
- NSIS 3.x with Unicode support. `makensis.exe` must be on `PATH`, in the
  standard `%ProgramFiles(x86)%\NSIS` directory, or supplied with
  `-MakensisPath`. `%ProgramFiles%\NSIS` is also checked.
- PowerShell 7 (`pwsh`) is the CI interface. The script also avoids PowerShell 7
  only syntax and should work with Windows PowerShell 5.1.
- The repository `LICENSE` file. It is installed beside the application.

## CI interface

The interface expected by the CI lane is exactly:

```text
pwsh -File tools/installers/windows/build.ps1 -Version "<version>" -BinaryPath "<target/release/paint-ui.exe>" -OutputDir "<output-dir>"
```

Example:

```text
pwsh -File tools/installers/windows/build.ps1 -Version "0.1.1" -BinaryPath "target/release/paint-ui.exe" -OutputDir "dist/windows"
```

Optional switches:

- `-MakensisPath "<path-to-makensis.exe>"` overrides discovery.
- `-PlanOnly` validates the inputs and prints the resolved build plan as JSON
  without requiring `makensis` and without creating any output. This is intended
  for cross-platform contract tests; it does not build an installer.

## Outputs

For a version such as `0.1.1`:

```text
<output-dir>/Paint.Rust-0.1.1-windows-x86_64-setup.exe
<output-dir>/Paint.Rust-0.1.1-windows-x86_64-setup.exe.sha256
```

The sidecar is UTF-8 without BOM and contains one lowercase SHA-256 digest and
the setup file name, separated by two spaces and followed by a newline:

```text
<64 lowercase hex characters>  Paint.Rust-<version>-windows-x86_64-setup.exe
```

The sidecar covers the setup executable only. It is not a signature and does not
authenticate the publisher.

## What the build driver validates

1. `Version` is a semantic version (`X.Y.Z` with an optional `-prerelease`
   suffix). Build metadata (`+...`), leading-zero numeric components, whitespace,
   and components above 65535 are rejected. The four-part numeric
   `VIProductVersion` is derived from the core version and `.0`.
2. The installer script, `INSTALL-WINDOWS.txt`, and repository `LICENSE` exist.
3. `BinaryPath` exists, is a file named `paint-ui.exe`, starts with `MZ`, has a
   valid `PE\0\0` signature, and reports PE machine `0x8664`.
4. `OutputDir` resolves to a path that is not an existing file.
5. `makensis` is resolved from `-MakensisPath`, `PATH`, or the standard NSIS
   Program Files locations. A normal build fails clearly if it is absent.
6. Old copies of the named setup executable and sidecar are removed before
   compiling, so a failed run cannot be mistaken for a fresh success.
7. `makensis` is invoked with one argument per define, including absolute paths
   with spaces. A nonzero exit aborts the build.
8. The setup executable exists, is nonempty, and has an `MZ` header. The build
   writes and reads back the SHA-256 sidecar byte-for-byte.

`-PlanOnly` performs steps 1-5 and emits JSON with the resolved paths, the
numeric product version, the binary hash, and whether `makensis` was found. It does
not create the output directory, remove files, or invoke `makensis`.

## Installer behavior

- Installs per-machine to `$PROGRAMFILES64\Paint.Rust` after administrator
  approval (`RequestExecutionLevel admin`, `SetShellVarContext all`).
- Refuses to run on a non-x64 Windows installation.
- Installs `paint-ui.exe`, `INSTALL-WINDOWS.txt`, `LICENSE`, and
  `uninstall.exe`.
- Creates a `Paint.Rust` Start Menu folder containing the application and
  uninstall shortcuts.
- Offers an optional desktop shortcut. The component is unselected by default.
- Writes an Add/Remove Programs entry under the per-machine
  `Software\Microsoft\Windows\CurrentVersion\Uninstall\Paint.Rust` key with
  display name `Paint.Rust`, publisher `Diego Garcia`, product URL
  `https://github.com/diegomgarcia/paint.rust`, and quoted uninstall commands.
- Writes `Software\Paint.Rust` installation metadata for upgrades.
- The uninstaller deletes only the files, shortcuts, and registry keys the
  installer created. It never deletes documents, PDR/PDN files, or per-user
  settings. The install directory is removed with a non-recursive `RMDir`, so
  user-placed files keep the directory in place.

`INSTALL-WINDOWS.txt` is checked into this directory and embedded into the
installer. It states the unsigned, development-preview limitations at install
time and remains available from the install directory and the finish page.

## Tests

Run from the repository root:

```text
python3 -m unittest discover -s tools/tests -p "test_windows_installer.py" -v
```

The tests use only the Python standard library and never invoke `makensis` or
claim that an installer was built.

Always-on checks on macOS/Linux/Windows:

- static validation of `paint-rust.nsi`: Unicode, MUI2, required defines,
  per-machine `$PROGRAMFILES64` target, admin request, Start Menu and optional
  desktop shortcuts, uninstaller, registry entries, metadata, and no signing
  commands or proprietary asset files;
- static validation of `build.ps1`: interface parameters, version validation,
  PE x86_64 inspection, `makensis` discovery and quoted invocation, output
  verification, and SHA-256 sidecar logic;
- Python execution of the version regex extracted from `build.ps1`, plus the
  static build-plan names and sidecar contract;
- documentation and ASCII-only source checks for Windows PowerShell 5.1
  compatibility.

When `pwsh` is present, the suite additionally runs `-PlanOnly` against a
synthetic x86_64 PE fixture and checks argument rejection for bad versions,
missing binaries, wrong binary names, non-x86_64 PE machines, and existing-file
output paths. If `pwsh` is present but `makensis` is not, one test proves a normal
build fails before creating the output directory. If `makensis` is present, no
test compiles an installer; the PowerShell build path remains a Windows CI
responsibility.

The standard macOS development environment for this checkout does not have
`pwsh` or `makensis` installed, so the PowerShell cases are skipped explicitly
rather than treated as passing.

### Optional manual source compile

If `pwsh` and a POSIX `makensis` are available, the build driver can be exercised
with a synthetic x86_64 PE fixture in a temporary directory. On non-Windows hosts
it selects `-` makensis option prefixes; Windows uses `/`. This validates NSIS
expansion and compilation plus the setup/sidecar write path only. It does not
execute the installer, test Windows behavior, or create a checked-in artifact.

## Windows verification still required

A Windows CI or reviewer host must run the exact interface above, confirm the
two output files, run `Get-FileHash -Algorithm SHA256` against the setup
executable, verify the sidecar, install with administrator approval, check the
Start Menu and optional desktop shortcuts, check the Add/Remove Programs entry,
launch and close the application, uninstall, and confirm that a user document
and per-user settings survive uninstall. That evidence is not present in this
lane and must not be inferred from these static tests.
