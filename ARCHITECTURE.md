# persona — architecture

*Engine manager and apex integration repository for the Persona component ecosystem.*

> `persona` is the host-level engine manager for the Persona component
> ecosystem. One privileged `persona` daemon supervises multiple engine
> instances, keeps component daemons visible and coordinated, allocates their
> per-engine sockets and state directories, wires them through Nix, documents
> the whole topology, and owns deployment-level verification. Component
> implementations live in component repositories.

---

## 0 · TL;DR

Persona coordinates interactive AI harnesses as first-class participants in
durable, inspectable engines. The top-level `persona-daemon` process is the
host-level engine manager: it runs as the dedicated `persona` system user,
supervises multiple engine instances, exposes engine status, classifies
connections, and gives operators and harnesses one place to ask whether the
total system is up, healthy, and coherent.

`signal-persona` is the management contract for the `persona` engine manager.
It is the contract a client uses to ask for engine status, component health,
engine-visible projections, and supervisor actions. Component-to-component
behavior uses the relation-specific `signal-persona-*` contracts.

The `persona` CLI is a thin daemon client. It decodes one NOTA request record,
sends one length-prefixed `signal-persona` frame to `persona-daemon`, waits for one
typed reply frame, renders one NOTA reply record, and exits. `persona-daemon` owns
the live Kameo `EngineManager` actor for the daemon lifetime.

The center of agent state is `persona-mind`: the daemon-backed state component
for role coordination, activity, work memory, decisions, aliases, and
ready/blocked views. The command-line surface for that state is the `mind`
binary: one NOTA request record in, one NOTA reply record out, with the CLI
acting as a thin client to the daemon.

The architecture is contract-first. A wire boundary is defined in a dedicated
`signal-persona-*` repository before producer and consumer implementations
move against it. Contract crates own typed records and rkyv frame behavior;
runtime crates own actors, policy, storage, and side effects.

`persona` is the apex repo and engine-manager home. It owns architecture,
flake composition, supervisor wiring, deployment verification, and
cross-component tests. Component repositories own router policy, mind state
transitions, terminal adapters, storage table internals, actor logic, and
relation-specific signal records.

Sema belongs to the component that owns the state inside an engine:
`persona-mind` has mind Sema / `mind.redb`, `persona-router` has router Sema /
`router.redb`, and so on. The `persona` engine manager owns manager-level
state: the engine catalog, component desired state, health, lifecycle
observations, startup/shutdown activity, inter-engine routes, and
engine-level status.

```mermaid
graph TB
    operator[human or harness] --> persona_cli[persona CLI]
    persona_cli --> persona_contract[signal persona]
    persona_contract --> persona_daemon[persona daemon]
    persona_daemon --> manager[persona engine manager]
    manager --> engine_a[engine A federation]
    manager --> engine_b[engine B federation]
    engine_a --> mind_daemon[persona mind daemon]
    engine_a --> router[persona router]
    engine_a --> system[persona system]
    engine_a --> harness[persona harness]
    engine_a --> terminal[persona terminal]
    agent[agent] --> mind_cli[mind CLI]
    mind_cli --> mind_daemon
    mind_daemon --> mind_db[mind redb]
    router --> router_db[router redb]
```

## 0.5 · Persona — the durable agent

Persona is the durable agent. The Persona ecosystem is the workspace's
answer to OpenClaw and Gas City: long-lived, persistent, inspectable
agent runtime instead of one-shot agent CLIs and reconciliation-stack
controllers.

| Failure mode being rejected | Persona answer |
|---|---|
| Many sources of truth reconciled by polling | Each state-bearing component owns one redb file; producers push, consumers subscribe. |
| Hidden mutation under uncertainty | Every state transition has a typed input event, typed output event, durable record. Constraints become witness tests. |
| State-machine controller spawning processes | Direct Kameo actors with named planes, supervised, traceable. |
| Tmux-as-runtime-substrate | Terminal as adapter; harnesses as first-class records. |
| One-shot agent CLIs with no persistent state | Long-lived daemons. CLIs are thin clients to the daemons. |

