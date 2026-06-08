# INTENT — persona

*What the psyche has explicitly intended for Persona. Synthesised
from psyche statements; not embellished. Verbatim psyche quotes
appear in italics where the original wording is load-bearing.*

## What Persona is

Persona is the engine-management component of the workspace's persona
component ecosystem. The canonical short name is **Persona**; the
repo is `persona`; the daemon binary is `persona-daemon`; the CLI
binary is `persona`. Engine-management names the **role** Persona
plays; "Persona" names the **entity** that fills it.

The psyche settled the canonical name explicitly:
*"So to be clear, when you say Persona Engine Manager Daemon you
mean Persona Engine or Persona? It's actually just Persona."*
Persona is composed of two binaries from the persona repo:
*"the persona engine will be persona, which is gonna be half the
persona CLI and the persona daemon"* — together they comprise
Persona, mirroring the standard component-triad CLI+daemon pattern.

The longer phrase "Persona engine" remains in scope for the
AI-work meaning of the Criome stack (per workspace vocabulary skill),
but for this component, default to "Persona".

## Persona is a permissioned system daemon

Persona runs as a privileged system daemon — *"persona will be a
permissioned system daemon"* — supervising component daemons on the
host. The privilege is scoped, not ambient root: Persona has
restricted OS authority for what its supervisory role requires
(force-focus during prompt injection, system-owned engines, peer
credential inspection, cross-engine auth proofs, component restart
after operator-user crashes). It is not the operator's user, and it
is not root.

Persona's elevated authority is the basis for everything below: it
is *because* Persona is privileged that it can take over component
upgrade management from CriomOS, manage systemd units for component
daemons, and orchestrate FD-handoff for lossless cutover routing.

## Persona takes over component upgrade management

Component upgrade orchestration moves from CriomOS-home into
Persona. The psyche named this directly: *"if we just had a
root level persona engine, then we wouldn't have all this problem
of having to upgrade CriomOS every time we want to upgrade a
component. We just move all of that upgrade logic into the
persona engine."*

CriomOS-home no longer needs flake.lock updates per component
upgrade; the selector-flip mechanism becomes a Persona concern
rather than a home-manager symlink; component upgrade cadence
decouples from CriomOS deploy cadence. The version-handover
protocol — main/next coexistence, divergence recording, mirror
streams, recovery — lives inside Persona's domain.

This positioning is upstream of Spirit cutover. *"Land persona
engine before Spirit cutover; engine orchestrates from day one."*
Persona becomes the upgrade orchestrator first; only then does
Spirit migrate to the new substrate behind it.

## Boot sequence

Persona's boot order across the component federation:

```
supervisor → sema-upgrade → mind → orchestrate → router → harness → terminal → message → introspect → spirit
```

The psyche has settled three load-bearing positions in this
order:

- **sema-upgrade comes first** among component-supervised daemons.
  Every other component needs the upgrade substrate available
  before its own startup can attempt a schema migration.
