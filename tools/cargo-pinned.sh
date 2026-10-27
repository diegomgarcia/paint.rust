#!/bin/sh
set -eu

TOOLCHAIN=1.95.0

# Rustup reports native Windows paths. MSYS/Git Bash needs POSIX paths to execute
# tools and construct PATH, while Cargo child processes need native RUSTC/RUSTDOC.
shell_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -u "$1"
    else
        printf '%s\n' "$1"
    fi
}

CARGO_NATIVE=$(rustup which cargo --toolchain "$TOOLCHAIN")
RUSTC_NATIVE=$(rustup which rustc --toolchain "$TOOLCHAIN")
RUSTDOC_NATIVE=$(rustup which rustdoc --toolchain "$TOOLCHAIN")
RUSTFMT_NATIVE=$(rustup which rustfmt --toolchain "$TOOLCHAIN")
CARGO_BIN=$(shell_path "$CARGO_NATIVE")
TOOLCHAIN_BIN=$(dirname "$CARGO_BIN")
CARGO=$CARGO_NATIVE
RUSTC=$RUSTC_NATIVE
RUSTDOC=$RUSTDOC_NATIVE
RUSTFMT=$RUSTFMT_NATIVE
RUSTUP_TOOLCHAIN=$TOOLCHAIN
PATH=$TOOLCHAIN_BIN:$PATH
export CARGO PATH RUSTC RUSTDOC RUSTFMT RUSTUP_TOOLCHAIN
exec "$CARGO_BIN" "$@"
