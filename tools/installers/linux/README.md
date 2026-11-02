# Linux Debian package builder

`build.sh` builds an **unsigned, internal development-preview** Debian package for the
x86_64 Linux `paint-ui` release binary. It is not a code-signed, notarized, published, or
officially supported installer. It does not create a repository, upload an artifact, or
modify a release.

## Interface

From any working directory:

```sh
bash tools/installers/linux/build.sh "<version>" "<binary>" "<output-dir>"
```

Relative `<binary>` and `<output-dir>` paths are resolved against the caller's current
working directory. `<version>` is a Debian version such as `0.1.1` or
`0.1.1-preview.1`; one leading `v` is accepted for a release tag and normalized away.
On success the output directory contains exactly:

- `paint-rust_<version>_amd64.deb`
- `paint-rust_<version>_amd64.deb.sha256`

Example:

```sh
bash tools/installers/linux/build.sh \
    "0.1.1" \
    "target/release/paint-ui" \
    "dist/linux"
```

`SOURCE_DATE_EPOCH` defaults to `946684800` (2000-01-01 UTC) when unset. It must be an
integer in the unsigned 32-bit range. The builder uses only the Python 3 standard
library; `dpkg-deb` is not required to build, but is used by the tests when available to
validate the result.

## Validation before packaging

The builder fails without producing artifacts when:

- the version is not a valid Debian version or cannot be used in the output filename;
- the binary is missing, unreadable, shorter than an ELF64 header, not little-endian
  64-bit ELF, not `ET_EXEC`/`ET_DYN`, or not `e_machine=62` (`EM_X86_64`);
- `README.md` is missing from the repository root;
- the bundled first-party SVG is missing, is not valid XML with an `<svg>` root, or
  references external resources;
- the output path is an existing non-directory.

`LICENSE` is included when it exists at the repository root.

## Package layout

| Path | Mode | Contents |
|---|---|---|
| `usr/bin/paint-ui` | `0755` | Unmodified release binary |
| `usr/share/applications/paint-rust.desktop` | `0644` | Desktop entry |
| `usr/share/icons/hicolor/scalable/apps/paint-rust.svg` | `0644` | First-party icon |
| `usr/share/doc/paint-rust/README.md` | `0644` | Repository README |
| `usr/share/doc/paint-rust/LICENSE` | `0644` | MIT license, when present |

The desktop entry uses `Name=Paint.Rust`, `Exec=paint-ui`, `Icon=paint-rust`,
`Categories=Graphics;RasterGraphics;2DGraphics;`, and MIME associations for PDR, PDN,
PNG, JPEG, TIFF, BMP, TGA, DDS, GIF and WebP.

The control archive contains only `./control` and `./md5sums`. There is **no**
`preinst`, `postinst`, `prerm`, or `postrm`; in particular, nothing fabricates a
signature, trust assertion, or installed-state claim.

Control fields include:

- `Package: paint-rust`
- `Version: <version>`
- `Architecture: amd64`
- `Maintainer: Diego Garcia <https://github.com/diegomgarcia>` (the public GitHub
  profile is used as contact because no public email is recorded)
- `Section: graphics`, `Priority: optional`
- `Depends: libc6, libx11-6, libxkbcommon0, libwayland-client0, libgl1, libegl1,
  libxcursor1, libxrandr2, libxi6`
- `Installed-Size` computed from packaged file bytes, rounded up to KiB
- a multi-line `Description` that labels the build as an unsigned development preview
- `Homepage: https://github.com/diegomgarcia/paint.rust`

No dependency version constraints or bundled native libraries are added. The package
assumes the target system supplies a compatible glibc, X11/Wayland, OpenGL/EGL, cursor,
Xrandr and Xi runtime. Those dependencies are declared, not validated by this tool.

## Determinism

Given identical input bytes, version and `SOURCE_DATE_EPOCH`, the output bytes are
reproducible:

- ar member order is `debian-binary`, `control.tar.gz`, `data.tar.gz`; ar metadata is
  `uid=0`, `gid=0`, mode `100644`, mtime `SOURCE_DATE_EPOCH`;
- tar entries are sorted POSIX paths, owned by `0:0` with empty names, mtime
  `SOURCE_DATE_EPOCH`, directory mode `0755`, binary `0755`, other files `0644`;
- gzip headers use `SOURCE_DATE_EPOCH` and contain no source filename;
- no host paths, usernames, build times, locale data, or filesystem order affect the
  archive;
- the sidecar is `<sha256>  <deb filename>` with a trailing newline.

## Local validation and tests

The tests run on macOS and Linux without root or `dpkg-deb`. They build a package from a
synthetic 64-byte ELF x86_64 header fixture, parse the ar/gzip/tar layers with Python, and
assert metadata, payload paths/modes, desktop fields, icon XML, documentation, sidecar,
error paths, and byte-for-byte determinism across two runs:

```sh
python3 -m unittest discover -s tools/installers/linux/tests -p 'test_*.py' -v
```

When `dpkg-deb` is present, the same test also runs `dpkg-deb --info`, `--contents` and
`--extract` against the generated package. A manual Linux check is:

```sh
dpkg-deb --info dist/linux/paint-rust_0.1.1_amd64.deb
dpkg-deb --contents dist/linux/paint-rust_0.1.1_amd64.deb
dpkg-deb --extract dist/linux/paint-rust_0.1.1_amd64.deb /tmp/paint-rust-root
```

## Limitations

- Unsigned and unnotarized; no GPG signing of the `.deb` or sidecar, no repository
  `Release` file, and no update mechanism.
- x86_64 only. The binary header is checked, but the package is not run or installed
  during the build.
- No desktop MIME database XML, icon cache refresh, or package-manager trigger; those
  would require maintainer scripts or distro integration outside this scope.
- Native runtime dependency availability and actual desktop launch on Linux are not
  proven by these package tests.
- The README copied into the package is the development-preview project README. This
  `.deb` is an internal engineering artifact, not a public release approval.
- The SVG is original first-party geometry authored for this package; it reproduces no
  Paint.NET artwork, logo, or icon.