This positioning is upstream of every individual persona-* component.
It is the criterion that decides what belongs in the persona ecosystem:
durable-agent work belongs here; one-shot operator actions (deploy CLIs)
live in `lojix-cli` / `CriomOS`; declarative cluster data lives in
`goldragon`; auth/security/identity infrastructure (host trust, cluster
identity) lives in the criome ecosystem.

> **Scope.** Today's Persona sits on today's stack — Rust on Linux,
> direct Kameo, `sema-db` storage, signal-* wire. The
> eventually-self-hosting stack is one Sema-on-Sema substrate that
> subsumes these pieces; today's Persona is a realization step
> toward it, built rightly for the scope it serves now. See
> `~/primary/ESSENCE.md` §"Today and eventually — different things,
> different names".

## 1 · Component Map

| Repository | Role |
|---|---|
| `persona` | Engine manager, `persona-daemon` home, apex Nix/deployment/test composition, and meta architecture. |
| `persona-mind` | Central state component and command-line mind runtime. |
| `signal-persona-mind` | Typed contract for role coordination, activity, and work graph operations. |
| `persona-router` | Message routing, delivery state, gate state, and pending-delivery decisions. |
| `persona-message` | Stateless message proxy: NOTA/Nexus CLI surface to router Signal frames and router replies back to NOTA. |
| `persona-system` | System/window/input observation adapters. |
| `persona-harness` | Harness identity, lifecycle, transcripts, and delivery adapter boundary. |
| `persona-terminal` | Durable PTY/session owner around `terminal-cell`, visible viewer adapters, raw terminal byte transport, and terminal metadata. |
| `terminal-cell` | Low-level daemon-owned PTY/transcript primitive consumed by `persona-terminal`. |
| `sema` | Typed database kernel library over redb/rkyv. |
| `signal-core` | Signal wire kernel: frames, channel macro, shared wire primitives. |
| `signal-persona` | Management contract for the `persona` engine manager. |
| `signal-persona-message` | Message ingress contract. |
| `signal-persona-system` | System observation contract. |
| `signal-persona-harness` | Router/harness delivery and observation contract. |
| `nexus` | Semantic text vocabulary written in NOTA syntax. |
| `nota` / `nota-codec` / `nota-derive` | NOTA language, parser/codec, and derive support. |

```mermaid
graph LR
    signal_core[signal core] --> mind_contract[signal persona mind]
    signal_core --> message_contract[signal persona message]
    signal_core --> system_contract[signal persona system]
    signal_core --> harness_contract[signal persona harness]
    mind_contract --> mind[persona mind]
    message_contract --> message[persona message]
    message_contract --> router[persona router]
    system_contract --> system[persona system]
    system_contract --> router
    harness_contract --> router
    harness_contract --> harness[persona harness]
```

## 1.5 · Engine Manager Model

One host has one `persona` daemon. That daemon can supervise N engine
instances. Each engine owns its own full component federation:
`persona-mind`, `persona-router`, `persona-system`, `persona-harness`,
`persona-terminal`, and the transitional `persona-message` proxy.

The daemon runs as the dedicated `persona` system user, not as `root` and not
as the operator's user. The elevated position is for scoped OS authority:
force-focus during prompt injection, system-owned engines, peer credential
inspection, cross-engine auth proofs, and component restart after
operator-user crashes. Components also run under the Persona authority
baseline, but the manager owns cross-engine authority.

Per-engine resources are always scoped by engine id:

| Resource | Shape |
|---|---|
| State directory | `/var/lib/persona/<engine-id>/` |
| Component redb files | `/var/lib/persona/<engine-id>/{mind,router,harness,terminal}.redb` |
| Socket directory | `/var/run/persona/<engine-id>/` |
| Component sockets | `/var/run/persona/<engine-id>/{mind,router,system,harness,terminal,message-proxy}.sock` |
| Manager redb | `/var/lib/persona/manager.redb` |
| Manager socket | `/var/run/persona/persona.sock` |

