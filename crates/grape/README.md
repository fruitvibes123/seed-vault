# grape

Deterministic, operator-owned derivation of an SSH ed25519 host key.
Published on crates.io as `grape-hostkey` (the bare name was taken); the
library target stays `grape`.

The problem it solves: a recovery environment needs an SSH host key, but a key
generated on the machine is a key the operator cannot verify on first contact
(TOFU). grape derives the host key deterministically from operator-owned
inputs instead, so the operator can precompute the exact host key offline,
pin it in `known_hosts`, and connect to a recovery shell with no
trust-on-first-use step.

Two stages, both pure and deterministic:

- Build time (`derive_seed`): HKDF-SHA256 over the public inputs and the
  operator's master key produces a 32-byte seed that ships in the read-only
  image.
- Runtime (`derive_host_key`): HKDF-SHA256 over that seed, with the image's
  dm-verity root hash as the boot-stable info input, produces the ed25519
  seed and the dropbear host-key wire bytes.

The path-taking functions are the operator-facing form; the byte cores
(`seed_from_inputs` / `host_key_from_seed`) carry the crypto with no
filesystem coupling. HKDF salts are version-tagged (`-v1`), so a construction
change bumps the tag and outputs from different constructions never silently
collide. Pure Rust, no FFI.

Built for the rescue path of the fruit-basket appliance OS, where the
whitepaper's host-identity precompute
(<https://fruitvibes123.github.io/fruit-basket/>) rests on this derivation.

## License

MIT OR Apache-2.0.
