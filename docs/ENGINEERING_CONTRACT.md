# Paint.R engineering and acceptance contract

## Release train

The current target is a verified **0.1.0 development preview**, not a claim of broad Paint.NET parity. The first slice includes a modular document/session core, rectangle selection, solid fill, Invert, Normal layer composition, undo/redo, PNG I/O, a usable desktop shell, a bounded PDN3 compatibility lane, and an isolated plugin-host proof. Features remain `implemented-unverified` until their reference and platform gates pass.

The full compatibility target and months-to-years roadmap remain in `IMPLEMENTATION_PLAN.md`. Speed comes from parallelizing independent, testable modules—not lowering acceptance standards.

## Ownership and merge protocol

The integrator owns root manifests, `Cargo.lock`, CI, shared contracts, merges, and release decisions. Agents own only their assigned paths and must not edit another lane's files.

1. Work in the assigned Git worktree/branch.
2. Write failing behavioral tests before implementation. Keep the test and implementation in the same commit when the red evidence is recorded in the report.
3. Run `cargo fmt`, the strict Clippy command, and every test for owned packages before requesting review.
4. Commit small, compiling increments. Send the integrator an early API commit when another lane depends on it.
5. Report exact commands, test counts, changed files, known gaps, and evidence. Never use “done” for a partial packet.
6. The integrator reruns tests, inspects diffs, scans for stubs, and exercises the integrated product before accepting a completion claim.

No agent may change the root dependency policy, bless reference goldens, mark a parity-ledger entry complete, or hide an exception behind a skipped/ignored test.

## Canonical model

### Core

- Pixels use explicit 8-bit **BGRA** storage with named conversion to/from RGBA.
- Dimensions and rectangle edges are checked; all allocation-size arithmetic is fallible.
- A surface owns a contiguous pixel buffer with defined row stride (`width * 4`) and checked indexing.
- Geometry uses integer coordinates and half-open rectangles (`left..right`, `top..bottom`).
- Errors are typed and actionable. No silent truncation, wraparound, or allocation from unvalidated external dimensions.

### Document

- Documents have a size, ordered layers, an active layer, selection state, and monotonically increasing revision information.
- Layers have stable IDs, Unicode names, visibility, opacity, background state, blend mode, revision, and a surface.
- The first preview implements Normal blending only. Other modes must return explicit unsupported errors until differential tests prove them.
- A pixel patch identifies a layer, expected layer revision, half-open rectangle, before pixels, and after pixels.
- Applying a patch validates dimensions, expected revision, and current before-pixels. Stale or mismatched patches are rejected without mutation.
- Undo/redo uses validated inverse/forward patches, not unbounded whole-document snapshots.

### Raster

- CPU output is authoritative and independent of display GPU, worker count, and tile partitioning.
- Fill and Invert produce validated patches and respect clipping and rectangle selection.
- Invert changes RGB and preserves alpha.
- Normal composition handles visibility, layer opacity, straight/premultiplied-alpha conversion points, rounding, saturation, and transparent pixels through explicit tests.
- Maintain simple scalar reference functions for any optimized implementation. Do not introduce nondeterministic parallelism or fast-math assumptions.

### Session

- `paint-app` is the single writer. UI and plugin workers submit commands or patches; they never mutate document layers directly.
- A UI-neutral command trace can replay the same operations headlessly and through reference automation.
- Undo/redo, selection, dirty state, and active-layer changes are part of the semantic snapshot.
- Failed commands leave the document and history unchanged.

### I/O and PDN3

- PNG helpers convert between canonical BGRA surfaces and RGBA file order, bound decoded allocations, and test malformed/truncated input.
- Atomic-save tests cover success, replacement, simulated write/rename failure, and preservation of the original file. Do not claim durability that the tests do not establish.
- PDN3 is semantic, not byte-identical, compatibility. NRBF data is parsed as data; no file-named type is instantiated.
- Unknown or unsupported PDN fields produce a loss report or explicit error. Never silently discard layers, pixels, or metadata.
- Self-round-trips are necessary but insufficient. Writer acceptance requires the unmodified Paint.NET reference to open and re-save clone output.

### Plugin host

- Production code never loads, probes, copies, or bundles Paint.NET assemblies.
- The Rust broker and .NET host use versioned bounded messages, explicit pixel layouts, request/document generations, cancellation, timeouts, and stale-response rejection.
- Begin with copied buffers. Shared memory is a later measured optimization.
- A source-compiled sample plugin validates transport only; it is not evidence of unmodified Paint.NET binary compatibility.
- Out-of-process execution improves crash containment but is not marketed as a security sandbox.

### UI and usability

- The first UI is an original development shell with self-authored visuals. Do not copy Paint.NET resources or claim screenshot parity.
- Core workflows must be usable without pointer-only interactions: new/open/save, fill, invert, rectangle selection, layer visibility/opacity, undo/redo, and error recovery as applicable.
- Usability tests exercise semantic controls and keyboard flows, not merely widget construction. Headless smoke mode must exercise the real session and exit successfully without opening a window.
- Screenshot regression goldens for the clone are separate from Paint.NET parity goldens.

## Test layers

- **Unit:** exact arithmetic, indexing, state transitions, and error paths.
- **Property:** clipping, round trips, patch invariants, arbitrary dimensions, malformed inputs, and history consistency.
- **Integration:** real command traces across session/document/raster/I/O/plugin boundaries.
- **Usability:** semantic UI actions, keyboard navigation, focus, labels, and recoverable failures.
- **Reference differential:** stock Paint.NET screenshots, decoded pixels, PDN semantics, and plugin results under a pinned configuration.
- **Robustness:** fuzz-like malformed inputs, resource limits, cancellation, stale generations, and process crashes.
- **Three-OS CI:** native build/test/package smoke on Windows x64, macOS ARM64, and Linux x86_64. Cross-compilation is not support.

## Stub and false-completion policy

A completion review rejects, among other things:

- `todo!`, `unimplemented!`, `unreachable!` used as a feature substitute, or placeholder success returns.
- Empty test bodies, `assert!(true)`, deleted negative tests, or ignored acceptance tests without an approved issue and explicit incomplete status.
- Hard-coded golden outputs that do not compute the specified behavior.
- A UI that only draws static screens without executing session commands.
- A codec that only self-round-trips a private format while claiming PDN compatibility.
- A plugin “host” that only launches a process without bounded protocol, output validation, cancellation, and crash handling.
- Claims that reference evidence exists when no artifact/hash/command can be produced.

Scans are supplemental; the integrator must inspect the actual behavior and test output.

## Required completion report

Each work packet ends with:

```text
implemented behavior and exact scope
owned files changed
red-to-green evidence
commands run and observed results
unit/property/integration/usability test counts
dependency and license additions
known gaps and unsupported behavior
remaining reference/platform gates
commit hashes
```

A packet with gaps can be useful and mergeable, but it must be labeled partial and must not be presented as a complete feature.
