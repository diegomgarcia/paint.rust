from __future__ import annotations

import contextlib
import io
import sys
import unittest
from pathlib import Path
from unittest import mock

REFERENCE_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REFERENCE_DIR))

import run_windows_arm64_gate as arm64  # noqa: E402
import run_windows_x64_gate as x64  # noqa: E402


TOOL_ROOT = r"C:\PaintR-Reference\tools"
TOOLCHAIN = TOOL_ROOT + r"\rustup\toolchains\1.95.0-aarch64-pc-windows-gnullvm"
LLVM = TOOL_ROOT + r"\llvm-mingw\bin"
ROOT = r"C:\PaintR-Reference\candidate-src"
TARGET_DIR = r"C:\PaintR-Reference\candidate-target"
SMOKE_OUTPUT = r"C:\PaintR-Reference\evidence\candidate-smoke"
COMMIT = "242be405f7f0af7bcd08e82d251ca18f08ab10cd"
X64_ARCHITECTURE = (
    "Windows ARM64 host cross-compiled x64 gnullvm; "
    "any executed x64 tests/smoke run through Windows x64 emulation; "
    "not native Windows x64 and not MSVC ABI"
)


def expected_environment(triple: str, linker_arch: str) -> dict[str, str]:
    cargo_target = triple.replace("-", "_").upper()
    cc_target = triple.replace("-", "_")
    linker = LLVM + "\\" + linker_arch + r"-w64-mingw32-clang.exe"
    return {
        "RUSTUP_HOME": TOOL_ROOT + r"\rustup",
        "CARGO_HOME": TOOL_ROOT + r"\cargo",
        "RUSTUP_TOOLCHAIN": "1.95.0-aarch64-pc-windows-gnullvm",
        "CARGO": TOOLCHAIN + r"\bin\cargo.exe",
        "RUSTC": TOOLCHAIN + r"\bin\rustc.exe",
        "RUSTDOC": TOOLCHAIN + r"\bin\rustdoc.exe",
        "RUSTFMT": TOOLCHAIN + r"\bin\rustfmt.exe",
        "CARGO_TARGET_DIR": TARGET_DIR,
        "CARGO_BUILD_TARGET": triple,
        f"CARGO_TARGET_{cargo_target}_LINKER": linker,
        f"CARGO_TARGET_{cargo_target}_AR": LLVM + r"\llvm-ar.exe",
        f"CC_{cc_target}": linker,
        f"AR_{cc_target}": LLVM + r"\llvm-ar.exe",
        "PATH": ";".join([
            TOOLCHAIN + r"\bin",
            LLVM,
            TOOL_ROOT + r"\dotnet",
            TOOL_ROOT + r"\node",
            r"C:\Windows\System32",
            r"C:\Windows",
        ]),
        "CARGO_TERM_COLOR": "never",
        "CARGO_HTTP_TIMEOUT": "60",
        "CARGO_NET_RETRY": "3",
        "RUST_BACKTRACE": "1",
        "DOTNET_ROOT": TOOL_ROOT + r"\dotnet",
        "DOTNET_CLI_HOME": TOOL_ROOT + r"\dotnet-home",
        "DOTNET_CLI_TELEMETRY_OPTOUT": "1",
        "DOTNET_NOLOGO": "1",
        "NUGET_PACKAGES": TOOL_ROOT + r"\nuget-packages",
    }


def completed_report(job_id: str, executable: str, arguments: list[str], **_: object) -> dict:
    return {
        "job_id": job_id,
        "status": "COMPLETED",
        "exit_code": 0,
        "timed_out": False,
        "seconds": 0.0,
        "exit": {},
        "config": {"executable": executable, "arguments": arguments},
        "observation_errors": [],
        "logs": {},
    }


