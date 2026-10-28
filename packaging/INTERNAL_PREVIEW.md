# Paint.R {{VERSION}} — internal development preview

Platform: `{{PLATFORM}}`.

## Distribution restriction

**This archive is an internal engineering artifact, not a public release.** Paint.R
first-party source code is MIT-licensed and the license text is included as `LICENSE`.
Third-party asset/font rights and required notices remain unresolved. This archive does
not grant rights to third-party content. Do not upload it to a release page or otherwise
redistribute it publicly.

The dependency notices are copied from locked Cargo package sources where discoverable.
`THIRD_PARTY_NOTICES.md` and `third-party-notices/index.json` state whether extraction
has gaps. This heuristic inventory and the SBOM license expressions are not legal review
or a claim of license compliance.

## Verification represented by this archive

The packager required the real `paint-ui` executable, checked its platform-specific
PE/Mach-O/ELF machine header, and successfully ran `tests/e2e/preview_smoke.py` against
that binary and the repository command traces before creating this archive.
`PREVIEW_BINARY.json` records the staged executable's format, machine, size, and hash;
this header evidence does not prove native/non-emulated execution. `PREVIEW_SMOKE.json`
contains normalized evidence with exact PNG/RGBA results, complete repeated-run metadata,
and rejection-path status. `SHA256SUMS.txt` covers the packaged payload; a separate
`.sha256` sidecar covers the archive.

Before returning the archive, the packager verifies that sidecar, reopens and extracts the
exact payload with explicit permission restoration, checks every staged byte and internal
hash, and repeats the same real smoke against the extracted executable. Pre/post evidence
must match. The adjacent `<archive>.verification.json` receipt records this headless
acceptance; it does not prove native-window behavior or OS integration.

This is not a broad Paint.NET compatibility claim. The included `PARITY_LEDGER.md` is the
authoritative per-feature disclosure. A trace passing against Paint.R is not itself
Paint.NET parity, screenshot parity, PDN round-trip acceptance, or arbitrary-plugin
compatibility.

## Dependency inventory

`sbom.cdx.json` is generated from `cargo metadata --locked --format-version 1`. It covers
the locked workspace dependency graph, including packages that may not be linked into this
platform's final executable. It is not an exact linked-binary, native-library, or asset
provenance audit.

## Practical platform limitations

- No installer, updater, signing, notarization, or store submission is provided.
- Windows: a ZIP containing `paint-ui.exe`; SmartScreen or other local policy may require
  an explicit administrator/user decision. No signature is claimed.
- macOS: a ZIP containing `Paint.R.app`; the app is unsigned and unnotarized, so Gatekeeper
  may prevent launch. The bundle has no icon or universal-binary claim.
- Linux: a `.tar.gz` containing `paint-ui`, not an AppImage or distribution package. Desktop
  integration, an AppDir runtime, and distro-specific native dependency validation are not
  supplied.

Development-preview exit still requires the owner to resolve distribution licensing and
the integrator to accept all reference, usability, and native three-OS gates.
