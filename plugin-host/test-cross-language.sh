#!/bin/sh
set -eu

# CROSS-LANGUAGE INTEGRATION LANE: builds the real .NET console host and runs
# the Rust broker against it. Failure is fatal; there are no skip fallbacks.
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$REPO_ROOT"

# cargo-pinned.sh selects Cargo/rustc/rustfmt; also pin companion drivers such
# as clippy-driver/rustdoc when this lane expands to full Rust verification.
PINNED_CARGO=$(rustup which cargo --toolchain 1.95.0)
PATH="${PINNED_CARGO%/*}:$PATH"
export PATH

dotnet restore "$SCRIPT_DIR/Paint.PluginHost.sln" --locked-mode
dotnet build "$SCRIPT_DIR/Paint.PluginHost.sln" --configuration Release --no-restore
./tools/cargo-pinned.sh run -p paint-plugin-bridge --bin paint-plugin-conformance -- \
    "$SCRIPT_DIR/src/Paint.PluginHost/bin/Release/net10.0/Paint.PluginHost.dll"
