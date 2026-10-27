#!/usr/bin/env python3
"""Detached, bounded QGA guest jobs with logs, exit records, and timeout cleanup."""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
from pathlib import Path, PureWindowsPath
from typing import Any, Mapping, Sequence

from collect_environment import write_json_new
from host_ui import LOCAL_ROOT
from utm import (
    DEFAULT_VM,
    POWERSHELL,
    ReferenceCommandError,
    guest_file_metadata,
    guest_json_script,
    guest_json_source,
    pull_file,
    push_file,
    sha256_file,
)

RUNNER_HOST = Path(__file__).resolve().parent / "guest" / "Invoke-ReferenceJob.ps1"
STOP_RUNNER_HOST = Path(__file__).resolve().parent / "guest" / "Stop-ReferenceJob.ps1"
RUNNER_GUEST = r"C:\PaintR-Reference\tools\Invoke-ReferenceJob-v2.ps1"
JOB_ROOT = PureWindowsPath(r"C:\PaintR-Reference\jobs")
SECRET_KEY = re.compile(r"(?:KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL|COOKIE)", re.IGNORECASE)


def guest_absolute(path: str) -> str:
    value = PureWindowsPath(path)
    if not value.is_absolute() or ".." in value.parts or "\x00" in path:
        raise ValueError(f"unsafe guest absolute path: {path!r}")
    return path


def guest_presence(path: str, vm: str) -> bool:
    result = guest_json_source(
        "[pscustomobject]@{ exists = [bool](Test-Path -LiteralPath $Path) } | ConvertTo-Json -Compress",
        bindings={"Path": path}, vm=vm, timeout=30,
    )
    if not isinstance(result.get("exists"), bool):
        raise ReferenceCommandError(f"guest path presence was not Boolean: {path}")
    return bool(result["exists"])


def ensure_guest_file(host: Path, guest: str, vm: str) -> dict[str, Any]:
    if guest_presence(guest, vm):
        metadata = guest_file_metadata(guest, vm=vm, timeout=30)
        if metadata["sha256"] != sha256_file(host) or metadata["length"] != host.stat().st_size:
            raise ReferenceCommandError(f"existing guest helper differs from owned source: {guest}")
        return {"method": "reused_hash_verified", **metadata}
    return {"method": "pushed_new", **push_file(host, guest, vm=vm, timeout=60)}


def start_guest_job(
    job_id: str,
    executable: str,
    arguments: Sequence[str],
    *,
    working_directory: str,
    environment: Mapping[str, str] | None = None,
    vm: str = DEFAULT_VM,
) -> dict[str, Any]:
    if re.fullmatch(r"[a-z0-9][a-z0-9-]{2,63}", job_id) is None:
        raise ValueError("job-id must be 3..64 lowercase letters/digits/hyphens")
    guest_absolute(executable)
    guest_absolute(working_directory)
    if any("\x00" in argument for argument in arguments):
        raise ValueError("NUL in guest argument")
    env = dict(environment or {})
    for name, value in env.items():
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name) is None:
            raise ValueError(f"invalid environment name: {name!r}")
        if SECRET_KEY.search(name):
            raise ValueError(f"refusing to record secret-like environment key: {name}")
        if "\x00" in value:
            raise ValueError("NUL in environment value")
    host_dir = LOCAL_ROOT / "jobs" / job_id
    host_dir.mkdir(parents=True, exist_ok=False)
    guest_dir = str(JOB_ROOT / job_id)
    guest_json_source(
        "New-Item -ItemType Directory -Path $Path | Out-Null; [pscustomobject]@{ path = $Path } | ConvertTo-Json -Compress",
        bindings={"Path": guest_dir}, vm=vm, timeout=30,
    )
    config = {
        "schema_version": 1,
        "job_id": job_id,
        "executable": executable,
        "arguments": list(arguments),
        "working_directory": working_directory,
        "environment": env,
        "stdout_path": guest_dir + r"\stdout.txt",
        "stderr_path": guest_dir + r"\stderr.txt",
        "wrapper_error_path": guest_dir + r"\wrapper-error.txt",
        "exit_path": guest_dir + r"\exit.json",
    }
    write_json_new(host_dir / "config.json", config)
    push_file(host_dir / "config.json", guest_dir + r"\config.json", vm=vm, timeout=60)
    runner = ensure_guest_file(RUNNER_HOST, RUNNER_GUEST, vm)
    wrapper_out = guest_dir + r"\wrapper-stdout.txt"
    wrapper_err = guest_dir + r"\wrapper-stderr.txt"
    started_guest_path = guest_dir + r"\started-guest.json"
    recovered_launch = False
    try:
        started = guest_json_source(
            r"""
$p = Start-Process -FilePath $PowerShell -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$Runner,'-ConfigPath',$Config) -WorkingDirectory $JobDir -WindowStyle Hidden -RedirectStandardOutput $WrapperOut -RedirectStandardError $WrapperErr -PassThru
$record = [pscustomobject]@{ pid = $p.Id; start_time = $p.StartTime.ToString('o'); wrapper_stdout = $WrapperOut; wrapper_stderr = $WrapperErr }
$json = $record | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($StartedPath + '.tmp', $json, [System.Text.Encoding]::UTF8)
Move-Item -LiteralPath ($StartedPath + '.tmp') -Destination $StartedPath -Force
$json
""",
            bindings={"PowerShell": POWERSHELL, "Runner": RUNNER_GUEST, "Config": guest_dir + r"\config.json", "JobDir": guest_dir, "WrapperOut": wrapper_out, "WrapperErr": wrapper_err, "StartedPath": started_guest_path},
            vm=vm,
            timeout=30,
        )
    except ReferenceCommandError:
        recovered = guest_json_source(
            "$record = $null; if (Test-Path -LiteralPath $Path) { $record = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }; [pscustomobject]@{ process = $record } | ConvertTo-Json -Compress",
            bindings={"Path": started_guest_path}, vm=vm, timeout=60,
        )
        if not isinstance(recovered.get("process"), dict):
            raise
        started = recovered["process"]
        recovered_launch = True
    manifest = {"schema_version": 1, "job_id": job_id, "guest_directory": guest_dir, "host_directory": str(host_dir), "config": config, "runner": runner, "process": started, "started_guest_path": started_guest_path, "recovered_launch": recovered_launch}
    write_json_new(host_dir / "started.json", manifest)
    return manifest


