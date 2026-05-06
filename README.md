# Persona

Persona is a new coordination framework for multi-harness AI systems.

The first design target is harness-to-harness messaging: durable messages,
live subscriptions, direct harness delivery, observed output, and explicit
authorization.

The current binary is a NOTA schema stub. With no arguments it emits an example
document:

```sh
cargo run
```

It also accepts inline NOTA or a path to a `.nota` request:

```sh
cargo run -- '(ValidateObject (HarnessRecord operator Operator Terminal "codex"))'
cargo run -- examples/persona-document.nota
```

Start with:

- `ARCHITECTURE.md`
- `reports/2026-05-06-gas-city-harness-design.md`