Exact host paths are deployment-owned, but the `<engine-id>` scoping is
architectural. Components do not discover peers by scanning the filesystem;
the manager passes peer sockets at spawn time.

The manager redb owns the engine catalog: engine identities, owners,
component desired state, lifecycle observations, and inter-engine route
declarations. Every transition appends a typed event and reduces into the
manager tables.

## 1.6 · Connection Class and Routes

Every connection into a manager-owned engine carries a `ConnectionClass`
minted by the engine boundary, never supplied by the agent:

```text
ConnectionClass =
  Owner
  | NonOwnerUser(Uid)
  | System(SystemPrincipal)
  | OtherPersona { engine_id, host }
```

The manager reads peer credentials, compares them with the engine owner,
verifies cross-engine auth proofs when needed, and stamps the class onto the
connection's Signal auth context. Components use the class as a policy axis:
router quarantines non-owner messages, system gates privileged focus actions,
harness redacts non-owner identity views, terminal gates programmatic input,
and mind records non-owner work-graph mutations as suggestions until adopted.

Inter-engine traffic requires an `EngineRoute`: a typed, manager-owned route
record from one engine to another with allowed message kinds and an approval
state. Human-owned engines require explicit owner approval on both sides;
system-owned engines may use explicit `SystemApproved` policy records.

## 1.7 · Startup Strategy

Startup has two layers.

Development and integration tests start component binaries directly through
Nix-owned scripts in this meta repo. The scripts allocate a temporary runtime
directory, start the current runnable daemons, push socket paths through
environment variables, and leave inspectable artifacts. This is the
`persona-dev-stack` surface; it exists so integration work can happen before
host-level service installation is settled.

Host deployment is systemd-shaped. The production `persona` daemon is the
host-level manager and should be started by a NixOS module as a systemd
service. Component daemons may become systemd units or manager-spawned child
processes, but the manager is still the component that allocates per-engine
state directories and pushes peer sockets to children. Daemons may use
systemd readiness/watchdog notification once they run under systemd; direct
systemd D-Bus control from Rust is only needed if the `persona` daemon later
creates or manipulates transient units itself.

The first meta-repo runner starts only the executable halves that exist today:

```mermaid
graph LR
    dev["persona-dev-stack"]
    router["persona-router daemon"]
    terminal["persona-terminal daemon"]
    message["message CLI"]
    terminal_signal["persona-terminal-signal"]
    pty["terminal-cell child PTY"]

    dev --> router
    dev --> terminal
    message -->|"Signal message frame"| router
    terminal_signal -->|"Signal terminal frame"| terminal
    terminal --> pty
```

That runner proves router ingress and terminal transport independently. It is
not the final delivery witness because the external router-to-harness
registration/control surface has not landed yet.

The full-engine sandbox witness starts at the same apex layer. The Nix app
`persona-engine-sandbox` creates an isolated `state/`, `run/`, `home/`,
`work/`, and `artifacts/` tree, writes NOTA manifests, and plans a
`systemd-run --user` transient unit with `PrivateUsers=yes`,
`PrivateTmp=yes`, `ProtectHome=tmpfs`, and `ReadWritePaths=<sandbox>`.
Prompt-bearing Claude and Codex runs require dedicated sandbox credentials;
the runner does not copy live host `~/.claude` or `~/.codex` authentication
files. Pi is the preferred first harness because it uses the local
Prometheus-backed model path.

