# persona — architecture

*Apex integration repository for the Persona component ecosystem.*

> `persona` composes the system. Component implementation lives in
> component repositories; this repo wires them together through Nix,
> documents the whole topology, and owns deployment-level verification.

---

## 0 · TL;DR

Persona coordinates interactive AI harnesses as first-class
participants in one inspectable system. Components are ractor-based:
each runtime component owns its own actors, and actors communicate by
typed signal frames. Durable Persona state is owned by an
orchestrator actor that holds the `persona-sema` database handle.
Human and harness text is a projection at the boundary.

The architecture is channel-first. Each pair of components that
communicates over a wire shares a dedicated `signal-persona-*`
contract repo. That contract repo is the synchronization point for
parallel development: settle the channel vocabulary first, then let
producer and consumer implementations move independently against the
same types.

`persona` is not the home for router internals, terminal adapters,
storage tables, actor logic, or signal records. It is the apex:
architecture, Nix composition, deployment wiring, and end-to-end
tests.

## 1 · Component Map

```mermaid
flowchart TB
    subgraph apex["persona"]
        architecture["apex architecture"]
        nix["Nix composition"]
        tests["end-to-end tests"]
        deployment["deployment wiring"]
    end

    subgraph contracts["signal contracts"]
        umbrella["signal-persona"]
        message_contract["signal-persona-message"]
        store_contract["signal-persona-store"]
        system_contract["signal-persona-system"]
        harness_contract["signal-persona-harness"]
        terminal_contract["signal-persona-terminal"]
    end

    subgraph runtime["runtime components"]
        message["persona-message"]
        router["persona-router"]
        sema["persona-sema"]
        system["persona-system"]
        harness["persona-harness"]
        wezterm["persona-wezterm"]
        orchestrate["persona-orchestrate"]
    end

    apex -->|imports flakes| runtime
    apex -->|imports flakes| contracts

    umbrella --> message_contract
    umbrella --> store_contract
    umbrella --> system_contract
    umbrella --> harness_contract
    umbrella --> terminal_contract

    message_contract --> message
    message_contract --> router
    store_contract --> router
    store_contract --> orchestrate
    system_contract --> system
    system_contract --> router
    harness_contract --> router
    harness_contract --> harness
    terminal_contract --> harness
    terminal_contract --> wezterm
    orchestrate --> sema
```

| Repository | Role |
|---|---|
| `signal-persona` | Umbrella Persona domain records shared across channels. |
| `signal-persona-message` | CLI/text ingress channel: `persona-message` → `persona-router`. |
| `signal-persona-store` | Durable commit channel: `persona-router` → `persona-orchestrate`'s state actor. |
| `signal-persona-system` | OS/window/input observation channel: `persona-system` → `persona-router`. |
| `signal-persona-harness` | Harness delivery and observation channel: `persona-router` ↔ `persona-harness`. |
| `signal-persona-terminal` | Terminal projection channel: `persona-harness` → `persona-wezterm`. |
| `persona-message` | Human and harness message CLI/projection boundary. |
| `persona-router` | Delivery reducer, gate reducer, and pending-delivery state machine. |
| `persona-sema` | Typed table layout and schema guard over the sema kernel. |
| `persona-system` | System/window/input observation adapters. |
| `persona-harness` | Harness identity, lifecycle, transcripts, and adapter contracts. |
| `persona-wezterm` | Durable PTY and detachable WezTerm viewer transport. |
| `persona-orchestrate` | Runtime orchestration actors, including the actor that owns the `PersonaSema` handle. |

## 2 · Choreography Model

The contract repo lands before the runtime behavior that uses it.
Producer and consumer repos do not invent local duplicate message
types while waiting for a channel change. A channel change starts in
the relevant `signal-persona-*` repo; after it is pushed, the
producer and consumer implementation repos update against it.

```mermaid
sequenceDiagram
    participant Contract as signal-persona-* contract
    participant Producer as producer component
    participant Consumer as consumer component
    participant Tests as architectural-truth tests

    Contract->>Contract: define closed request/reply/event records
    Producer->>Contract: depend on shared channel types
    Consumer->>Contract: depend on shared channel types
    Producer->>Consumer: send length-prefixed rkyv frame
    Consumer->>Tests: leave observable witness
    Tests->>Tests: prove the intended component path was used
```

This lets multiple agents work in parallel without relying on chat
memory: the contract crate is the stable typed agreement.

## 3 · Wire Vocabulary

Rust-to-Rust traffic uses signal-family frames: length-prefixed
rkyv archives with channel-specific request and reply payloads.
Text is NOTA syntax. In practice, Persona request/message text is
usually Nexus: a NOTA-based request surface. Convenience CLIs such
as `message` construct the Nexus record shape in NOTA syntax for the
user instead of asking them to type the full wrapper. None of this
is the inter-component wire.

