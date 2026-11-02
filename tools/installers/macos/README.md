# macOS development-preview installers

This lane builds two **unsigned development-preview** artifacts from the
verified macOS package ZIP produced by `tools/package/preview_package.py`:

- `Paint.Rust-<version>-macos-aarch64.dmg` with a drag-install layout
  (`Paint.Rust.app` plus an `/Applications` shortcut);
- `Paint.Rust-<version>-macos-aarch64.pkg`, a real component package built
  with `pkgbuild`/`productbuild` that installs `Paint.Rust.app` into
  `/Applications`.

The DMG also contains a concise `INSTALL.txt` and, when the input package
provides them, `LICENSE` and `README-PREVIEW.md`. No signing, notarization,
Gatekeeper approval, auto-update, universal-binary, or public-release claim is
made. The app remains an Apple-silicon (`aarch64`) preview.

## Usage

From the repository root on macOS:

```sh
bash tools/installers/macos/build.sh \
  "0.1.1" \
  "dist/paint-r-0.1.1-macos-aarch64.zip" \
  "dist/installers"
```

The interface is intentionally fixed for the CI lane:

```text
bash tools/installers/macos/build.sh "<version>" "<preview-package.zip>" "<output-dir>"
```

The input ZIP may place `Paint.R.app` at its root or inside one top-level
payload directory, as the current packager does. The app is renamed to
`Paint.Rust.app`; `Info.plist` gets `CFBundleName` and `CFBundleDisplayName`
set to `Paint.Rust`, `CFBundleIdentifier` set to `dev.paint-rust.app`, and its
existing `CFBundleExecutable=paint-ui` is preserved and verified. The required
executable is `Paint.Rust.app/Contents/MacOS/paint-ui`.

The script fails before writing artifacts when run off macOS, when the version
is empty or unsafe for a filename/package version, when inputs or required
commands (`hdiutil`, `pkgbuild`, `productbuild`, `pkgutil`, `python3`, `ditto`)
are missing, when the ZIP has no safe top-level `Paint.R.app`, when the app
metadata or executable is invalid, or when either built artifact fails
verification. Existing artifacts with the same names are replaced only after
both new artifacts verify. Temporary state is removed on exit.

## Determinism and verification

Staged files, including the app bundle and documentation, are timestamped from
`SOURCE_DATE_EPOCH` (default `946684800`, the packager's default). DMG and PKG
container metadata produced by `hdiutil`, `pkgbuild`, and `productbuild` is not
claimed to be byte-identical across runs or hosts; deterministic staging is the
practical limit without signing identities. Build products are verified before
they are moved into the output directory:

- the DMG checksum is checked with `hdiutil verify`;
- the DMG is mounted read-only and the app metadata, executable, `/Applications`
  symlink, `INSTALL.txt`, and copied documents are checked;
- the PKG is expanded and its `PackageInfo` identifier
  (`dev.paint-rust.app`), version, install location (`/Applications`), arm64
  host architecture, app bundle name, and payload executable bytes are checked.

## Tests

The integration test requires macOS with the same tools and exercises both
supported ZIP layouts plus rejection paths:

```sh
python3 -m unittest discover -s tools/installers/tests -p 'test_*.py' -v
```

## Limitations

- No code signing or notarization. Gatekeeper will likely warn about the app.
- No public redistribution, release upload, updater, or plugin installation.
- The `.pkg` installs the app bundle only; the DMG documentation is not copied
  into `/Applications` by the package.
- The PKG is a development artifact; it is not a Mac App Store or notarized
  distribution package.
- Verification is local to the produced files. It does not prove native launch,
  runtime behavior, or compatibility beyond the upstream package evidence.