- **Persona lands before Spirit cutover** — Persona orchestrates
  component upgrades from day one (per *"Land persona engine
  before Spirit cutover; engine orchestrates from day one"*).
- **Spirit spawns last** — the psyche settled this in
  `intent/persona.nota` 2026-05-19T14:00:00Z:
  *"persona intent is a new component. … it is the apex, the
  most powerful part, notwithstanding the supervisor, which only
  has higher permission because it's an infrastructure
  component that's there to make sure the engine is running. …
  it would be the last one to start."*

The principle reads: spirit-as-apex spawns last because every
supervised component must be up before the cognitive layer
animates. The supervisor's higher authority is infrastructure-
shaped (permission to spawn, restart, observe); spirit's
authority is cognitive (the apex of the thinking chain).

## Systemd template units from day one

In production, Persona uses systemd template units —
`persona-component@<component>:<version>.service` — to manage
component daemons. The decision was settled as
*"Template units from the start"*.

The implementation uses a `UnitController` trait abstraction with
two backends: production systemd D-Bus, plus a direct-fork backend
for tests and sandbox. The direct-fork `DirectProcessLauncher`
stays available for non-production paths; it is not the production
path.

Persona keeps engine-management authority — active-version
selector, handover protocol, event log; systemd owns process
control — cgroups, restart, sandboxing, journald.

## No client-side discovery — one stable socket per component

Clients connect to one stable socket per component and just talk.
Persona handles which unit listens behind it. The psyche rejected
client-side discovery: *"the CLI is not going to be complicated.
There's no reason to do that. Because we want to make it, this is
not CLI-based, the client has to be able to just talk."*

The dev sandbox follows the same model as production — no
asymmetry. Both rely on Persona binding the stable public socket
and handing off accepted connections to the active version daemon.

## FD-handoff via SCM_RIGHTS for lossless cutover

The active-version cutover mechanism is Persona-orchestrated
file-descriptor handoff via SCM_RIGHTS. Persona binds the stable
public socket per component, accepts client connections, sends
accepted FDs over SCM_RIGHTS to the active-version daemon. Component
daemons receive FDs on a per-component control connection to
Persona. Same socket model in dev and prod (no asymmetry);
Persona is off the byte path after handoff.

The psyche ratified this shape over alternative designs (data-plane
proxy; systemd socket activation): *"in /155 I go with your leans
for first prototype. file beads and lets do it."*

Persona-restart resilience and EngineManagement-channel handling
are separate from this — meta sockets stay direct-bind per-version;
the FD-handoff applies to public ordinary sockets only.

## Multi-engine supervision

One privileged Persona daemon supervises multiple engine
instances. Each engine has its own component federation
(mind, orchestrate, router, harness, terminal, message, introspect,
spirit). Persona owns the engine catalog; per-engine state lives
under per-engine paths; component daemons are versioned and may
coexist side-by-side during a handover.

The psyche has named this multi-engine shape from early on
(intent/persona.nota 2026-05-18 and earlier brainstorming): a
top-level engine manager that organizes multiple engines so that
agents within each engine federate independently.

## Intent substrate

Legacy `intent/*.nota` files are read-only historical. Agents
must not append new psyche intent to those files; new psyche
statements are captured through the deployed Spirit CLI. The
psyche settled this twice in close succession:

*"Why are you logging in the files? We are not using the files
anymore, we are using Spirit."*

*"remove that old instructions, we use spirit now."*

The legacy file substrate remains as a historical snapshot — it
is not the normal write path. Persona's design intent draws on
those files as durable history but does not generate new entries
into them. The intent substrate flows through Spirit going
forward.

## Principles

- **Persona is privileged infrastructure.** Its OS authority is
  scoped to its supervisory role: it can spawn, restart, and
  observe component daemons; manage systemd units; route public
  client traffic via FD-handoff; inspect peer credentials; mint
  spawn envelopes. It is not root; it is not the operator user.
- **Engine-management is the role; Persona is the entity.** The
  daemon does engine-management; the daemon is named Persona.
- **Persona owns upgrade orchestration.** Components upgrade
  through Persona, not through CriomOS rebuilds.
- **Stable client socket; Persona owns the byte handoff.**
  Clients talk to one socket per component; Persona moves the
  socket onto the active version daemon at cutover.
- **Same socket model in dev and prod.** No asymmetry between
  development sandbox and production deployment.
- **Spirit spawns last; the supervisor's higher permission is
  infrastructure-shaped, not cognitive.** Persona has higher
  permission *because* it is infrastructure; Spirit is the apex
  of the cognitive authority chain.

## Constraints

- **Persona runs as a permissioned system daemon** (privileged,
  supervising component daemons).
- **Persona must land before Spirit cutover.** Persona
  orchestrates component upgrades from day one.
- **Systemd template units for production** — `persona-component@
  <component>:<version>.service` from day one.
- **No client-side discovery** — the client connects to one
  stable socket and just talks.
- **Persona is off the byte path after FD handoff** — the active
  version daemon owns the accepted client connection directly.
- **Component upgrade cadence decouples from CriomOS deploy
  cadence** — CriomOS no longer needs redeploy per component
  upgrade.

## Anti-patterns

- **Persona as ambient-root.** Privileged does not mean root;
  Persona has scoped OS authority for its supervisory concerns
  only.
- **Client-side discovery of which version listens.** Rejected.
  Persona handles the routing behind the stable socket.
- **Different socket model in dev vs prod.** Rejected — same
  shape both ways.
- **Spirit-first boot.** The supervisor spawns Spirit last;
  earlier components are running before the cognitive apex
  animates.
- **Appending psyche intent to `intent/*.nota` files.** Use the
  Spirit CLI; the file substrate is historical.

## Pending schema-engine upgrade

**Status:** scheduled for migration to schema-language-based contract per `reports/designer/326-v13-spirit-complete-schema-vision.md` + `reports/designer/324-migration-mvp-spirit-handover-re-specification.md`.

**Target:** Persona's hand-written `signal_channel!` invocation + Layer 2 Command/Effect + storage types convert to a single `persona/persona.schema` file consumed by the brilliant macro library (`primary-ezqx.1`).

**Sequence:** Spirit is the MVP pilot landing first via `primary-ezqx.1`; Persona follows after pilot succeeds. Persona's contract surface is narrowed post-/318 to engine supervision + systemd unit-start; the AttemptHandover verb has moved to the upgrade triad. The cutover edits a small daemon contract plus the engine-management surface.

**References:**
- `reports/designer/326-v13-spirit-complete-schema-vision.md`
- `reports/designer/324-migration-mvp-spirit-handover-re-specification.md`
- `reports/operator/174-schema-import-header-design-critique-2026-05-24.md`

## See also

- `/home/li/primary/skills/component-triad.md` — the triad
  shape Persona follows for its CLI+daemon composition; the
  authority-chain example uses Persona's downstream components.
- `/home/li/primary/skills/workspace-vocabulary.md` — canonical
  Persona naming; the engine-management socket axis.
- `/home/li/primary/repos/persona/ARCHITECTURE.md` — the
  structural shape; §1.7 "Startup Strategy" details the systemd
  template units and the spawn order.
- `/home/li/primary/repos/meta-signal-persona/ARCHITECTURE.md` — the
  engine-management contract Persona owns; the skeleton-honesty
  rule that every supervised component answers.
- Spirit intent records under topic `persona` (208, 209, 215,
  216, 238, 239, 240, 246, 252) — the source psyche statements
  for this file.
- Legacy `~/primary/intent/persona.nota` — historical psyche
  statements on persona-spirit, persona-orchestrate, and the
  earlier engine-manager / spawn-order framing.