The sandbox runner also owns the dedicated-auth bootstrap surface:
`persona-engine-sandbox --bootstrap-auth --harness <kind>`. Codex bootstrap
uses a separate sandbox `CODEX_HOME` and the real `codex login --device-auth`
flow so the host browser can authorize a distinct `auth.json`. Claude
bootstrap either consumes a dedicated `PERSONA_CLAUDE_OAUTH_TOKEN_FILE`
through `LoadCredential=` or runs `claude auth login --claudeai` under a
separate `CLAUDE_CONFIG_DIR`. Pi bootstrap creates isolated
`PI_CODING_AGENT_DIR` and `PI_CODING_AGENT_SESSION_DIR` paths and records
`PI_PACKAGE_DIR`. This bootstrap path is specifically not a host-auth copy
path; copying `~/.codex/auth.json` or `~/.claude/.credentials.json` for
prompt-bearing tests is forbidden.

The first daemon-first apex slice is present: `persona-daemon` binds a Unix socket,
starts the Kameo `EngineManager`, accepts one Signal frame per connection,
dispatches through `HandleEngineRequest`, writes one Signal reply frame, and
keeps manager state across CLI invocations. The manager redb path is present
through a dedicated `ManagerStore` Kameo actor backed by Sema; manager
mutations persist by sending typed messages to that actor. The full
multi-engine catalog, spawn supervisor, connection-class minting, socket ACL
application, and privileged-user deployment witnesses are the next
engine-manager layers.

## 2 · Command-line Mind

The first foundational implementation target is the command-line mind backed
by a long-lived `persona-mind` daemon.

```mermaid
graph LR
    orchestrate[tools orchestrate shim] --> mind_cli[mind CLI]
    direct_agent[agent direct call] --> mind_cli
    mind_cli --> contract[signal persona mind]
    contract --> daemon[persona mind daemon]
    daemon --> runtime[persona mind runtime]
    runtime --> store[mind redb]
```

The target surface:

```sh
mind '<one NOTA request record>'
```

Output:

```sh
'<one NOTA reply record>'
```

`tools/orchestrate` may remain as external cutover glue while agents
transition. It should lower ergonomic commands into the same
`signal-persona-mind` request records, send them through the `mind` client
path, and stop treating lock files as authoritative state.

## 3 · Wire Vocabulary

Rust-to-Rust traffic uses Signal frames: length-prefixed rkyv archives with
channel-specific request/reply payloads.

`signal-persona` is the contract for talking to the top-level `persona` engine
manager. A client uses it to ask the engine manager for engine status,
component health, engine-visible projections, and supervisor actions.
It is also the home for engine catalog and route records: `EngineId`,
`OwnerIdentity`, `ConnectionClass`, `EngineRoute`, `ComponentName`,
`ComponentSet`, `DesiredState`, and shutdown/restart requests. If a second
contract domain needs `ConnectionClass`, it may move down into `signal-core`;
until then it remains part of the engine-manager contract.

The `signal-persona-*` repos are relation-specific contracts between concrete
components: mind, message, router, system, harness, terminal, and their
neighbors. Runtime crates move against those contracts instead of reaching into
another component's state.

Text uses NOTA syntax. Nexus is semantic content written in NOTA syntax, not a
second parser or alternate text format. Convenience CLIs may hide wrapper
records, but their output must still lower into typed Signal records.

```mermaid
graph TB
    human_text[NOTA text] --> cli[CLI projection]
    cli --> typed[typed Signal records]
    typed --> frame[rkyv Signal frame]
    frame --> runtime[component runtime]
```

Each contract repo owns only its channel vocabulary: closed request/reply/event
enums, validation newtypes, rkyv round trips, and text projection examples
where useful. It owns no daemon code, Kameo actors, routing policy, storage
policy, or terminal adapter logic.

## 4 · State and Ownership

`sema` is the database kernel library. There is no shared Persona storage
layer. Every state-bearing component owns its own Sema layer or table module
inside that component's implementation. Neither `persona` nor `sema` is a
process boundary for another component's state.

Each state-bearing component owns:

- its Kameo actor tree;
- its durable redb file;
- its write-order actor;
- its post-commit subscription behavior.

```mermaid
graph TB
    router_actor[router actor tree] --> router_db[router redb]
    mind_actor[mind actor tree] --> mind_db[mind redb]
    harness_actor[harness actor tree] --> harness_db[harness redb]
    router_db --> sema[sema library]
    mind_db --> sema
    harness_db --> sema
```