class WindowsX64GateTests(unittest.TestCase):
    def test_x64_environment_is_exact_and_host_toolchain_remains_arm64(self) -> None:
        self.assertEqual(x64.TRIPLE, "x86_64-pc-windows-gnullvm")
        self.assertEqual(x64.TOOLCHAIN, TOOLCHAIN)
        env = x64.environment(TARGET_DIR)
        self.assertEqual(env, expected_environment(x64.TRIPLE, "x86_64"))
        linker = LLVM + r"\x86_64-w64-mingw32-clang.exe"
        self.assertEqual(env["CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER"], linker)
        self.assertEqual(env["CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_AR"], LLVM + r"\llvm-ar.exe")
        self.assertEqual(env["CC_x86_64_pc_windows_gnullvm"], linker)
        self.assertEqual(env["AR_x86_64_pc_windows_gnullvm"], LLVM + r"\llvm-ar.exe")
        self.assertNotIn("CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER", env)
        self.assertNotIn("CC_aarch64_pc_windows_gnullvm", env)

    def test_x64_release_and_smoke_commands_are_exact(self) -> None:
        self.assertEqual(
            x64.command("release", ROOT, TARGET_DIR, SMOKE_OUTPUT),
            (
                TOOLCHAIN + r"\bin\cargo.exe",
                ["build", "-p", "paint-ui", "--release", "--locked", "--target", "x86_64-pc-windows-gnullvm"],
            ),
        )
        self.assertEqual(
            x64.command("smoke", ROOT, TARGET_DIR, SMOKE_OUTPUT),
            (
                TARGET_DIR + r"\x86_64-pc-windows-gnullvm\release\paint-ui.exe",
                [
                    "--headless-smoke",
                    "--trace",
                    ROOT + r"\tests\e2e\traces\invert-800x600.json",
                    "--output-dir",
                    SMOKE_OUTPUT,
                ],
            ),
        )

    def test_x64_summary_is_explicit_and_job_bounds_are_reused(self) -> None:
        self.assertEqual(x64.TIMEOUTS, arm64.TIMEOUTS)
        with (
            contextlib.redirect_stdout(io.StringIO()),
            mock.patch.object(x64, "run_guest_job", side_effect=completed_report) as run,
            mock.patch.object(x64, "write_json_new") as write,
        ):
            return_code = x64.main([
                "--source-commit", COMMIT,
                "--root", ROOT,
                "--target-dir", TARGET_DIR,
                "--smoke-output", SMOKE_OUTPUT,
                "--prefix", "unit-x64",
                "--steps", "release,smoke",
                "--summary", "unit-x64.json",
            ])
        self.assertEqual(return_code, 0)
        self.assertEqual([call.kwargs["timeout"] for call in run.call_args_list], [1800, 300])
        self.assertTrue(all(call.kwargs["working_directory"] == ROOT for call in run.call_args_list))
        summary_path, summary = write.call_args.args
        self.assertEqual(summary_path, x64.LOCAL_ROOT / "harness-final" / "unit-x64.json")
        self.assertEqual(summary["architecture"], X64_ARCHITECTURE)
        self.assertEqual(summary["source_commit"], COMMIT)
        self.assertEqual(summary["selected_steps"], ["release", "smoke"])
        self.assertEqual(summary["executed_steps"], ["unit-x64-release", "unit-x64-smoke"])
        self.assertEqual(summary["status"], "PASS")

    def test_arm64_environment_commands_and_label_do_not_regress(self) -> None:
        self.assertEqual(arm64.TRIPLE, "aarch64-pc-windows-gnullvm")
        self.assertEqual(arm64.environment(TARGET_DIR), expected_environment(arm64.TRIPLE, "aarch64"))
        self.assertEqual(
            arm64.command("release", ROOT, TARGET_DIR, SMOKE_OUTPUT),
            (
                TOOLCHAIN + r"\bin\cargo.exe",
                ["build", "-p", "paint-ui", "--release", "--locked", "--target", "aarch64-pc-windows-gnullvm"],
            ),
        )
        self.assertEqual(
            arm64.command("smoke", ROOT, TARGET_DIR, SMOKE_OUTPUT),
            (
                TARGET_DIR + r"\aarch64-pc-windows-gnullvm\release\paint-ui.exe",
                [
                    "--headless-smoke",
                    "--trace",
                    ROOT + r"\tests\e2e\traces\invert-800x600.json",
                    "--output-dir",
                    SMOKE_OUTPUT,
                ],
            ),
        )
        self.assertEqual(
            arm64.ARCHITECTURE,
            "native Windows ARM64; Rust host and target aarch64-pc-windows-gnullvm; not Windows x64 and not MSVC ABI coverage",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
