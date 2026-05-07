# Persona

Persona is the integration repository for the multi-harness AI system.

It wires the component repositories together through Nix and keeps the
high-level architecture visible. Component implementation belongs in the
component repos:

- `persona-signal` for the shared rkyv frame contract;
- `persona-store` for the durable database and transaction boundary;
- `persona-router` for delivery routing;
- `persona-system` for OS and window-manager observations;
- `persona-harness` for harness actors;
- `persona-message` for the NOTA CLI boundary.

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