def guest_job_status(job: Mapping[str, Any], vm: str = DEFAULT_VM) -> dict[str, Any]:
    guest_dir = str(job["guest_directory"])
    pid = int(job["process"]["pid"])
    expected_start = str(job["process"]["start_time"])
    result = guest_json_source(
        r"""
$completed = [bool](Test-Path -LiteralPath $ExitPath)
$exit = $null
if ($completed) { $exit = Get-Content -LiteralPath $ExitPath -Raw | ConvertFrom-Json }
$current = $null
$identityMatch = $false
$process = Get-Process -Id $PidValue -ErrorAction SilentlyContinue
if ($null -ne $process) {
    $actualStart = $process.StartTime.ToString('o')
    $actualPath = $process.Path
    $identityMatch = ($actualStart -ceq $ExpectedStart -and $actualPath -ieq $ExpectedExecutable)
    $current = [pscustomobject]@{ pid = $PidValue; name = $process.ProcessName; executable = $actualPath; created = $actualStart }
}
[pscustomobject]@{ completed = $completed; running = $identityMatch; identity_match = $identityMatch; current_process = $current; exit = $exit } | ConvertTo-Json -Depth 5 -Compress
""",
        bindings={"ExitPath": guest_dir + r"\exit.json", "PidValue": str(pid), "ExpectedStart": expected_start, "ExpectedExecutable": POWERSHELL}, vm=vm, timeout=30,
    )
    return result


def stop_guest_job(job: Mapping[str, Any], vm: str = DEFAULT_VM) -> dict[str, Any]:
    pid = int(job["process"]["pid"])
    status = guest_job_status(job, vm)
    if status.get("completed"):
        return {"stopped": False, "reason": "already completed", "identity_match": False}
    if status.get("identity_match") is not True:
        return {"stopped": False, "reason": "original executable/start identity not verified; no process killed", "identity_match": False, "current_process": status.get("current_process")}
    result = guest_json_script(
        STOP_RUNNER_HOST,
        bindings={"PidValue": str(pid), "ExpectedStart": str(job["process"]["start_time"]), "ExpectedExecutable": POWERSHELL, "ExitPath": str(job["guest_directory"]) + r"\exit.json"},
        vm=vm,
        timeout=60,
    )
    result["pid"] = pid
    return result


