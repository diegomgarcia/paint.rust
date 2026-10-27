# Active work packets

All agents follow `ENGINEERING_CONTRACT.md`. The integrator owns everything not explicitly assigned below.

## A — core, document, raster

- **Paths:** `crates/core`, `crates/document`, `crates/raster`
- **Scope:** checked BGRA surfaces/geometry; layers and revisions; selection; validated forward/inverse patches; fill, Invert, Normal composition; unit/property tests.
- **Early integration output:** one compiling API commit so the app/UI lane can start.
- **Not accepted as complete:** hard-coded examples, unchecked dimensions, stale-patch mutation, or unsupported blend modes returning success.

## B — PNG and PDN3

- **Paths:** `crates/image-io`, `crates/pdn3`
- **Scope:** bounded PNG encode/decode; atomic output with fault tests; bounded PDN3 container/NRBF reading and as much real semantic writer coverage as can be validated; malformed-input/property tests.
- **Reference output:** explicit corpus needs and semantic comparison tooling for the integrator/reference lane.
- **Not accepted as complete:** self-round-trip-only PDN claims, dropped metadata, unbounded allocations, or a writer that has not been submitted for stock Paint.NET acceptance.

## C — plugin bridge and .NET host

- **Paths:** `crates/plugin-bridge`, `plugin-host`
- **Scope:** versioned framed IPC; lifecycle/timeout/cancellation/stale-response handling; clone-owned SDK and a real source-compiled sample effect; .NET tests and Rust transport integration; binary API inventory tooling for a named candidate.
- **Not accepted as complete:** Paint.NET DLL dependencies, source compatibility marketed as binary compatibility, unbounded stdio, missing crash/timeout tests, or placeholder effects.

## D — reference automation and evidence

- **Paths:** `tools/reference`, `docs/work-packets/reference`, assigned reference artifacts only
- **Scope:** durable UTM/QGA helpers; pinned environment inventory; repeatable A/A PNG observations; UI/window evidence; authentic PDN fixtures when practical; reproducible commands and hashes.
- **Exclusive resource:** the `Windows` UTM VM. Other lanes must not operate it concurrently.
- **Not accepted as complete:** invented baselines, screenshots without environment manifests, one-off undocumented GUI actions, or reference code loaded into the product.

## E — verification and release engineering

- **Paths:** `xtask`, `tools/verify`, `docs/work-packets/quality`
- **Scope:** cross-platform verification runner, stub/suspicious-test scan, dependency/test inventory, artifact checks, release-smoke orchestration that can be extended by the integrator.
- **Not accepted as complete:** a scanner that always passes, tests of only the scanner’s happy path, or suppressing legitimate warnings to obtain green output.

## F — application session and UI (starts from A’s early API commit)

- **Paths:** `crates/app`, `crates/ui`
- **Scope:** single-writer session, command replay, undo/redo and dirty state; real desktop interactions for the preview; semantic usability tests and headless smoke.
- **Not accepted as complete:** a second mutation model inside the UI, static mock screens, pointer-only workflows, or usability tests that do not execute the UI.

## Integration queue

1. A early API → start F.
2. E verification runner → gate every later merge.
3. A complete core/raster + F app/session → integrated history/raster tests.
4. B PNG → integrated open/save and failure-path tests.
5. C host/broker → real cross-language protocol and crash/cancel tests.
6. F UI → usability and package smoke on the available host, followed by CI review.
7. B PDN3 + D authentic fixtures → stock Paint.NET differential acceptance.
8. Update the parity ledger and produce only the release level actually supported by evidence.