Component boundaries are crossed with Signal contracts, not by opening another
component's database file.

## 5 · Mind, Router, Harness, System

The central split:

| Component | Owns | Does not own |
|---|---|---|
| `persona-mind` | role state, activity, work graph, decisions, aliases, ready/blocked views. | message delivery, terminal sessions, system focus facts. |
| `persona-router` | message routing, delivery queue, delivery gate state, message durability. | role claims, work graph, harness process lifecycle. |
| `persona-system` | OS/window/input observations. | router decisions, mind state, harness delivery. |
| `persona-harness` | harness identity, lifecycle, injection/observation adapter boundary. | router policy, central work graph. |
| `persona-terminal` | durable PTY/session ownership, visible viewer adapters, and raw terminal byte transport. | Persona delivery policy or role state. |

`persona-mind` is not a router. `persona-router` is not the central project
memory. The two communicate through explicit contracts when they need each
other.

Connection-class policy is component-owned:

| Component | Class-aware behavior |
|---|---|
| `persona-router` | Applies router-owned authorized-channel state. Unknown or inactive channels park and ask `persona-mind` for adjudication. |
| `persona-system` | Read observations are subscribable; privileged focus actions require `System(persona)`. |
| `persona-harness` | Full harness identity is owner/system visible; non-owner views are redacted. |
| `persona-terminal` | Programmatic input uses `ConnectionClass` at the gate; non-owner injections are dropped unless owner-approved. |
| `persona-mind` | Non-owner work-graph mutations are third-party suggestions until the owner adopts them. |

## 6 · Lock Files and BEADS

Lock files and BEADS are transitional coordination surfaces in the primary
workspace. They are not the destination architecture.

Do not implement lock-file projections in Persona. The current lock files are
part of the temporary operator workflow and will be retired when agents switch
to the command-line mind. `persona-mind` stores typed role state; it does not
write lock files as a compatibility feature.

Destination:

```mermaid
graph LR
    claim[RoleClaim] --> mind[persona mind]
    mind --> db[mind redb]
    db --> role_view[RoleSnapshot]
    db --> work_view[Ready work view]
```

Migration rules:

- lock files are not part of Persona implementation work;
- lock files are not durable truth and do not get projections from
  `persona-mind`;
- BEADS entries may be imported once as items, aliases, or external
  references;
- Persona does not grow a long-term BEADS bridge;
- new work graph behavior belongs in `persona-mind`.

## 7 · Constraints

- `persona` composes the stack; component repos implement behavior.
- One host has one privileged `persona` daemon supervising multiple engine
  instances.
- The daemon runs as the dedicated `persona` system user, not as root and not
  as the operator's user.
- `persona` may wire Nix inputs, checks, deployment modules, and
  cross-component witness tests.
- The meta repo exposes Nix apps for stateful integration runners; recurring
  daemon startup commands are not left as ad hoc shell history.
- The sandboxed engine runner is a Nix app named `persona-engine-sandbox`;
  its reusable command line is not an ad hoc shell transcript.
- Prompt-bearing Claude/Codex sandbox tests require dedicated sandbox
  credentials; the runner never copies live host authentication files.
- Sandboxed engine artifacts are sanitized manifests and targeted witness
  outputs, not raw home snapshots.
- Dedicated auth bootstrap is an explicit runner mode; prompt-bearing Codex
  and Claude tests never bootstrap by copying live host OAuth files.
- Auth-isolation witnesses run the actual sandbox runner against fake host
  auth/session files and fail if host paths leak into artifacts, host files
  change, or credential files are copied into the sandbox.
- Development runners push socket paths to components through environment and
  argv, never by filesystem discovery.
- Production startup is systemd/NixOS-shaped; Rust systemd control is an
  implementation detail, not the first required integration boundary.
- `persona` does not own mind state transitions, router policy, harness
  lifecycle, terminal transport, storage table internals, or Signal records.
