# persona skill

This is the Persona apex repository and engine-manager home. Work here only
when the change concerns the whole system: privileged manager architecture,
multi-engine supervision, flake composition, deployment wiring, or
end-to-end tests.

The `persona` daemon is the host-level engine manager: one privileged
`persona` system-user daemon per host supervising multiple engine instances.
It owns the manager catalog, per-engine socket/state allocation, connection
classification, and inter-engine route declarations. Component repos own the
behavior inside each engine.

Component implementation belongs in the component repo that owns the behavior:

- `signal-persona` owns the contract for talking to the top-level `persona`
  engine manager: engine catalog, component lifecycle, connection class, and
  inter-engine route records.
- `persona-message` owns the NOTA message CLI and harness/human projection.
- `persona-router` owns delivery routing and pending-delivery state.
- `persona-system` owns OS/window/input observation abstractions.
- `persona-harness` owns harness identity, lifecycle, transcripts, and adapter
  contracts.
- `persona-terminal` owns durable PTY/session transport around
  `terminal-cell`; viewer implementations stay adapter-local inside that
  owner, not separate terminal-brand component repos.
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
