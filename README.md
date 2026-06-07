# Persona

Persona is the engine manager and integration repository for the multi-harness
AI system.

It supervises the Persona component ecosystem, wires the component
repositories together through Nix, and keeps the high-level architecture
visible. Component implementation belongs in the component repos:

- `owner-signal-persona` for the engine-manager contract;
- `sema-engine` for typed database-operation support inside state-bearing components;
- `router` for delivery routing;
- `system` for OS and window-manager observations;
- `harness` for harness actors;
- `message` for the NOTA CLI boundary.

The current binary is a minimal NOTA client over the in-process engine-manager
stub. With no arguments it queries engine status:

```sh
cargo run --bin persona
```

It also accepts inline NOTA or a path to a `.nota` request:

```sh
cargo run --bin persona -- '(ComponentStatusQuery ([persona-router]))'
cargo run --bin persona -- examples/engine-status.nota
```

Start with:

- `ARCHITECTURE.md`
- `reports/2026-05-06-gas-city-harness-design.md`