- Every runtime boundary in the stack has a dedicated Signal contract repo.
- Cross-component tests prove boundaries by bytes, processes, dependency
  graphs, or durable files; they do not share in-process memory as the witness.
- State-bearing components own separate redb files and separate Sema table
  declarations.
- Per-engine state and socket paths include the engine id; cross-engine state
  lives only in the manager catalog.
- Engine layout planning names every first-stack component socket and state
  file before a component is spawned.
- Internal component sockets are private to the Persona authority boundary;
  the message-proxy socket is group-writable for owner ingress.
- Spawn envelopes carry the component's own state/socket paths and every peer
  socket path; components do not derive peers by scanning directories.
- The manager mints `ConnectionClass` at the engine boundary; agents cannot
  supply their own class.
- Inter-engine routes are typed, manager-owned records and require owner or
  system approval before traffic flows.
- Component spawn receives peer socket paths from the manager; components do
  not scan the filesystem to discover peers.
- Components talk by Signal frames, not by opening another component's redb
  file.
- The manager catalog is written through the `ManagerStore` actor; request
  handling does not open `manager.redb` directly.
- NOTA is the only text syntax; Nexus is semantic content written in NOTA.
- The `mind` CLI is a daemon client: one NOTA request record in, one NOTA reply
  record out.
- The `persona` CLI is also a daemon client: one NOTA request record in, one
  NOTA reply record out.
- Lock files and BEADS are temporary workspace surfaces, not Persona
  implementation targets.
- Existing transitional shims in this repo remain visibly marked as shims until
  component-owned implementations replace them.
- The Persona ecosystem owns durable-agent runtime work. Auth/security/identity
  infrastructure (host trust, cluster identity) is criome-shaped, not
  persona-shaped. One-shot deploy actions live in `lojix-cli` / `CriomOS`.
  Declarative cluster proposals live in `goldragon`.

## 8 · Invariants

- The meta repo composes; component repos implement.
- The `persona` runtime owns the top-level engine-manager actor and supervisor
  status surface.
- The `persona` runtime owns the manager catalog and supervises multiple
  per-engine component federations.
- `ConnectionClass` is minted by the manager boundary and carried as auth
  context through downstream Signal traffic.
- Each wire between components has a Signal contract repo.
- Contract repos own types; runtime repos own behavior.
- Runtime behavior lives in direct Kameo actors inside the owning component.
- `persona-mind` is Persona's central daemon-backed state component.
- Each state-bearing component owns its own redb file.
- Each state-bearing component owns its own Sema layer/table declarations.
  There is no shared `persona-sema` component in the current architecture.
- The engine manager owns `manager.redb` through its own Sema table layer.
  The write path is a data-bearing Kameo `ManagerStore` actor, not a CLI
  helper or direct redb call in request decoding.
- Cross-component access is by Signal frame, not database peeking.
- Rust-to-Rust component traffic uses rkyv Signal frames.
- NOTA is the only text syntax.
- Producers push; consumers subscribe. Polling is not a fallback.
- Harnesses are first-class records, not hidden terminal sessions.
- Message delivery is downstream of durable router-owned message commit.
- Command-line mind input is one NOTA request record; output is one NOTA reply
  record.
- Command-line persona input is one NOTA request record; output is one NOTA
  reply record.
- The `mind` CLI is a thin client. The long-lived `persona-mind` daemon owns
  `MindRoot` and `mind.redb`.
- Persona is the durable agent — long-lived, persistent, inspectable.
  One-shot agent CLIs and reconciliation-stack controllers are not
  persona-shaped. Auth/security/identity is criome-shaped, not
  persona-shaped.

## 9 · Architectural-Truth Tests

The apex repo owns tests that prove cross-component shape:

