# dragonfruit

Operator-sovereign artifact signing: an ed25519 two-role verify/sign core with a
canonical fixed-width statement format. `#![no_std]`, alloc-free,
`#![forbid(unsafe_code)]`; the same verify core compiles for host, embedded
firmware (RP2350), and UEFI targets.

## Two-role model

A cold **root** key signs `DELEGATION` statements over a **worker** public key:
a purpose, a validity window, and a monotonic counter. The warm worker key signs
`ATTESTATION` statements binding a SHA-256 artifact hash. Verification walks the
explicit chain root → delegation → attestation → artifact hash. Every public
entry point enforces purpose separation and the artifact-hash binding.

Statements are fixed-width canonical byte layouts (`to_canonical` /
`from_canonical` with compile-time lengths) with no DER, no JSON, and no parser
ambiguity surface.

## Verify surface

- `verify_bundle(bundle, root_pubkey, now_unix, artifact_hash, expected_purpose)`
  → sealed `VerifiedArtifact`. The clocked entry point: the delegation window
  is checked against the caller's trusted clock.
- `verify_bundle_no_window(bundle, root_pubkey, artifact_hash, expected_purpose, min_ctr)`
  → sealed `WindowUncheckedArtifact`, a distinct type that cannot convert to
  `VerifiedArtifact`. For verifiers with no trustworthy clock (installers,
  pre-boot loaders): the window check is dropped, and the caller supplies a
  monotonic-counter floor (`min_ctr`) as the anti-rollback lever.
- `verify_bundle_over_bytes` binds a byte slice by hashing it in-crate.

Rejects are deterministic and typed (`VerifyError`), including
`ArtifactHashMismatch`, `UnexpectedPurpose { expected, got }`, and
`CounterRollback`.

## Sign surface

Verify-only by default. `--features sign` adds the software signer
(`sign_delegation` / `sign_attestation`), and compiling it additionally
requires the `sanctioned-signer` feature: a build that has not declared itself
a sanctioned signing context fails at compile time. A deployment that only
consumes artifacts gains no signing capability without that declaration.

## Provenance

Built as the root of trust for an air-gapped signer and the fruit-basket
immutable appliance OS, where it signs and verifies OS images, update
manifests, and backup archives. Developed under multi-round adversarial review
iterated to informational-only findings; the fruit-basket engineering
whitepaper, §12.3 (<https://fruitvibes123.github.io/fruit-basket/>), describes
the methodology.

## License

GPL-2.0-only.
