#!/usr/bin/env bash
# Build unsigned macOS development-preview installers from a verified package ZIP.
#
# Usage:
#   bash tools/installers/macos/build.sh "<version>" "<preview-package.zip>" "<output-dir>"
#
# Inputs:
#   - a ZIP produced by tools/package/preview_package.py for macos-aarch64;
#   - the requested version, used for artifact and package metadata.
#
# Outputs (in <output-dir>):
#   Paint.Rust-<version>-macos-aarch64.dmg
#   Paint.Rust-<version>-macos-aarch64.pkg
#
# This script does not sign or notarize either artifact.
set -euo pipefail

usage() {
  printf 'usage: %s "<version>" "<preview-package.zip>" "<output-dir>"\n' "$0" >&2
}

die() {
  printf 'build.sh: %s\n' "$*" >&2
  exit 1
}

if [[ $# -ne 3 ]]; then
  usage
  exit 64
fi

version=$1
input_zip=$2
output_dir=$3

if [[ $(uname -s) != Darwin ]]; then
  die "macOS is required to build DMG and PKG installers (found $(uname -s))"
fi
if [[ -z $version ]]; then
  die "version must not be empty"
fi
if [[ ! $version =~ ^[0-9A-Za-z][0-9A-Za-z._+-]*$ ]] || [[ $version == *..* ]]; then
  die "version contains unsupported characters: $version"
fi
if [[ ! -f $input_zip ]]; then
  die "input ZIP not found: $input_zip"
fi
if [[ ! -r $input_zip ]]; then
  die "input ZIP is not readable: $input_zip"
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
helper=$script_dir/installer_support.py
if [[ ! -f $helper ]]; then
  die "installer support script is missing: $helper"
fi

for command_name in hdiutil pkgbuild productbuild pkgutil python3 ditto; do
  if ! command -v -- "$command_name" >/dev/null 2>&1; then
    die "required command not found: $command_name"
  fi
done

mkdir -p -- "$output_dir" || die "cannot create output directory: $output_dir"
output_dir=$(cd -- "$output_dir" && pwd -P) || die "cannot resolve output directory: $output_dir"
if [[ ! -w $output_dir ]]; then
  die "output directory is not writable: $output_dir"
fi
input_dir=$(cd -- "$(dirname -- "$input_zip")" && pwd -P) || die "cannot resolve input ZIP directory"
input_zip=$input_dir/$(basename -- "$input_zip")

work=$(mktemp -d "$output_dir/.paint-rust-installer.XXXXXX") || die "cannot create a temporary directory in $output_dir"
mounted=false
mount_point=$work/mnt
cleanup() {
  if [[ $mounted == true ]]; then
    hdiutil detach "$mount_point" -force >/dev/null 2>&1 || true
    mounted=false
  fi
  if [[ -n ${work:-} && -d ${work:-} && $work == "$output_dir"/.paint-rust-installer.* ]]; then
    rm -rf -- "$work"
  fi
}
detach_image() {
  if hdiutil detach "$mount_point" >/dev/null 2>&1; then
    mounted=false
    return 0
  fi
  if hdiutil detach "$mount_point" -force >/dev/null 2>&1; then
    mounted=false
    return 0
  fi
  return 1
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

dmg_name="Paint.Rust-${version}-macos-aarch64.dmg"
pkg_name="Paint.Rust-${version}-macos-aarch64.pkg"

printf 'Preparing %s from %s\n' "$version" "$(basename -- "$input_zip")"
python3 "$helper" prepare --zip "$input_zip" --work "$work" --version "$version" \
  || die "could not prepare the app bundle from the input ZIP"

app=$work/Paint.Rust.app
python3 "$helper" check-app --app "$app" || die "staged app bundle failed verification"

dmg_stage=$work/dmg-stage
pkg_root=$work/pkg-root
mkdir -p -- "$dmg_stage" "$pkg_root"
ditto --norsrc --noextattr --noqtn "$app" "$dmg_stage/Paint.Rust.app"
ditto --norsrc --noextattr --noqtn "$app" "$pkg_root/Paint.Rust.app"
ln -s /Applications "$dmg_stage/Applications"
cp -p "$work/INSTALL.txt" "$dmg_stage/INSTALL.txt"
for document_name in LICENSE README-PREVIEW.md; do
  if [[ -f "$work/docs/$document_name" ]]; then
    cp -p "$work/docs/$document_name" "$dmg_stage/$document_name"
  fi
done
python3 "$helper" normalize --root "$dmg_stage" || die "could not normalize the DMG staging tree"
python3 "$helper" normalize --root "$pkg_root" || die "could not normalize the PKG staging tree"

dmg_build=$work/$dmg_name
pkg_build=$work/$pkg_name
component_pkg=$work/component.pkg
distribution=$work/Distribution

printf 'Building %s\n' "$dmg_name"
hdiutil create -quiet -ov -fs HFS+ -format UDZO -noanyowners \
  -volname "Paint.Rust" -srcfolder "$dmg_stage" "$dmg_build" >/dev/null

printf 'Building %s\n' "$pkg_name"
pkgbuild --quiet --root "$pkg_root" --identifier dev.paint-rust.app \
  --version "$version" --install-location /Applications "$component_pkg"
productbuild --quiet --synthesize --package "$component_pkg" "$distribution"
sed -i '' 's/hostArchitectures="[^"]*"/hostArchitectures="arm64"/' "$distribution"
if ! grep -q 'hostArchitectures="arm64"' "$distribution"; then
  die "could not constrain the PKG to arm64"
fi
productbuild --quiet --distribution "$distribution" --package-path "$work" "$pkg_build"

printf 'Verifying %s\n' "$dmg_name"
hdiutil verify "$dmg_build" >/dev/null
mkdir -p -- "$mount_point"
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mount_point" "$dmg_build" >/dev/null
mounted=true
if [[ ! -d $mount_point/Paint.Rust.app ]]; then
  die "DMG is missing Paint.Rust.app"
fi
python3 "$helper" check-app --app "$mount_point/Paint.Rust.app" \
  || die "DMG app bundle failed verification"
if [[ ! -L $mount_point/Applications ]] || [[ $(readlink "$mount_point/Applications") != /Applications ]]; then
  die "DMG is missing the /Applications shortcut"
fi
if [[ ! -f $mount_point/INSTALL.txt ]]; then
  die "DMG is missing INSTALL.txt"
fi
for document_name in LICENSE README-PREVIEW.md; do
  if [[ -f "$work/docs/$document_name" ]]; then
    if [[ ! -f "$mount_point/$document_name" ]]; then
      die "DMG is missing $document_name"
    fi
    if ! cmp -s "$work/docs/$document_name" "$mount_point/$document_name"; then
      die "DMG $document_name differs from the packaged document"
    fi
  fi
done
detach_image || die "could not detach the DMG"

printf 'Verifying %s\n' "$pkg_name"
python3 "$helper" verify-pkg --pkg "$pkg_build" --version "$version" \
  --work "$work" --source-app "$app" || die "PKG failed verification"

chmod 0644 "$dmg_build" "$pkg_build"
mv -f -- "$dmg_build" "$output_dir/$dmg_name"
mv -f -- "$pkg_build" "$output_dir/$pkg_name"

printf 'DMG: %s\n' "$output_dir/$dmg_name"
printf 'PKG: %s\n' "$output_dir/$pkg_name"
