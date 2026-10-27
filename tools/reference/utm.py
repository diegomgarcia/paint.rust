#!/usr/bin/env python3
"""Bounded UTM/QGA helpers. No interactive scheduled tasks are used."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

REFERENCE_DIR = Path(__file__).resolve().parent
POWERSHELL = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
DEFAULT_VM = "Windows"
MAX_GUEST_OUTPUT = 8 * 1024 * 1024
# Windows CreateProcess limits the full command line to 32767 UTF-16 characters.
# The fixed PowerShell executable/options consume fewer than 200 characters.
MAX_ENCODED_POWERSHELL_CHARS = 32_000


class ReferenceCommandError(RuntimeError):
    pass


def run_bounded(
    arguments: Sequence[str | os.PathLike[str]],
    *,
    timeout: float,
    max_output_bytes: int = MAX_GUEST_OUTPUT,
) -> subprocess.CompletedProcess[bytes]:
    argv = [os.fspath(argument) for argument in arguments]
    try:
        result = subprocess.run(argv, capture_output=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        raise ReferenceCommandError(f"command exceeded {timeout:g}s: {argv[0]}") from error
    except OSError as error:
        raise ReferenceCommandError(f"cannot execute {argv[0]}: {error}") from error
    if len(result.stdout) > max_output_bytes or len(result.stderr) > max_output_bytes:
        raise ReferenceCommandError(f"command output exceeded {max_output_bytes} bytes: {argv[0]}")
    if result.returncode != 0:
        stdout = result.stdout.decode("utf-8", "replace").strip()
        stderr = result.stderr.decode("utf-8", "replace").strip()
        raise ReferenceCommandError(
            f"command exited {result.returncode}: {argv[0]}\nstdout: {stdout}\nstderr: {stderr}"
        )
    return result


def _decode_applescript_base64(output: bytes) -> bytes:
    encoded = output.strip()
    if not encoded:
        return b""
    if len(encoded) > MAX_GUEST_OUTPUT * 2:
        raise ReferenceCommandError("guest base64 output exceeds the configured bound")
    try:
        return base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        preview = encoded[:200].decode("utf-8", "replace")
        raise ReferenceCommandError(f"UTM returned non-base64 guest output: {preview!r}") from error


def guest_execute(
    arguments: Sequence[str],
    *,
    vm: str = DEFAULT_VM,
    timeout: int = 45,
) -> bytes:
    if not arguments:
        raise ValueError("guest executable and arguments are required")
    if not 1 <= timeout <= 300:
        raise ValueError("timeout must be 1..300 seconds")
    script = REFERENCE_DIR / "guest_exec.applescript"
    argv = ["osascript", str(script), vm, str(timeout), *arguments]
    result = run_bounded(argv, timeout=timeout + 15)
    return _decode_applescript_base64(result.stdout)


def guest_powershell_source(
    source: str,
    *,
    bindings: Mapping[str, str] | None = None,
    vm: str = DEFAULT_VM,
    timeout: int = 60,
) -> bytes:
    prefix = ["$ErrorActionPreference = 'Stop'", "$ProgressPreference = 'SilentlyContinue'"]
    for name, value in (bindings or {}).items():
        if not name or not (name[0].isalpha() or name[0] == "_") or not all(c.isalnum() or c == "_" for c in name):
            raise ValueError(f"invalid PowerShell binding name {name!r}")
        escaped = value.replace("'", "''")
        prefix.append(f"${name} = '{escaped}'")
    script = "\n".join(prefix) + "\n" + source
    encoded = base64.b64encode(script.encode("utf-16-le")).decode("ascii")
    if len(encoded) > MAX_ENCODED_POWERSHELL_CHARS:
        raise ValueError(
            f"encoded PowerShell command is {len(encoded)} characters; "
            f"the safe bound is {MAX_ENCODED_POWERSHELL_CHARS}"
        )
    arguments = [
        POWERSHELL,
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        encoded,
    ]
    return guest_execute(arguments, vm=vm, timeout=timeout)


def guest_script(
    script_path: Path,
    *,
    bindings: Mapping[str, str] | None = None,
    vm: str = DEFAULT_VM,
    timeout: int = 60,
) -> bytes:
    source = Path(script_path).read_text(encoding="utf-8")
    return guest_powershell_source(source, bindings=bindings, vm=vm, timeout=timeout)


def guest_json_script(
    script_path: Path,
    *,
    bindings: Mapping[str, str] | None = None,
    vm: str = DEFAULT_VM,
    timeout: int = 60,
) -> dict[str, Any]:
    output = guest_script(script_path, bindings=bindings, vm=vm, timeout=timeout)
    try:
        value = json.loads(output.decode("utf-8-sig"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ReferenceCommandError(f"guest script did not return JSON: {output[:500]!r}") from error
    if not isinstance(value, dict):
        raise ReferenceCommandError("guest JSON root is not an object")
    return value


def guest_file_metadata(path: str, *, vm: str = DEFAULT_VM, timeout: int = 30) -> dict[str, Any]:
    source = r"""
