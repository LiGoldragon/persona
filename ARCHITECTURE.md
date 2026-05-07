# Persona Architecture

Persona is the meta-repository for the multi-harness system. It coordinates the
component repositories through architecture docs and Nix integration. The core
runtime design is still a single reducer-owned state machine: all durable
harness, message, delivery, interaction, and observation changes land as typed
transitions, but the implementation of those transitions belongs to the
component crates.

The initial deployable shape is a set of small daemons and tools with one typed
message fabric. Harnesses are the operational unit: each harness has a durable
identity, a live process when running, an inbound message path, an outbound
observation path, and an explicit authorization context.

## Role

The Persona meta-repo owns:

- the top-level architecture view;
- component composition through Nix;
- eventual NixOS module wiring for a full Persona deployment;
- integration reports that explain how the component repositories fit together.

The component repositories own:

- `persona-signal`: shared rkyv frame contract;
- `persona-store`: durable database and transition commits;
- `persona-router`: delivery decisions and pending-delivery state;
- `persona-system`: OS and window-manager observations;
- `persona-harness`: harness actor lifecycle;
- `persona-message`: human and harness NOTA projection.

Persona does not own:

- model inference itself;
- provider billing or account policy;
- project-specific agent role prompts;
- component-internal daemon code;
- component-internal database tables;
- terminal adapter implementations.

## Starting Map

```mermaid
flowchart LR
    "persona meta-repo" -->|"Nix composition"| "persona-signal"
    "persona meta-repo" -->|"Nix composition"| "persona-store"
    "persona meta-repo" -->|"Nix composition"| "persona-router"
    "persona meta-repo" -->|"Nix composition"| "persona-system"
    "persona meta-repo" -->|"Nix composition"| "persona-harness"
    "persona meta-repo" -->|"Nix composition"| "persona-message"

    "persona-message" -->|"typed frame"| "persona-signal"
    "persona-router" -->|"commit transition"| "persona-store"
    "persona-router" -->|"gate query"| "persona-system"
    "persona-router" -->|"delivery request"| "persona-harness"
    "persona-harness" -->|"projection boundary"| "persona-message"
```

## Invariants

- The meta-repo composes; component repos implement.
- Harnesses are first-class records, not hidden terminal sessions.
- Producers push; consumers subscribe.
- Durable state and live process state are separate records.
- The message fabric is typed before it is clever.
- A delivery attempt produces observable state whether it succeeds, waits, or
  fails.
- Authorization is part of the route, not an afterthought.
- The core state machine is singular; extension state is peripheral and feeds
  the core through typed commands or observations.

## Code Map

```text
flake.nix       top-level component composition
README.md       repo orientation
ARCHITECTURE.md high-level system map
src/            temporary schema stub until component repos absorb the runtime
```

## Status

Implementation in this repo is a schema scaffold and integration wrapper. New
runtime implementation should land in the component repositories first, then be
composed here through Nix.
