#!/bin/sh
set -eu

TOOLCHAIN=1.95.0
CARGO_BIN=$(rustup which cargo --toolchain "$TOOLCHAIN")
RUSTC=$(rustup which rustc --toolchain "$TOOLCHAIN")
RUSTFMT=$(rustup which rustfmt --toolchain "$TOOLCHAIN")
export RUSTC RUSTFMT
exec "$CARGO_BIN" "$@"
