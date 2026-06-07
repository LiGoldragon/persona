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

- `owner-signal-persona` currently owns the contract for talking to the
  top-level `persona` engine manager: engine catalog, component lifecycle,
  connection class, and inter-engine route records. This is the policy-signal
  leg that will follow the workspace meta-signal naming track.
- `message` owns the NOTA message CLI and harness/human projection.
- `router` owns delivery routing and pending-delivery state.
- `system` owns OS/window/input observation abstractions.
- `harness` owns harness identity, lifecycle, transcripts, and adapter
  contracts.
- `terminal` owns durable PTY/session transport around
  `terminal-cell`; viewer implementations stay adapter-local inside that
  owner, not separate terminal-brand component repos.
- `sema-engine` is the database-operation boundary used inside state-bearing
  components; components own `.sema` stores through engine objects, not raw
  storage handles.
- `mind` owns central mind state: memory/work items, typed thoughts,
  relations, dependencies, decisions, aliases, subscriptions, choreography
  policy, and ready-work views.
- `orchestrate` owns ordinary role claims, handoffs, role activity, and
  the orchestration machinery that carries out mind-authorized work.

When adding a component to the system, wire it through `flake.nix` from a
GitHub input and expose its package/check under this repo's outputs. Do not use
`git+file` inputs. Do not copy component source into this repo.

End-to-end tests live here when they require multiple components. Unit and
component integration tests live in the component repo.

## Manager state — event log is the truth

`manager.engine-events` is append-only. The two snapshot tables
(`engine-lifecycle-snapshot`, `engine-status-snapshot`) are reducer
projections over that log; they are acceleration for reads, never truth in
their own right. Writes follow one shape only:

- Build an `EngineEventDraft`.
- Send `AppendEngineEvent` to the `ManagerStore` actor.
- The actor stamps the next sequence, runs the reducer, writes the event row
  and both snapshot rows in **one sema-engine storage transaction**.

Reading flows the opposite way:

- `ComponentStatusQuery` / `EngineStatusQuery` read the status snapshot
  through `EngineManager`.
- Audit and recovery paths walk the event log directly through
  `ReadEngineEvents`.

Two implications for agents working in this repo:

- **Never write a snapshot row directly.** No bypass to set a status; if you
  need a `Running` health row, you append the `ComponentReady` event that
  reduces to it. The reducer is the only legal writer of snapshot rows.
- **Deleting a snapshot table is recoverable.** `ManagerStore::open` replays
  the event log into the snapshots on every start. A test that wants to
  prove "snapshots rebuild from the event log" deletes the snapshot table
  before opening a fresh `ManagerStore` and asserts the snapshot returns.

## Child supervision — no polling

Child-exit observation is push, never poll. `DirectProcessLauncher` owns
one watcher tokio task per launched child; that task awaits `child.wait()`
and pushes either a `StopComponentReceipt` (manager-initiated stop) or a
`ChildProcessExited` (natural exit) into the launcher's mailbox. The
launcher's mailbox never holds a `child.wait()` future; the stop path
never blocks the mailbox on a child.

Startup-time domain-socket reachability is a **bounded reachability probe**
carrying the `ESSENCE.md` §"Named carve-outs" carveout — *"is the child
alive and listening?"* — not state-change polling. It terminates: success
appends `ComponentReady`; timeout returns typed `ComponentReadinessTimeout`.
Ongoing health observation is push-shaped from the supervision socket;
manager handlers do not loop on a clock.