| Invariant | Witness |
|---|---|
| `mind` uses the mind contract | CLI decodes into `signal-persona-mind::MindRequest`. |
| `tools/orchestrate` is external cutover glue | wrapper output reaches the same `mind` path; it is not a Persona component. |
| mind owns role state | lock files are absent and role claims still work. |
| router commits before delivery | delivery trace follows durable router commit. |
| router does not own terminal transport | router dependency graph excludes `persona-terminal` and `terminal-cell`. |
| component databases are separate | router/mind/harness open distinct redb files. |
| NOTA is the only text syntax | no CLI-only parser accepts non-NOTA command records. |
| engine resources are scoped | generated state/socket paths include `EngineId`. |
| connection class is manager-minted | request decoding rejects agent-supplied class fields. |
| persona CLI is daemon client | CLI accepts exactly one NOTA request and prints one NOTA reply. |
| persona-daemon preserves unrelated files | daemon startup refuses a non-socket endpoint path instead of deleting it. |
| manager catalog writes go through the writer actor | `nix flake check .#persona-manager-store-writes-engine-status-through-writer-actor` |
| engine manager persists accepted mutations | `nix flake check .#persona-engine-manager-persists-component-mutation-through-manager-store` |
| persona CLI mutation reaches manager.redb via daemon path | `nix flake check .#persona-daemon-persists-cli-mutation-to-manager-store` |
| sandbox runner is a Nix-owned app | `nix flake check .#persona-engine-sandbox-script-builds` |
| sandbox runner supports each first harness name | `nix flake check .#persona-engine-sandbox-supports-all-harnesses` |
| sandbox runner documents dedicated auth | `nix flake check .#persona-engine-sandbox-documents-dedicated-auth` |
| sandbox auth bootstrap emits real dedicated login surfaces | `nix flake check .#persona-engine-sandbox-bootstrap-auth-dry-run` |
| Pi bootstrap creates isolated config/session directories | `nix flake check .#persona-engine-sandbox-pi-bootstrap-creates-isolated-dirs` |
| auth isolation witness protects host credential/session files | `nix flake check .#persona-engine-sandbox-auth-isolation-witness` |
| engine resources are scoped | `nix flake check .#persona-engine-layout-uses-engine-id-scoped-paths` |
| socket policy is boundary-specific | `nix flake check .#persona-engine-layout-assigns-socket-modes-by-component-boundary` |
| spawn envelopes carry manager-supplied peers | `nix flake check .#persona-spawn-envelope-carries-component-paths-and-peer-sockets` |
| engine preparation does not write global manager state as a side effect | `nix flake check .#persona-engine-layout-prepares-only-engine-scoped-directories` |

## Code Map

```text
ARCHITECTURE.md  apex system shape
skills.md        how to work in the meta repo
flake.nix        component flake composition
TESTS.md         cross-component test architecture
scripts/persona-engine-sandbox  systemd-run sandbox scaffold for full-engine witnesses
scripts/persona-engine-sandbox-auth-isolation-witness  Nix witness for host auth/session isolation
src/main.rs      thin CLI client for persona-daemon
src/bin/persona_daemon.rs  long-lived daemon entry
src/engine.rs    EngineId-scoped layout, socket policy, spawn envelope records
src/transport.rs Unix-socket Signal codec, client, daemon, endpoint, caller
src/manager.rs   Kameo EngineManager actor scaffold and trace witness
src/manager_store.rs  Kameo ManagerStore actor and manager.redb Sema tables
src/request.rs   NOTA projection into signal-persona requests and replies
src/state.rs     in-memory engine-state reducer
src/bin/wire_*   signal-persona-message wire-test shims
tests/           daemon, manager, request, projection, and state tests
```

## See Also

- `~/primary/protocols/active-repositories.md`
- `~/primary/reports/operator/105-command-line-mind-architecture-survey.md`
- `~/primary/reports/designer/115-persona-engine-manager-architecture.md`
- `~/primary/reports/1-gas-city-fiasco.md` — failure mode persona is contrasting against
- `../persona-mind/ARCHITECTURE.md`
- `../signal-persona-mind/ARCHITECTURE.md`
