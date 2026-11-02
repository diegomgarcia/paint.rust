#!/usr/bin/env python3
"""Integration tests for the unsigned macOS development-preview installers.

The real build invokes hdiutil, pkgbuild, productbuild, and pkgutil, so these
tests run only on macOS with those tools available. Fixtures are disposable
synthetic app bundles; they are not product artifacts.
"""

from __future__ import annotations

import contextlib
import importlib.util
import os
import plistlib
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
import zipfile
from pathlib import Path
from typing import Iterator

REPO_ROOT = Path(__file__).resolve().parents[3]
BUILD = REPO_ROOT / "tools" / "installers" / "macos" / "build.sh"
VERSION = "0.2.0"
EXECUTABLE = b"#!/bin/sh\nexit 0\n"


def _required_tools() -> bool:
    return all(shutil.which(name) for name in ("hdiutil", "pkgbuild", "productbuild", "pkgutil", "ditto"))


@unittest.skipUnless(
    sys.platform == "darwin" and _required_tools(),
    "DMG/PKG integration tests require macOS with hdiutil, pkgbuild, productbuild, and pkgutil",
)
class MacInstallerIntegrationTest(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory(prefix="paint-rust-installer-test-")
        self.root = Path(self._temporary.name)

    def tearDown(self) -> None:
        self._temporary.cleanup()

    def _make_zip(
        self,
        *,
        layout: str = "wrapped",
        include_app: bool = True,
        include_executable: bool = True,
        plist_executable: str = "paint-ui",
        include_documents: bool = True,
        extra_entries: dict[str, bytes] | None = None,
    ) -> Path:
        source = self.root / "source"
        if layout == "wrapped":
            payload = source / f"paint-r-{VERSION}-macos-aarch64"
        elif layout == "root":
            payload = source
        else:
            raise ValueError(f"unknown layout: {layout}")
        payload.mkdir(parents=True, exist_ok=True)
        if include_app:
            app = payload / "Paint.R.app"
            (app / "Contents" / "MacOS").mkdir(parents=True)
            (app / "Contents" / "Resources" / "Preview").mkdir(parents=True)
            if include_executable:
                executable = app / "Contents" / "MacOS" / "paint-ui"
                executable.write_bytes(EXECUTABLE)
                executable.chmod(0o755)
            info = {
                "CFBundleDevelopmentRegion": "en",
                "CFBundleExecutable": plist_executable,
                "CFBundleIdentifier": "dev.paint-r.internal-preview",
                "CFBundleInfoDictionaryVersion": "6.0",
                "CFBundleName": "Paint.R Development Preview",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": VERSION,
                "CFBundleVersion": VERSION,
            }
            (app / "Contents" / "Info.plist").write_bytes(
                plistlib.dumps(info, fmt=plistlib.FMT_XML, sort_keys=True)
            )
            (app / "Contents" / "Resources" / "Preview" / "PREVIEW_BINARY.json").write_text(
                "{}\n", encoding="utf-8"
            )
        if include_documents:
            (payload / "LICENSE").write_text("MIT License\n", encoding="utf-8")
            (payload / "README-PREVIEW.md").write_text("# Preview\n", encoding="utf-8")
        for relative, data in (extra_entries or {}).items():
            target = source / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        archive = self.root / f"{layout}-preview.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    output.write(path, path.relative_to(source).as_posix())
        return archive

    def _make_packager_zip(self) -> Path:
        package_path = REPO_ROOT / "tools" / "package" / "preview_package.py"
        spec = importlib.util.spec_from_file_location("preview_package_installer_fixture", package_path)
        if spec is None or spec.loader is None:
            self.fail(f"could not load {package_path}")
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        packaged = self.root / "packaged"
        payload = packaged / f"paint-r-{VERSION}-macos-aarch64"
        binary = packaged / "paint-ui"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(EXECUTABLE)
        binary.chmod(0o755)
        module.stage_platform_files("macos-aarch64", VERSION, binary, payload)
        (payload / "LICENSE").write_text("MIT License\n", encoding="utf-8")
        (payload / "README-PREVIEW.md").write_text("# Preview\n", encoding="utf-8")
        module.write_sha256_manifest(payload)
        archive = self.root / "packager-preview.zip"
        module.create_zip_archive(payload, archive, epoch=946684800)
        return archive

    def _build(
        self,
        version: str,
        archive: Path,
        output_dir: Path,
        *,
        env: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment["SOURCE_DATE_EPOCH"] = "946684800"
        if env:
            environment.update(env)
        return subprocess.run(
            ["/bin/bash", str(BUILD), version, str(archive), str(output_dir)],
            check=False,
            capture_output=True,
            text=True,
            timeout=300,
            env=environment,
        )

    def _assert_build_ok(self, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(
            result.returncode,
            0,
            msg=f"build failed\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )

    @contextlib.contextmanager
    def _mounted(self, dmg: Path) -> Iterator[Path]:
        mount = self.root / "mnt"
        mount.mkdir()
        result = subprocess.run(
            [
                "hdiutil",
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
                str(mount),
                str(dmg),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        try:
            yield mount
        finally:
            detached = subprocess.run(
                ["hdiutil", "detach", str(mount)],
                check=False,
                capture_output=True,
                text=True,
            )
            if detached.returncode != 0:
                subprocess.run(
                    ["hdiutil", "detach", "-force", str(mount)],
                    check=False,
                    capture_output=True,
                    text=True,
                )
            shutil.rmtree(mount, ignore_errors=True)

    def _assert_app_bundle(self, app: Path, *, expected_bytes: bytes = EXECUTABLE) -> None:
        self.assertTrue(app.is_dir(), f"missing app bundle: {app}")
        info = plistlib.loads((app / "Contents" / "Info.plist").read_bytes())
        self.assertEqual(info["CFBundleName"], "Paint.Rust")
        self.assertEqual(info["CFBundleDisplayName"], "Paint.Rust")
        self.assertEqual(info["CFBundleIdentifier"], "dev.paint-rust.app")
        self.assertEqual(info["CFBundleExecutable"], "paint-ui")
        self.assertEqual(info["CFBundleShortVersionString"], VERSION)
        executable = app / "Contents" / "MacOS" / "paint-ui"
        self.assertTrue(executable.is_file(), "missing Contents/MacOS/paint-ui")
        self.assertTrue(os.access(executable, os.X_OK), "paint-ui is not executable")
        self.assertEqual(executable.read_bytes(), expected_bytes)
        self.assertEqual(int(executable.stat().st_mtime), 946684800)

    def _assert_dmg(self, dmg: Path, *, expected_bytes: bytes = EXECUTABLE) -> None:
        with self._mounted(dmg) as mount:
            self.assertFalse((mount / "Paint.R.app").exists(), "source app name leaked into the DMG")
            self._assert_app_bundle(mount / "Paint.Rust.app", expected_bytes=expected_bytes)
            applications = mount / "Applications"
            self.assertTrue(applications.is_symlink(), "missing /Applications shortcut")
            self.assertEqual(os.readlink(applications), "/Applications")
            install_text = (mount / "INSTALL.txt").read_text(encoding="utf-8")
            self.assertIn(f"Paint.Rust {VERSION}", install_text)
            self.assertIn("unsigned", install_text)
            self.assertIn("Applications", install_text)
            self.assertEqual((mount / "LICENSE").read_text(encoding="utf-8"), "MIT License\n")
            self.assertEqual(
                (mount / "README-PREVIEW.md").read_text(encoding="utf-8"), "# Preview\n"
            )

    def _assert_pkg(self, pkg: Path, *, expected_bytes: bytes = EXECUTABLE) -> None:
        expanded = self.root / f"expanded-{pkg.stem}"
        result = subprocess.run(
            ["pkgutil", "--expand-full", str(pkg), str(expanded)],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        package_info_paths = sorted(expanded.rglob("PackageInfo"))
        self.assertEqual(len(package_info_paths), 1)
        package_info = ElementTree.parse(package_info_paths[0]).getroot()
        self.assertEqual(package_info.tag, "pkg-info")
        self.assertEqual(package_info.get("identifier"), "dev.paint-rust.app")
        self.assertEqual(package_info.get("version"), VERSION)
        self.assertEqual(package_info.get("install-location"), "/Applications")
        distribution_paths = sorted(expanded.rglob("Distribution"))
        self.assertEqual(len(distribution_paths), 1)
        distribution = ElementTree.parse(distribution_paths[0]).getroot()
        options = distribution.find("options")
        self.assertIsNotNone(options)
        assert options is not None
        self.assertEqual(options.get("hostArchitectures"), "arm64")
        self.assertTrue(
            any(
                element.get("id") == "dev.paint-rust.app" and element.get("version") == VERSION
                for element in distribution.findall("pkg-ref")
            )
        )
        payload_executables = sorted(
            path
            for path in expanded.rglob("paint-ui")
            if path.is_file() and path.parent.name == "MacOS"
        )
        self.assertEqual(len(payload_executables), 1)
        payload_parts = payload_executables[0].parts
        self.assertIn("Paint.Rust.app", payload_parts)
        self.assertNotIn("Paint.R.app", payload_parts)
        self.assertEqual(payload_executables[0].read_bytes(), expected_bytes)

    def test_wrapped_package_zip_builds_dmg_and_pkg(self) -> None:
        archive = self._make_zip(layout="wrapped")
        output = self.root / "out"
        result = self._build(VERSION, archive, output)
        self._assert_build_ok(result)
        dmg = output / f"Paint.Rust-{VERSION}-macos-aarch64.dmg"
        pkg = output / f"Paint.Rust-{VERSION}-macos-aarch64.pkg"
        self.assertTrue(dmg.is_file())
        self.assertTrue(pkg.is_file())
        self.assertGreater(dmg.stat().st_size, 0)
        self.assertGreater(pkg.stat().st_size, 0)
        self.assertEqual(sorted(path.name for path in output.iterdir()), sorted([dmg.name, pkg.name]))
        self._assert_dmg(dmg)
        self._assert_pkg(pkg)
        self.assertIn(f"DMG: {dmg.resolve()}", result.stdout)
        self.assertIn(f"PKG: {pkg.resolve()}", result.stdout)

    def test_root_app_zip_and_prerelease_version(self) -> None:
        archive = self._make_zip(layout="root")
        output = self.root / "out"
        version = "1.2.3-preview.1"
        result = self._build(version, archive, output)
        self._assert_build_ok(result)
        dmg = output / f"Paint.Rust-{version}-macos-aarch64.dmg"
        pkg = output / f"Paint.Rust-{version}-macos-aarch64.pkg"
        self.assertTrue(dmg.is_file())
        self.assertTrue(pkg.is_file())
        with self._mounted(dmg) as mount:
            self._assert_app_bundle(mount / "Paint.Rust.app")
            self.assertIn(f"Paint.Rust {version}", (mount / "INSTALL.txt").read_text(encoding="utf-8"))
        expanded = self.root / "root-expanded"
        subprocess.run(
            ["pkgutil", "--expand-full", str(pkg), str(expanded)],
            check=True,
            capture_output=True,
            text=True,
        )
        package_info = ElementTree.parse(next(expanded.rglob("PackageInfo"))).getroot()
        self.assertEqual(package_info.get("version"), version)

    def test_zip_from_the_real_packager_layout_builds(self) -> None:
        archive = self._make_packager_zip()
        output = self.root / "out"
        result = self._build(VERSION, archive, output)
        self._assert_build_ok(result)
        dmg = output / f"Paint.Rust-{VERSION}-macos-aarch64.dmg"
        pkg = output / f"Paint.Rust-{VERSION}-macos-aarch64.pkg"
        self.assertTrue(dmg.is_file())
        self.assertTrue(pkg.is_file())
        self._assert_dmg(dmg)
        self._assert_pkg(pkg)

    def test_empty_version_is_rejected_without_artifacts(self) -> None:
        archive = self._make_zip()
        output = self.root / "out"
        result = self._build("", archive, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("version must not be empty", result.stderr)
        self.assertFalse(output.exists())
        self.assertEqual(list(output.glob("Paint.Rust-*")), [])

    def test_missing_zip_is_rejected_without_artifacts(self) -> None:
        output = self.root / "out"
        result = self._build(VERSION, self.root / "missing.zip", output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("input ZIP not found", result.stderr)
        self.assertEqual(list(output.glob("Paint.Rust-*")), [])

    def test_unsafe_version_is_rejected(self) -> None:
        archive = self._make_zip()
        output = self.root / "out"
        result = self._build("../escape", archive, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("version contains unsupported characters", result.stderr)
        self.assertFalse(output.exists())

    def test_non_macos_host_is_rejected(self) -> None:
        archive = self._make_zip()
        output = self.root / "out"
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        uname = fake_bin / "uname"
        uname.write_text("#!/bin/sh\necho Linux\n", encoding="utf-8")
        uname.chmod(uname.stat().st_mode | stat.S_IXUSR)
        result = self._build(
            VERSION,
            archive,
            output,
            env={"PATH": f"{fake_bin}:{os.environ['PATH']}"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("macOS is required", result.stderr)
        self.assertFalse(output.exists())

    def test_missing_required_command_is_rejected(self) -> None:
        archive = self._make_zip()
        output = self.root / "out"
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        uname = fake_bin / "uname"
        uname.write_text("#!/bin/sh\necho Darwin\n", encoding="utf-8")
        uname.chmod(uname.stat().st_mode | stat.S_IXUSR)
        for command_name in ("dirname", "basename"):
            executable = shutil.which(command_name)
            self.assertIsNotNone(executable)
            assert executable is not None
            os.symlink(executable, fake_bin / command_name)
        result = self._build(
            VERSION,
            archive,
            output,
            env={"PATH": str(fake_bin)},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required command not found: hdiutil", result.stderr)
        self.assertFalse(output.exists())

    def test_zip_without_app_is_rejected(self) -> None:
        archive = self._make_zip(include_app=False)
        output = self.root / "out"
        result = self._build(VERSION, archive, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not contain a top-level Paint.R.app", result.stderr)
        self.assertEqual(list(output.glob("Paint.Rust-*")), [])

    def test_app_without_executable_is_rejected(self) -> None:
        archive = self._make_zip(include_executable=False)
        output = self.root / "out"
        result = self._build(VERSION, archive, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing regular file Contents/MacOS/paint-ui", result.stderr)
        self.assertEqual(list(output.glob("Paint.Rust-*")), [])

    def test_wrong_bundle_executable_is_rejected(self) -> None:
        archive = self._make_zip(plist_executable="other-ui")
        output = self.root / "out"
        result = self._build(VERSION, archive, output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CFBundleExecutable must be 'paint-ui'", result.stderr)
        self.assertEqual(list(output.glob("Paint.Rust-*")), [])

    def test_path_traversal_zip_is_rejected(self) -> None:
        archive = self.root / "traversal.zip"
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("Paint.R.app/Contents/Info.plist", b"not a plist")
            output.writestr("../escape", b"escape")
        result = self._build(VERSION, archive, self.root / "out")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsafe ZIP entry path", result.stderr)
        self.assertFalse((self.root / "escape").exists())


if __name__ == "__main__":
    unittest.main()
