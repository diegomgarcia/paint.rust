#!/bin/sh
set -eu

# CROSS-LANGUAGE INTEGRATION LANE: builds the real .NET console host and runs
# the Rust broker against it. Failure is fatal; there are no skip fallbacks.
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)
cd "$REPO_ROOT"

# Native .NET processes need Windows paths when this POSIX script runs in
# MSYS/Git Bash. Unix-like hosts keep the original paths unchanged.
native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s\n' "$1"
    fi
}
SOLUTION=$(native_path "$SCRIPT_DIR/Paint.PluginHost.sln")
HOST_DLL=$(native_path "$SCRIPT_DIR/src/Paint.PluginHost/bin/Release/net10.0/Paint.PluginHost.dll")

dotnet restore "$SOLUTION" --locked-mode
dotnet build "$SOLUTION" --configuration Release --no-restore
./tools/cargo-pinned.sh run --locked -p paint-plugin-bridge --bin paint-plugin-conformance -- \
    "$HOST_DLL"
