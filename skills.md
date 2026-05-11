# persona skill

This is the Persona apex repository and engine-manager home. Work here only
when the change concerns the whole system: supervisor architecture, flake
composition, deployment wiring, or end-to-end tests.

Component implementation belongs in the component repo that owns the behavior:

- `signal-persona` owns the contract for talking to the top-level `persona`
  engine manager.
- `persona-message` owns the NOTA message CLI and harness/human projection.
- `persona-router` owns delivery routing and pending-delivery state.
- `persona-system` owns OS/window/input observation abstractions.
- `persona-harness` owns harness identity, lifecycle, transcripts, and adapter
  contracts.
- `persona-wezterm` owns durable PTY and WezTerm viewer transport.
- `sema` is the typed database library used inside state-bearing components;
  each component owns its own redb handle and transaction-ordering actor for
  its own domain.
- `persona-mind` owns the central state: role coordination,
  activity, memory/work items, dependencies, decisions, aliases,
  and ready-work views.

When adding a component to the system, wire it through `flake.nix` from a
GitHub input and expose its package/check under this repo's outputs. Do not use
`git+file` inputs. Do not copy component source into this repo.

End-to-end tests live here when they require multiple components. Unit and
component integration tests live in the component repo.
