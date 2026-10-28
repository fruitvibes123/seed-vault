# pomegranate

Generic per-key attempt budget: a storage trait with burn semantics,
framework-free.

- `AttemptKind`: a marker trait per use case. It names the key type
  (`Key`), the budget (`BUDGET`), the wire-visible exhaustion status code and
  header (`EXHAUSTION_STATUS_CODE`, `EXHAUSTION_HEADER`), and a log name
  (`NAME`). The same shape fits pairing codes, password-reset attempts,
  or any credential with a bounded guess budget.
- `BudgetStorage<A>`: the trait behind the budget's persistent state. Its two
  methods are split. `check` is read-only: a request guard can run it on
  every request with no side effects, and a successful request burns no budget
  by arriving. `decrement` is called only when an attempt counted (for
  example, after a credential validation failed).
- The router adapter is the consumer's: read `A::Key` from the request, call
  `check`, fail with `A`'s exhaustion status when the budget is gone.

The crate encodes the policy shape and the burn discipline; storage and
framework wiring stay with the consumer. The one dependency is `async-trait`.

Extracted from a production web application (the reference tenant of the
fruit-basket appliance OS), where it bounds pairing-code guesses.

## License

MIT OR Apache-2.0.
