# Isolated plugin host (protocol v1)

This directory contains a clone-owned .NET SDK, a binary wire protocol, a console host, and a source-compiled `Invert` sample. `crates/plugin-bridge` contains the Rust supervisor and independent implementation of the same protocol.

**Scope:** transport and crash containment only. The sample is compiled from source against `ICloneEffect`; it is not evidence that an unmodified Paint.NET effect DLL is compatible. No Paint.NET assemblies, WinForms, WPF, or System.Drawing are referenced. Out-of-process execution is not a security sandbox. Buffers are copied; there is no shared-memory path.

## Wire format

All integers are little-endian. Every frame has this 16-byte header:

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | 4 bytes | ASCII `PRBR` |
| 4 | `u16` | protocol version, exactly 1 |
| 6 | `u8` | message kind |
| 7 | `u8` | flags, exactly 0 |
| 8 | `u32` | payload byte length |
| 12 | 4 bytes | reserved, exactly zero |

The declared length is checked before payload allocation/read. Maximum payload is **67,108,928 bytes** (64 MiB BGRA data plus 64 bytes of metadata). Malformed magic/version/kind/flags/reserved bytes, truncation, invalid UTF-8, invalid dimensions, excess metadata, and trailing bytes are errors.

Message kinds and payloads:

| Kind | Tag | Payload after fixed fields |
| --- | --- | --- |
| Apply | 1 | `request_id:u64`, `generation:u64`, `effect_len:u16`, `width:u32`, `height:u32`, UTF-8 effect, BGRA bytes |
| Cancel | 2 | `request_id:u64` |
| ApplyResult | 3 | `request_id:u64`, `generation:u64`, `width:u32`, `height:u32`, BGRA bytes |
| Error | 4 | `request_id:u64`, `generation:u64`, `code:u16`, reserved `u16=0`, `message_len:u32`, UTF-8 message |
| Hello | 5 | `request_id:u64` |
| Ready | 6 | `request_id:u64`, `effect_count:u16`, reserved `u16=0`, then (`name_len:u16`, UTF-8 name) entries |
| Shutdown | 7 | `request_id:u64` |
| ShutdownAck | 8 | `request_id:u64` |

Request IDs are nonzero and monotonically allocated by the broker. Effect names are 1–64 UTF-8 bytes, errors 1–4096 bytes, and Ready advertises 1–64 effects. Error codes are InvalidRequest=1, UnsupportedEffect=2, StaleGeneration=3, Cancelled=4, Internal=5.

Images have nonzero dimensions and exactly `width * height * 4` bytes, with checked arithmetic and a 64 MiB maximum. Rows are top-down with `stride = width * 4`; each pixel is **B, G, R, straight A**. v1 effects preserve request dimensions. `Invert` subtracts B/G/R from 255 and leaves every alpha byte unchanged.

## Lifecycle semantics

- Startup requires Hello/Ready within a bounded startup window (minimum five seconds, or the configured request timeout if longer).
- Applies carry the current document generation. Older requests and mismatched responses are rejected; the .NET host also rejects generations older than the newest dispatched generation.
- Up to 64 active/cancelled requests are tracked. The writer queue holds one command and the reader queue holds eight decoded messages, providing backpressure alongside per-frame bounds.
- Cancellation sends Cancel and returns `Cancelled` locally. One matching late result/error is consumed but never delivered as current output. The console host processes applies on worker tasks so cancellation can interrupt a running operation.
- Request and blocked-write timeouts are fail-stop: the broker kills and reaps the child so late output cannot contaminate another request. EOF/crash and malformed child output are fatal to that connection.
- Shutdown cancels outstanding work, waits for ShutdownAck, and waits for process exit within the deadline. `Drop` force-kills/reaps any remaining child. Child stderr is not a protocol channel.

## Reproducible commands

Run from the repository root with the required pinned toolchains:

```sh
# Ensure companion tools (clippy-driver and rustdoc) match pinned Cargo/rustc.
PINNED_CARGO=$(rustup which cargo --toolchain 1.95.0)
export PATH="${PINNED_CARGO%/*}:$PATH"

./tools/cargo-pinned.sh fmt -p paint-plugin-bridge -- --check
./tools/cargo-pinned.sh clippy -p paint-plugin-bridge --all-targets -- -D warnings
./tools/cargo-pinned.sh test -p paint-plugin-bridge --all-features

dotnet restore plugin-host/Paint.PluginHost.sln --locked-mode
dotnet build plugin-host/Paint.PluginHost.sln --configuration Release --no-restore
dotnet test plugin-host/Paint.PluginHost.sln --configuration Release --no-build

# Explicit CROSS-LANGUAGE INTEGRATION LANE; no environment-dependent skips:
plugin-host/test-cross-language.sh
```

The lane executes eight fatal-on-failure checks against the real .NET DLL: valid output, stale generation, cancellation/late-response handling, timeout, forced crash, malformed frame, oversized declaration, and wrong version. The Rust test suite additionally uses a real Rust fixture child for exit-code and malformed-output checks. The .NET xUnit suite independently launches the real console host; it does not skip when the DLL is absent.

## Static binary inventory utility

For a **named third-party candidate DLL only**, the optional utility reads CLI metadata without `Assembly.Load`, execution, or decompilation:

```sh
dotnet run --project plugin-host/src/Paint.Plugin.BinaryInventory -- \
  --assembly /absolute/path/to/candidate.dll
```

It emits SHA-256, public type names, and public method names. Paint.NET-named inputs are refused before file access. No candidate was supplied or inspected in this worktree, and inventory output is not a compatibility verdict.

## Known limits

Only macOS ARM64 was exercised locally. Windows/Linux and three-OS CI remain integration gates. v1 does not load Paint.NET binaries, provide Paint.NET API shims, resize through effects, share memory, or prove binary/source compatibility beyond the clone-owned sample shown here.