$item = Get-Item -LiteralPath $Path
if ($item.PSIsContainer) { throw "Path is a directory" }
[pscustomobject]@{
    path = $item.FullName
    length = $item.Length
    last_write_time = $item.LastWriteTime.ToString("o")
    sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json -Compress
"""
    return guest_json_source(source, bindings={"Path": path}, vm=vm, timeout=timeout)


def guest_json_source(
    source: str,
    *,
    bindings: Mapping[str, str] | None = None,
    vm: str = DEFAULT_VM,
    timeout: int = 60,
) -> dict[str, Any]:
    output = guest_powershell_source(source, bindings=bindings, vm=vm, timeout=timeout)
    try:
        value = json.loads(output.decode("utf-8-sig"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ReferenceCommandError(f"guest command did not return JSON: {output[:500]!r}") from error
    if not isinstance(value, dict):
        raise ReferenceCommandError("guest JSON root is not an object")
    return value


def wait_guest_file(path: str, *, vm: str = DEFAULT_VM, timeout: int = 45) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    previous: tuple[int, str] | None = None
    last_error = "not queried"
    while time.monotonic() < deadline:
        try:
            metadata = guest_file_metadata(path, vm=vm, timeout=min(20, timeout))
            if metadata.get("length", 0) > 0:
                current = (int(metadata["length"]), str(metadata["sha256"]))
                if current == previous:
                    return metadata
                previous = current
        except ReferenceCommandError as error:
            last_error = str(error).splitlines()[0]
        time.sleep(1.0)
    raise ReferenceCommandError(f"guest file did not become stable within {timeout}s: {path}; {last_error}")


def push_file(host_path: Path, guest_path: str, *, vm: str = DEFAULT_VM, timeout: int = 45) -> dict[str, Any]:
    """Push a new guest file and verify its content; never overwrite a guest path."""
    host_path = Path(host_path).resolve()
    if not host_path.is_file():
        raise ReferenceCommandError(f"host source does not exist: {host_path}")
    size = host_path.stat().st_size
    if not 0 < size <= 256 * 1024 * 1024:
        raise ReferenceCommandError(f"host source size is outside 1 byte..256 MiB: {size}")
    if not 1 <= timeout <= 300:
        raise ValueError("timeout must be 1..300 seconds")
    presence = guest_json_source(
        "[pscustomobject]@{ exists = [bool](Test-Path -LiteralPath $Path) } | ConvertTo-Json -Compress",
        bindings={"Path": guest_path}, vm=vm, timeout=min(timeout, 30),
    )
    if presence.get("exists") is not False:
        raise ReferenceCommandError(f"refusing to overwrite guest path or unable to prove absence: {guest_path}")
    expected_sha = sha256_file(host_path)
    result = run_bounded(
        ["osascript", str(REFERENCE_DIR / "guest_push.applescript"), vm, guest_path, str(host_path)],
        timeout=timeout + 15,
    )
    if result.stdout.strip() != b"pushed":
        raise ReferenceCommandError(f"unexpected UTM push response: {result.stdout[:200]!r}")
    metadata = guest_file_metadata(guest_path, vm=vm, timeout=min(timeout, 30))
    if metadata.get("length") != size or metadata.get("sha256") != expected_sha:
        raise ReferenceCommandError(f"guest push hash/length mismatch: {guest_path}")
    return metadata


def pull_file(guest_path: str, host_path: Path, *, vm: str = DEFAULT_VM, timeout: int = 45, overwrite: bool = False) -> None:
    host_path = Path(host_path).resolve()
    if host_path.exists() and not overwrite:
        raise ReferenceCommandError(f"refusing to overwrite host file {host_path}")
    if not host_path.parent.is_dir():
        raise ReferenceCommandError(f"host destination directory does not exist: {host_path.parent}")
    created_placeholder = not host_path.exists()
    if created_placeholder:
        host_path.touch()
    try:
        result = run_bounded(
            ["osascript", str(REFERENCE_DIR / "guest_pull.applescript"), vm, guest_path, str(host_path)],
            timeout=timeout,
        )
        if result.stdout.strip() != b"pulled":
            raise ReferenceCommandError(f"unexpected UTM pull response: {result.stdout[:200]!r}")
        if not host_path.is_file() or host_path.stat().st_size == 0:
            raise ReferenceCommandError(f"UTM pull did not create a nonempty file: {host_path}")
    except Exception:
        if created_placeholder and host_path.exists() and host_path.stat().st_size == 0:
            host_path.unlink()
        raise


def guest_png_evidence(path: str, *, vm: str = DEFAULT_VM, timeout: int = 60) -> dict[str, Any]:
    return guest_json_script(
        REFERENCE_DIR / "guest" / "Get-PngEvidence.ps1",
        bindings={"Path": path},
        vm=vm,
        timeout=timeout,
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def utm_inventory(vm: str = DEFAULT_VM) -> dict[str, Any]:
    script = REFERENCE_DIR / "utm_inventory.applescript"
    output = run_bounded(["osascript", str(script), vm], timeout=20).stdout.decode("utf-8").splitlines()
    fields = [
        "utm_version", "vm_id", "backend", "status", "architecture", "memory_mib",
        "cpu_cores", "hypervisor", "uefi", "display_hardware", "dynamic_resolution",
        "native_resolution", "upscaling_filter", "downscaling_filter",
    ]
    if len(output) != len(fields):
        raise ReferenceCommandError(f"unexpected UTM inventory field count {len(output)}: {output!r}")
    result = dict(zip(fields, output, strict=True))
    for key in ("memory_mib", "cpu_cores"):
        result[key] = int(result[key])
    for key in ("hypervisor", "uefi", "dynamic_resolution", "native_resolution"):
        result[key] = {"true": True, "false": False}.get(result[key], result[key])
    result["vm_name"] = vm
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    execute_parser = subparsers.add_parser("guest-exec")
    execute_parser.add_argument("--vm", default=DEFAULT_VM)
    execute_parser.add_argument("--timeout", type=int, default=45)
    execute_parser.add_argument("arguments", nargs=argparse.REMAINDER)
    script_parser = subparsers.add_parser("guest-script")
    script_parser.add_argument("script", type=Path)
    script_parser.add_argument("--set", action="append", default=[], metavar="NAME=VALUE")
    script_parser.add_argument("--vm", default=DEFAULT_VM)
    script_parser.add_argument("--timeout", type=int, default=60)
    push_parser = subparsers.add_parser("push")
    push_parser.add_argument("host_path", type=Path)
    push_parser.add_argument("guest_path")
    push_parser.add_argument("--vm", default=DEFAULT_VM)
    push_parser.add_argument("--timeout", type=int, default=45)
    pull_parser = subparsers.add_parser("pull")
    pull_parser.add_argument("guest_path")
    pull_parser.add_argument("host_path", type=Path)
    pull_parser.add_argument("--vm", default=DEFAULT_VM)
    pull_parser.add_argument("--timeout", type=int, default=45)
    inventory_parser = subparsers.add_parser("inventory")
    inventory_parser.add_argument("--vm", default=DEFAULT_VM)
    args = parser.parse_args(argv)
    try:
        if args.command == "guest-exec":
            guest_arguments = list(args.arguments)
            if guest_arguments[:1] == ["--"]:
                guest_arguments = guest_arguments[1:]
            output = guest_execute(guest_arguments, vm=args.vm, timeout=args.timeout)
            sys.stdout.buffer.write(output)
        elif args.command == "guest-script":
            bindings = dict(item.split("=", 1) for item in args.set)
            output = guest_script(args.script, bindings=bindings, vm=args.vm, timeout=args.timeout)
            sys.stdout.buffer.write(output)
        elif args.command == "push":
            push_file(args.host_path, args.guest_path, vm=args.vm, timeout=args.timeout)
            print(args.guest_path)
        elif args.command == "pull":
            pull_file(args.guest_path, args.host_path, vm=args.vm, timeout=args.timeout)
            print(args.host_path)
        else:
            print(json.dumps(utm_inventory(args.vm), indent=2, sort_keys=True))
    except (ReferenceCommandError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
