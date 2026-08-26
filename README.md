# Seed Vault

The durable, reusable crates of a self-hosted server ecosystem: six permissive security cores,
four permissive support crates, and the copyleft artifact-signing core. This repository is the
library sibling of [fruit-basket](https://github.com/fruitvibes123/fruit-basket), the immutable
appliance OS that consumes several of these crates by path dependency.

> [!WARNING]
> Every line of code and documentation in this repository is LLM output, written end to end by
> Claude (Anthropic) under the direction and review of a human operator. Read and reuse it with
> that provenance in mind.

## Crates

| Crate | License | Role |
|---|---|---|
| `coconut`, `lychee`, `grapefruit`, `pomegranate`, `mangosteen`, `durian` | MIT OR Apache-2.0 | the six security cores (guard and enforcement primitives) |
| `pomelo` | MIT OR Apache-2.0 | typed scope-ownership request guard: loads by id and verifies ownership, compile-time IDOR-safe |
| `fig` | MIT OR Apache-2.0 | typed download responder |
| `banana` | MIT OR Apache-2.0 | HTTP byte-range parser + mtime-keyed BLAKE3 ETag cache |
| `grape` | MIT OR Apache-2.0 | HKDF key-derivation core (deterministic SSH host keys, build seeds) |
| `dragonfruit` | GPL-2.0-only | the artifact-signing and verify core: `#![no_std]`, alloc-free, ed25519 delegation cascade with purpose binding and a monotonic counter |
| `dragonfruit-nostd-gate`, `dragonfruit-noalloc-gate` | GPL-2.0-only | standing gates that the trimmed firmware config verifies correctly and stays alloc-free |

Verification in the canonical repository runs fmt + clippy + the workspace tests + the
dragonfruit sign/no_std/alloc-freedom gate rungs + the crypto sentry.

## On crates.io

Six crates are published (in-tree names in parentheses where the registry name differs; the
library target keeps the in-tree name, so `use` paths are unchanged):

| crates.io | In-tree | |
|---|---|---|
| [`dragonfruit`](https://crates.io/crates/dragonfruit) | `dragonfruit` | [docs.rs](https://docs.rs/dragonfruit) |
| [`pomelo-guard`](https://crates.io/crates/pomelo-guard) | `pomelo` | [docs.rs](https://docs.rs/pomelo-guard) |
| [`coconut-fs`](https://crates.io/crates/coconut-fs) | `coconut` | [docs.rs](https://docs.rs/coconut-fs) |
| [`pomegranate`](https://crates.io/crates/pomegranate) | `pomegranate` | [docs.rs](https://docs.rs/pomegranate) |
| [`durian-throttle`](https://crates.io/crates/durian-throttle) | `durian` | [docs.rs](https://docs.rs/durian-throttle) |
| [`grape-hostkey`](https://crates.io/crates/grape-hostkey) | `grape` | [docs.rs](https://docs.rs/grape-hostkey) |

The rest (lychee, grapefruit, mangosteen, banana, fig, the gates) build from this repository as
path dependencies and are not on the registry.

## About this repository

This is the published export of a private canonical repository, produced by the same
equivalence-gated release tool as fruit-basket:

- **Working comments are padded out, not deleted.** Internal non-doc comments are replaced with
  same-length whitespace so published source stays byte-faithful to what was reviewed; runs of
  trailing spaces and blank-looking lines are that padding. Doc comments publish as written.
- **Every release commit carries its proof.** The commit message records the canonical commit id
  and a gate witness list: per-file token-stream identity, doc-comment byte-identity,
  per-workspace compile checks, and a content tripwire over every published byte.
- Extracted from a private monorepo in June 2026; the pre-split history stays in the private
  archive.

## License

Mixed, by crate:

- **`MIT OR Apache-2.0`** (the standard Rust dual) for the reusable permissive cores — coconut, lychee, grapefruit, pomegranate, mangosteen, durian, pomelo, fig, banana, and grape. A GPLv2 consumer may take these under the MIT arm.
- **`GPL-2.0-only`** for the signer core `dragonfruit` and its `dragonfruit-nostd-gate` — see [`COPYING`](COPYING). The artifact-signing core is the sovereign root of trust, kept copyleft against capture. GPLv2 (not v3/AGPL, to match the box's verified-boot chain), `-only` not `-or-later`.
