# Release status

**Candidate:** Paint.R 0.1.0 internal preview  
**Source:** `0732502311e7a28e0e72603b888846259d162248`  
**Product:** `3090c18b6260cffa5ecd3b1fe92d94952bd6fbb7`  
**Result:** internal candidate passes the demonstrated macOS and emulated Linux gates; **public release is blocked**.

This document is a go/no-go summary, not a release announcement. Machine-readable evidence is in
[`2026-10-27-release-status-0732502.json`](../artifacts/validation/2026-10-27-release-status-0732502.json).

## Gate matrix

| Gate | Status | Evidence or blocker |
|---|---|---|
| Rust workspace format, Clippy, tests | **PASS** | 219 tests across 40 suite records; strict workspace Clippy |
| Deterministic macOS package | **PASS** | Two-run equality; extracted `.app` live native gate exit 0 |
| Development code signature | **PASS** | Apple Development identity; `codesign --verify --strict` valid; signed app live gate exit 0 |
| Gatekeeper distribution assessment | **REJECTED** | Apple Development is not a Developer ID signature; app is not notarized |
| Linux x86_64 build/package | **PASS, EMULATED** | Lima ARM64 VZ + Rosetta; 219 tests; no native x86_64 hardware |
| Native Linux window/input | **NOT RUN** | No native Linux desktop gate |
| Native Windows ARM64/x64 | **BLOCKED** | UTM guest RPC times out (`-2700`) before guest execution |
| Hosted three-OS CI | **NOT CONFIGURED** | No git remote and no authenticated GitHub CLI |
| First-party project license | **PASS** | MIT `LICENSE`; all workspace packages declare MIT |
| Third-party notices | **INCOMPLETE** | 447/457 packages with texts; 50 curated; 10 irreducible gaps |
| Third-party/asset legal clearance | **NOT PROVIDED** | MIT does not relicense dependencies or bundled assets |
| Signing/notarization for distribution | **NOT RUN** | No Developer ID Application identity/notary submission |
| Public release | **BLOCKED** | Package tool continues to refuse public distribution |

## Artifact identities

| Artifact | SHA-256 |
|---|---|
| macOS archive | `7abed7bcd838df50824d0ef3c7a806def4260fea1542879d91e2e81dd155f256` |
| macOS binary | `148add8ef907fdba8a3f37b75de4c452b74d3e31fbaf0382f280cfa4fd785dc8` |
| Development-signed binary | `6d183e2b0b6ce2b110a78515f49bdd89d92fd2d11a60d7d2a91c947fd6110207` |
| Linux x86-64 emulated archive | `983b61046738282269aa4dd28d67e4aee06fb00d4c7bd333dd7a58f268b9da07` |
| Linux x86-64 emulated binary | `ae0244c2f8875a42c177ba960b75b4567a211fb7df06dc3ee02735d4797ff6bf` |

The development signature has CDHash `f27e2eb7c81bd89cc172b4ddb4de60497bae1649`.
It proves bundle-signature integrity and runtime compatibility only. `spctl` rejects it, as expected.

## Remaining blockers

1. Ten notice gaps: seven egui-family packages with an unavailable exact commit, two `r-efi`
   versions with no exact-root license resource, and one `dispatch` version with HTTP-only locked
   repository metadata. No forbidden substitution was used.
2. Third-party/asset legal clearance for redistribution; project MIT licensing is resolved.
3. Developer ID Application signing and notarization.
4. Windows native execution and native Windows packaging.
5. Hosted three-OS CI.
6. Native Linux x86_64 hardware and native Linux window/input gates.
7. Broader reference-driven tool and visual parity. Implemented shapes follow Paint.NET's
   interaction hierarchy, but no exact pixel-parity or full tool-parity claim is made.
