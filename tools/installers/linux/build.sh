#!/usr/bin/env bash
# Build an unsigned Paint.Rust x86_64 development-preview Debian package.
#
# Usage: bash tools/installers/linux/build.sh "<version>" "<binary>" "<output-dir>"
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd -P)"

if [ "$#" -ne 3 ]; then
    printf 'usage: %s "<version>" "<binary>" "<output-dir>"\n' "${BASH_SOURCE[0]}" >&2
    exit 2
fi

if [ -z "$2" ] || [ -z "$3" ]; then
    printf 'error: <binary> and <output-dir> must not be empty\n' >&2
    exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
    printf 'error: python3 is required to build the Debian package\n' >&2
    exit 1
fi

# Keep archive bytes reproducible by default. Callers may pin a different epoch.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-946684800}"

exec python3 "${SCRIPT_DIR}/build_deb.py" \
    --version "$1" \
    --binary "$2" \
    --output-dir "$3" \
    --repo-root "${REPO_ROOT}"
