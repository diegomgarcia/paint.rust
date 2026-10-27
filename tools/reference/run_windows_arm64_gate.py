#!/usr/bin/env python3
"""Run selected frozen-source gates as bounded native Windows ARM64 QGA jobs."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

from collect_environment import write_json_new
from guest_jobs import guest_absolute, run_guest_job
from host_ui import LOCAL_ROOT

COMMIT = "8c6d0c2ca3813efdce110be81f098dab1f5cdb30"
TRIPLE = "aarch64-pc-windows-gnullvm"
TOOL_ROOT = r"C:\PaintR-Reference\tools"
TOOLCHAIN = TOOL_ROOT + r"\rustup\toolchains\1.95.0-aarch64-pc-windows-gnullvm"
CARGO = TOOLCHAIN + r"\bin\cargo.exe"
DOTNET = TOOL_ROOT + r"\dotnet\dotnet.exe"
LLVM = TOOL_ROOT + r"\llvm-mingw\bin"
TIMEOUTS = {
    "fmt": 300,
    "clippy": 1800,
    "test": 1800,
    "xtask": 1800,
    "release": 1800,
    "dotnet-restore": 600,
    "dotnet-build": 600,
    "dotnet-test": 900,
    "conformance": 600,
    "smoke": 300,
}


def environment(target_dir: str) -> dict[str, str]:
    return {
        "RUSTUP_HOME": TOOL_ROOT + r"\rustup",
        "CARGO_HOME": TOOL_ROOT + r"\cargo",
        "RUSTUP_TOOLCHAIN": "1.95.0-aarch64-pc-windows-gnullvm",
        "CARGO": CARGO,
        "RUSTC": TOOLCHAIN + r"\bin\rustc.exe",
        "RUSTDOC": TOOLCHAIN + r"\bin\rustdoc.exe",
        "RUSTFMT": TOOLCHAIN + r"\bin\rustfmt.exe",
        "CARGO_TARGET_DIR": target_dir,
        "CARGO_BUILD_TARGET": TRIPLE,
        "CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER": LLVM + r"\aarch64-w64-mingw32-clang.exe",
        "CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_AR": LLVM + r"\llvm-ar.exe",
        "CC_aarch64_pc_windows_gnullvm": LLVM + r"\aarch64-w64-mingw32-clang.exe",
        "AR_aarch64_pc_windows_gnullvm": LLVM + r"\llvm-ar.exe",
        "PATH": ";".join([TOOLCHAIN + r"\bin", LLVM, TOOL_ROOT + r"\dotnet", TOOL_ROOT + r"\node", r"C:\Windows\System32", r"C:\Windows"]),
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


def command(step: str, root: str, target_dir: str, smoke_output: str) -> tuple[str, list[str]]:
    solution = root + r"\plugin-host\Paint.PluginHost.sln"
    if step == "fmt":
        return CARGO, ["fmt", "--all", "--", "--check"]
    if step == "clippy":
        return CARGO, ["clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"]
    if step == "test":
        return CARGO, ["test", "--workspace", "--all-features", "--locked"]
    if step == "xtask":
        return CARGO, ["run", "-p", "xtask", "--locked", "--", "verify"]
    if step == "release":
        return CARGO, ["build", "-p", "paint-ui", "--release", "--locked", "--target", TRIPLE]
    if step == "dotnet-restore":
        return DOTNET, ["restore", solution, "--locked-mode"]
    if step == "dotnet-build":
        return DOTNET, ["build", solution, "--configuration", "Release", "--no-restore"]
    if step == "dotnet-test":
        return DOTNET, ["test", solution, "--configuration", "Release", "--no-build"]
    if step == "conformance":
        host_dll = root + r"\plugin-host\src\Paint.PluginHost\bin\Release\net10.0\Paint.PluginHost.dll"
        return CARGO, ["run", "--locked", "-p", "paint-plugin-bridge", "--bin", "paint-plugin-conformance", "--", host_dll, DOTNET]
    if step == "smoke":
        binary = target_dir + "\\" + TRIPLE + r"\release\paint-ui.exe"
        trace = root + r"\tests\e2e\traces\invert-800x600.json"
        return binary, ["--headless-smoke", "--trace", trace, "--output-dir", smoke_output]
    raise ValueError(f"unknown step: {step}")


def compact_result(report: dict[str, Any]) -> dict[str, Any]:
    return {
        "job_id": report["job_id"],
        "status": report["status"],
        "exit_code": report["exit_code"],
        "timed_out": report["timed_out"],
        "seconds": report["seconds"],
        "started_at": (report.get("exit") or {}).get("started_at"),
        "ended_at": (report.get("exit") or {}).get("ended_at"),
        "command": {"executable": report["config"]["executable"], "arguments": report["config"]["arguments"]},
        "observation_error_count": len(report.get("observation_errors", [])),
        "logs": report["logs"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-commit", default=COMMIT)
    parser.add_argument("--root", default=r"C:\PaintR-Reference\final-src")
    parser.add_argument("--target-dir", default=r"C:\PaintR-Reference\target-final-gnullvm")
    parser.add_argument("--smoke-output", default=r"C:\PaintR-Reference\evidence\final-headless")
    parser.add_argument("--prefix", default="final8c6d")
    parser.add_argument("--steps", required=True, help="comma-separated ordered step names")
    parser.add_argument("--summary", required=True, help="new JSON filename under artifacts/reference-local/harness-final")
    args = parser.parse_args(argv)
    try:
        if re.fullmatch(r"[0-9a-f]{40}", args.source_commit) is None:
            raise ValueError("source-commit must be a full lowercase Git commit hash")
        guest_absolute(args.root)
        guest_absolute(args.target_dir)
        guest_absolute(args.smoke_output)
        if re.fullmatch(r"[a-z0-9][a-z0-9-]{2,40}", args.prefix) is None:
            raise ValueError("unsafe job prefix")
        if re.fullmatch(r"[a-z0-9][a-z0-9-]{0,60}\.json", args.summary) is None:
            raise ValueError("summary must be a new simple JSON filename")
        steps = args.steps.split(",")
        if not steps or any(step not in TIMEOUTS for step in steps) or len(set(steps)) != len(steps):
            raise ValueError(f"invalid unique step list; choose from {', '.join(TIMEOUTS)}")
    except ValueError as error:
        parser.error(str(error))
    summary_path = LOCAL_ROOT / "harness-final" / args.summary
    results: list[dict[str, Any]] = []
    status = "PASS"
    error_text = None
    for step in steps:
        executable, arguments = command(step, args.root, args.target_dir, args.smoke_output)
        job_id = f"{args.prefix}-{step}"
        print(f"START {job_id}: {executable} {' '.join(arguments)}", flush=True)
        try:
            report = run_guest_job(job_id, executable, arguments, working_directory=args.root, environment=environment(args.target_dir), timeout=TIMEOUTS[step])
            results.append(compact_result(report))
            print(f"END {job_id}: exit={report['exit_code']} seconds={report['seconds']}", flush=True)
            if report["exit_code"] != 0:
                status = "FAIL"
                break
        except Exception as error:
            status = "ERROR"
            error_text = f"{type(error).__name__}: {error}"
            print(f"ERROR {job_id}: {error_text}", flush=True)
            break
    summary = {
        "schema_version": 1,
        "status": status,
        "source_commit": args.source_commit,
        "architecture": "native Windows ARM64; Rust host and target aarch64-pc-windows-gnullvm; not Windows x64 and not MSVC ABI coverage",
        "source_root": args.root,
        "target_dir": args.target_dir,
        "selected_steps": steps,
        "executed_steps": [result["job_id"] for result in results],
        "results": results,
        "infrastructure_error": error_text,
    }
    write_json_new(summary_path, summary)
    print(json.dumps(summary, indent=2), flush=True)
    return 0 if status == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
