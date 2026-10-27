# Paint.R

An original, cross-platform Rust image editor being developed against a pinned Paint.NET 5.1.12 reference environment.

> **Status:** engineering foundation and development-preview release train. This is not yet a pixel-identical Paint.NET clone. Exact compatibility claims are tracked per feature in `docs/PARITY_LEDGER.md`.

## Prerequisites

- Rust 1.95.0 with `rustfmt` and `clippy` (provided by `rust-toolchain.toml`).
- .NET SDK 10.0.201 for the isolated plugin-host development lane.
- For reference generation only: the licensed Windows 11 ARM64 UTM VM and Paint.NET 5.1.12 described in `docs/IMPLEMENTATION_PLAN.md`.

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- verify
```

Run the development UI after the first preview slice is integrated:

```sh
cargo run -p paint-ui
```

## Engineering rules

- The document/session is the single writer; rendering and plugin workers return versioned patches.
- CPU document semantics are authoritative. GPU presentation is not a correctness oracle.
- Every feature starts with failing behavioral tests and ends with independent integration verification.
- No stub implementations, ignored acceptance tests, auto-blessed goldens, or unreported platform exceptions.
- Paint.NET and its assemblies are reference-only and are never product dependencies.

See `docs/ENGINEERING_CONTRACT.md`, `docs/WORK_PACKETS.md`, and `docs/PARITY_LEDGER.md`.