def collect_guest_job(job: Mapping[str, Any], result: Mapping[str, Any], vm: str = DEFAULT_VM) -> dict[str, Any]:
    host_dir = Path(str(job["host_directory"]))
    guest_dir = str(job["guest_directory"])
    logs: dict[str, Any] = {}
    for name in ("stdout.txt", "stderr.txt", "wrapper-stdout.txt", "wrapper-stderr.txt", "wrapper-error.txt"):
        guest_path = guest_dir + "\\" + name
        if not guest_presence(guest_path, vm):
            logs[name] = {"exists": False}
            continue
        metadata = guest_file_metadata(guest_path, vm=vm, timeout=30)
        host_path = host_dir / name
        if metadata["length"] == 0:
            host_path.write_bytes(b"")
        else:
            pull_file(guest_path, host_path, vm=vm, timeout=60)
        if host_path.stat().st_size != metadata["length"] or sha256_file(host_path) != metadata["sha256"]:
            raise ReferenceCommandError(f"job log hash mismatch: {name}")
        logs[name] = {"exists": True, "host_path": str(host_path), **metadata}
    completed = bool(result.get("completed"))
    exit_data = result.get("exit") if isinstance(result.get("exit"), dict) else None
    timed_out = bool(result.get("timed_out"))
    if timed_out:
        exit_code = 124
    elif exit_data is not None:
        exit_code = int(exit_data["exit_code"])
        write_json_new(host_dir / "exit.json", exit_data)
    else:
        exit_code = 125
    report = {
        "schema_version": 1,
        "job_id": job["job_id"],
        "status": "TIMED_OUT" if timed_out else ("COMPLETED" if completed and exit_data is not None else "MISSING_EXIT"),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "seconds": result.get("seconds"),
        "config": job["config"],
        "process": job["process"],
        "stop": result.get("stop"),
        "observation_errors": result.get("observation_errors", []),
        "exit": exit_data,
        "logs": logs,
    }
    write_json_new(host_dir / "result.json", report)
    return report


def run_guest_job(job_id: str, executable: str, arguments: Sequence[str], *, working_directory: str, environment: Mapping[str, str] | None = None, timeout: int = 1800, vm: str = DEFAULT_VM) -> dict[str, Any]:
    if not 1 <= timeout <= 86400:
        raise ValueError("timeout must be 1..86400 seconds")
    job = start_guest_job(job_id, executable, arguments, working_directory=working_directory, environment=environment, vm=vm)
    started = time.monotonic()
    result: dict[str, Any]
    observation_errors: list[str] = []
    try:
        while True:
            status: dict[str, Any] | None = None
            try:
                status = guest_job_status(job, vm)
            except ReferenceCommandError as error:
                observation_errors.append(str(error))
            if status is not None and (status.get("completed") or not status.get("running")):
                result = status
                break
            elapsed = time.monotonic() - started
            if elapsed >= timeout:
                stop = stop_guest_job(job, vm)
                if stop.get("stopped"):
                    result = {"completed": False, "running": False, "exit": None, "timed_out": True, "stop": stop}
                    break
                if stop.get("reason") == "already completed":
                    completed_status = guest_job_status(job, vm)
                    if completed_status.get("completed"):
                        result = completed_status
                        break
                raise ReferenceCommandError(f"timeout reached but safe cleanup did not terminate the original wrapper: {stop}")
            time.sleep(min(2.0, max(0.1, timeout - elapsed)))
    except Exception:
        try:
            stop_guest_job(job, vm)
        except Exception:
            pass
        raise
    result["observation_errors"] = observation_errors
    result["seconds"] = round(time.monotonic() - started, 3)
    return collect_guest_job(job, result, vm)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--job-id", required=True)
    parser.add_argument("--vm", default=DEFAULT_VM)
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--workdir", required=True)
    parser.add_argument("--set", action="append", default=[], metavar="NAME=VALUE")
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        arguments = list(args.arguments)
        if arguments[:1] == ["--"]:
            arguments = arguments[1:]
        if not arguments:
            parser.error("guest executable is required")
        environment = dict(item.split("=", 1) for item in args.set)
        report = run_guest_job(args.job_id, arguments[0], arguments[1:], working_directory=args.workdir, environment=environment, timeout=args.timeout, vm=args.vm)
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return report["exit_code"] if 1 <= report["exit_code"] <= 120 else (0 if report["exit_code"] == 0 else 1)


if __name__ == "__main__":
    raise SystemExit(main())
