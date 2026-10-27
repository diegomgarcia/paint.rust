#!/bin/sh
set -eu

TOOLCHAIN=1.95.0
CARGO_BIN=$(rustup which cargo --toolchain "$TOOLCHAIN")
RUSTC=$(rustup which rustc --toolchain "$TOOLCHAIN")
RUSTFMT=$(rustup which rustfmt --toolchain "$TOOLCHAIN")
CARGO=$CARGO_BIN
RUSTUP_TOOLCHAIN=$TOOLCHAIN
export CARGO RUSTC RUSTFMT RUSTUP_TOOLCHAIN
exec "$CARGO_BIN" "$@"
