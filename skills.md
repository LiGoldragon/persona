# persona skill

This is the Persona apex repository. Work here only when the change concerns
the whole system: architecture, flake composition, deployment wiring, or
end-to-end tests.

Component implementation belongs in the component repo that owns the behavior:

- `signal-persona` owns the shared rkyv signal contract.
- `persona-message` owns the NOTA message CLI and harness/human projection.
- `persona-router` owns delivery routing and pending-delivery state.
- `persona-system` owns OS/window/input observation abstractions.
- `persona-harness` owns harness identity, lifecycle, transcripts, and adapter
  contracts.
- `persona-wezterm` owns durable PTY and WezTerm viewer transport.
- `persona-sema` owns typed storage tables and the schema guard; the store
  actor owns durable transaction ordering.
- `persona-orchestrate` owns workspace coordination state.

When adding a component to the system, wire it through `flake.nix` from a
GitHub input and expose its package/check under this repo's outputs. Do not use
`git+file` inputs. Do not copy component source into this repo.

End-to-end tests live here when they require multiple components. Unit and
component integration tests live in the component repo.
