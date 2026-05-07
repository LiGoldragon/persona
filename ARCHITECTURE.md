# persona — architecture

*Apex integration repository for the Persona component ecosystem.*

> `persona` composes the system. Component implementation lives in the
> component repositories; this repo wires them together through Nix, documents
> the whole topology, and owns deployment-level verification.

---

## 0 · TL;DR

Persona coordinates interactive AI harnesses as first-class participants in one
inspectable system. The runtime shape is a set of typed components signaling
through `persona-signal`; the meta repo imports those components and assembles
the deployment.

`persona` is not the home for router internals, terminal adapters, store
tables, or signal records. It is the apex: architecture, Nix composition,
deployment wiring, and end-to-end tests.

## 1 · Components

```mermaid
flowchart TB
    subgraph apex["persona"]
        architecture["apex ARCHITECTURE.md"]
        nix["Nix composition"]
        tests["end-to-end tests"]
        deployment["deployment wiring"]
    end

    subgraph contract["persona-signal"]
        frame["Frame"]
        handshake["handshake"]
        vocabulary["request/reply/event vocabulary"]
    end

    subgraph components["component repositories"]
        message["persona-message"]
        router["persona-router"]
        system["persona-system"]
        harness["persona-harness"]
        wezterm["persona-wezterm"]
        store["persona-store"]
        orchestrate["persona-orchestrate"]
    end

    apex -->|"imports flakes"| components
    components -->|"signals with rkyv"| contract
```

| Repository | Role |
|---|---|
| `persona-signal` | Shared length-prefixed rkyv signal contract. |
| `persona-message` | Human and harness NOTA message boundary. |
| `persona-router` | Delivery reducer and pending-delivery state. |
| `persona-system` | OS/window/input observation boundary. |
| `persona-harness` | Harness identity, lifecycle, transcripts, adapter contracts. |
| `persona-wezterm` | Durable PTY and detachable WezTerm viewer transport. |
| `persona-store` | Durable redb transaction boundary and schema guard. |
| `persona-orchestrate` | Workspace coordination: roles, claims, handoffs. |

## 2 · Wire Vocabulary

Rust components signal each other with `persona-signal::Frame`, encoded as
length-prefixed rkyv archives. NOTA is a projection format for humans, CLIs,
harness prompts, and debug output.

```mermaid
sequenceDiagram
    participant Human as human or harness
    participant Message as persona-message
    participant Signal as persona-signal
    participant Router as persona-router
    participant Store as persona-store
    participant Harness as persona-harness

    Human->>Message: NOTA input
    Message->>Signal: typed Frame
    Signal->>Router: length-prefixed rkyv
    Router->>Store: transition Frame
    Store-->>Router: commit reply
    Router->>Harness: delivery request
    Harness-->>Human: NOTA projection
```

## 3 · State and Ownership

`persona-store` owns the assembled runtime's durable write boundary. During
parallel development, component repos may keep local redb stores so their CLIs
and tests are useful in isolation. At assembly time those stores become table
views composed by `persona-store`.

```mermaid
flowchart LR
    "component CLI" -->|"typed request"| "component library"
    "component library" -->|"local tests"| "component-local redb"
    "component library" -->|"assembled runtime"| "persona-store table view"
    "persona-store table view" -->|"write transaction"| "unified redb"
```

The component remains the schema authority for its own record shapes.
`persona-store` owns ordering, transactions, schema-version checks, and durable
commit visibility.

## 4 · Boundaries

This repository owns:

- apex architecture;
- Nix flake inputs and component composition;
- end-to-end tests that prove component composition;
- deployment wiring for a full Persona system.

This repository does not own:

- shared signal records (`persona-signal`);
- router policy (`persona-router`);
- terminal transport (`persona-wezterm`);
- harness lifecycle internals (`persona-harness`);
- OS/window-manager adapters (`persona-system`);
- durable database internals (`persona-store`);
- workspace coordination internals (`persona-orchestrate`).

## 5 · Invariants

- The meta repo composes; component repos implement.
- Rust-to-Rust component traffic uses `persona-signal` rkyv frames.
- NOTA appears only at human, harness, CLI, and audit projection boundaries.
- Producers push; consumers subscribe. Polling is not a fallback.
- Harnesses are first-class records, not hidden terminal sessions.
- Durable writes in the assembled runtime pass through `persona-store`.
- Every component remains testable in isolation through its library, CLI, and
  tests.

## Code Map

```text
ARCHITECTURE.md  apex system shape
skills.md        how to work in the meta repo
flake.nix        component flake composition
src/             temporary schema stub while component repos absorb runtime
tests/           schema tests and multi-component end-to-end tests
```

## See Also

- `../persona-signal/ARCHITECTURE.md`
- `../persona-message/ARCHITECTURE.md`
- `../persona-router/ARCHITECTURE.md`
- `../persona-system/ARCHITECTURE.md`
- `../persona-harness/ARCHITECTURE.md`
- `../persona-wezterm/ARCHITECTURE.md`
- `../persona-store/ARCHITECTURE.md`
- `../persona-orchestrate/ARCHITECTURE.md`
- `~/primary/reports/designer/19-persona-parallel-development.md`
- `~/primary/reports/operator/10-persona-parallel-implementation.md`
