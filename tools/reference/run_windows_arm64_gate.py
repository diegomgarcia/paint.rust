#!/usr/bin/env python3
"""Run selected frozen-source gates as bounded native Windows ARM64 QGA jobs."""

from __future__ import annotations

from collect_environment import write_json_new
from guest_jobs import run_guest_job
from host_ui import LOCAL_ROOT
import windows_guest_gate as shared

TRIPLE = "aarch64-pc-windows-gnullvm"
HOST_TRIPLE = shared.HOST_TRIPLE
TOOL_ROOT = shared.TOOL_ROOT
TOOLCHAIN = shared.TOOLCHAIN
CARGO = shared.CARGO
DOTNET = shared.DOTNET
LLVM = shared.LLVM
TIMEOUTS = shared.TIMEOUTS
ARCHITECTURE = "native Windows ARM64; Rust host and target aarch64-pc-windows-gnullvm; not Windows x64 and not MSVC ABI coverage"
CONFIG = shared.GateConfig(TRIPLE, "aarch64", ARCHITECTURE)
compact_result = shared.compact_result


def environment(target_dir: str) -> dict[str, str]:
    return shared.environment(CONFIG, target_dir)


def command(step: str, root: str, target_dir: str, smoke_output: str) -> tuple[str, list[str]]:
    return shared.command(CONFIG, step, root, target_dir, smoke_output)


def main(argv: list[str] | None = None) -> int:
    return shared.main(
        argv,
        config=CONFIG,
        description=__doc__,
        local_root=LOCAL_ROOT,
        run_job=run_guest_job,
        write_summary=write_json_new,
    )


if __name__ == "__main__":
    raise SystemExit(main())
