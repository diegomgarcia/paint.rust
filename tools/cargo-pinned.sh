#!/bin/sh
set -eu

TOOLCHAIN=1.95.0
CARGO_BIN=$(rustup which cargo --toolchain "$TOOLCHAIN")
TOOLCHAIN_BIN=$(dirname "$CARGO_BIN")
RUSTC=$(rustup which rustc --toolchain "$TOOLCHAIN")
RUSTDOC=$(rustup which rustdoc --toolchain "$TOOLCHAIN")
RUSTFMT=$(rustup which rustfmt --toolchain "$TOOLCHAIN")
CARGO=$CARGO_BIN
RUSTUP_TOOLCHAIN=$TOOLCHAIN
PATH=$TOOLCHAIN_BIN:$PATH
export CARGO PATH RUSTC RUSTDOC RUSTFMT RUSTUP_TOOLCHAIN
exec "$CARGO_BIN" "$@"
