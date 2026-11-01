# Contributing to Paint.Rust

Thanks for your interest. Paint.Rust is an early development preview, so small, focused
contributions are easiest to review.

## Before you start

- Open an issue before a large feature, refactor or architectural change.
- Do not copy Paint.NET source, icons, bitmaps or other proprietary assets.
- Keep `references/` local and read-only. It is not part of the repository.
- Do not add fake controls, placeholder menu entries or claimed features without tests.

## Development workflow

1. Fork the repository and create a topic branch.
2. Add a failing test that demonstrates the intended behavior.
3. Implement the smallest real change that makes it pass.
4. Keep the change scoped to one behavior or UI surface.
5. Run the full verification suite.
6. Open a pull request with the behavior, tests and any known limitations.

## Verification

```sh
./tools/cargo-pinned.sh run -p xtask --locked -- verify
```

This covers formatting, strict Clippy, workspace tests, locked metadata and the stub scan.

For UI changes, also run the relevant `egui_kittest` suites and include before/after
screenshots when the visual result matters.

On macOS, native window changes should be validated with:

```sh
./tools/cargo-pinned.sh build --release -p paint-ui --locked
python3 tools/macos_native_usability_gate.py target/release/paint-ui \
  --output-dir artifacts/native-gate
```

Accessibility and Screen Recording permissions are required for the terminal running the gate.

## Code style

- Rust 1.95.0, edition 2024.
- `cargo fmt --all`.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- Prefer small, explicit functions and deterministic behavior.
- Document non-obvious invariants and avoid speculative abstractions.

## Reporting issues

Include:

- Operating system and version.
- Paint.Rust commit or tag.
- Steps to reproduce.
- Expected and actual behavior.
- Screenshots or native-gate output when applicable.

Security-sensitive issues should be reported privately to the maintainer rather than in a
public issue.
