# durian

Bounded in-RAM sliding-window failure store with exponential soft-lock
backoff. Published on crates.io as `durian-throttle` (the bare name was
taken); the library target stays `durian`.

One `ThrottleStore<K>` is one throttle bucket: a mutex-wrapped LRU cache from
key to failure timestamps. The structural properties are all load-bearing
against attacker-supplied keys:

- Bounded entry count: the LRU capacity caps total entries, so spraying
  distinct keys (usernames, IPs) churns the LRU instead of growing memory.
- Bounded per-key history: each key's timestamp list is capped; the soft-lock
  delay saturates anyway, so extra timestamps would add memory pressure
  without signal.
- Sliding-window counting: `current_delay` counts only timestamps fresher
  than the caller's window and prunes stale ones in place. Reads use
  `peek_mut` (no recency bump) so idle entries age out; writes bump recency
  so an active attacker keeps their own entry hot until the window expires.
- The backoff curve: over-budget counts map to delays of 1, 2, 4, 8, 16, then
  a 30 s cap. Soft-lock (delay, then proceed) rather than hard rejection, so
  a legitimate user who fat-fingered a password is slowed, not locked out.

The one dependency is `lru`.

Extracted from a production web application (the reference tenant of the
fruit-basket appliance OS), where it throttles login and pairing failures.

## License

MIT OR Apache-2.0.