```mermaid
flowchart LR
    human["human or harness"] -->|text projection| message["persona-message"]
    message -->|signal-persona-message| router["persona-router"]
    router -->|signal-persona-store| orchestrate["persona-orchestrate"]
    orchestrate -->|typed tables| sema["persona-sema"]
    system["persona-system"] -->|signal-persona-system| router
    router -->|signal-persona-harness| harness["persona-harness"]
    harness -->|signal-persona-terminal| wezterm["persona-wezterm"]
```

Each channel contract owns only the records exchanged on that
channel: closed request/reply/event enums, rkyv round trips, text
projection examples where useful, and version expectations. It owns
no daemon code, actors, routing policy, storage policy, or terminal
adapter logic.

## 4 · State and Ownership

`persona-sema` owns Persona's typed storage tables and schema
version. An actor inside `persona-orchestrate` owns the
`PersonaSema` handle, runtime transaction ordering, the mailbox into
the database, and commit visibility. The router requests commits
through `signal-persona-store`; it does not write redb directly.

```mermaid
flowchart LR
    router["persona-router"] -->|CommitRequest| orchestrate["persona-orchestrate state actor"]
    orchestrate -->|write transaction| sema["persona-sema"]
    sema -->|redb + rkyv| database[("persona.redb")]
    database -->|authoritative read| reader["persona-sema reader"]
    orchestrate -->|CommitOutcome| router
    router -->|after commit| harness["persona-harness"]
```

The load-bearing safety rule is commit-before-deliver: no harness
delivery happens without a durable store commit that can be read
back through `persona-sema`.

## 5 · Boundaries

This repository owns:

- apex architecture;
- Nix flake inputs and component composition;
- end-to-end tests that prove component composition;
- deployment wiring for a full Persona system;
- architectural-truth tests that need multiple components.

This repository does not own:

- shared Persona records (`signal-persona`);
- per-channel wire contracts (`signal-persona-*`);
- router policy (`persona-router`);
- orchestrator state actors (`persona-orchestrate`);
- terminal transport (`persona-wezterm`);
- harness lifecycle internals (`persona-harness`);
- OS/window-manager adapters (`persona-system`);
- typed table internals (`persona-sema`);
- workspace coordination internals (`persona-orchestrate`).

## 6 · Invariants

- The meta repo composes; component repos implement.
- Each wire between components has a signal contract repo.
- Contract repos own types; runtime repos own behavior.
- Stateful runtime behavior lives in ractor actors inside the
  component that owns the concern.
- Rust-to-Rust component traffic uses length-prefixed rkyv frames.
- NOTA syntax appears only at human, harness, CLI, configuration,
  and audit projection boundaries. Persona request/message text is
  normally Nexus, which is a NOTA-based surface.
- Producers push; consumers subscribe. Polling is not a fallback.
- Harnesses are first-class records, not hidden terminal sessions.
- Durable writes in the assembled runtime pass through
  `persona-orchestrate`'s state actor and `persona-sema`.
- Delivery is downstream of durable commit.
- Macro extraction follows observed repetition. `signal-derive` does
  not own channel behavior; channel boilerplate remains hand-written
  until several channels reveal the shared shape.

## 7 · Architectural-Truth Tests

The end-to-end test suite proves the architecture, not only visible
behavior. The first messaging stack needs witnesses for:

| Invariant | Witness |
|---|---|
| Message CLI uses the message contract | CLI emits a `signal-persona-message` frame. |
| Router commits before delivery | Event trace shows commit outcome before harness delivery. |
| Router does not write redb directly | Router depends on store contract, not `persona-sema` internals. |
| Orchestrator actor owns database writes | redb file is produced through `persona-orchestrate` and read by a separate `persona-sema` reader. |
| Delivery is push-based | No retry occurs without pushed system or harness observation. |
| Terminal transport stays isolated | Router dependency graph excludes `persona-wezterm`. |

## Code Map

```text
ARCHITECTURE.md  apex system shape
skills.md        how to work in the meta repo
flake.nix        component flake composition
src/             temporary schema stub while component repos absorb runtime
tests/           schema tests and multi-component end-to-end tests
```

## See Also

- `../signal-persona/ARCHITECTURE.md`
- `../persona-message/ARCHITECTURE.md`
- `../persona-router/ARCHITECTURE.md`
- `../persona-system/ARCHITECTURE.md`
- `../persona-harness/ARCHITECTURE.md`
- `../persona-wezterm/ARCHITECTURE.md`
- `../persona-sema/ARCHITECTURE.md`
- `../persona-orchestrate/ARCHITECTURE.md`
- `~/primary/reports/designer/72-harmonized-implementation-plan.md`
- `~/primary/reports/designer/73-signal-derive-research.md`
- `~/primary/reports/operator/71-parallel-signal-contract-architecture-plan.md`
