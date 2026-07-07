# Agent Instructions - Persona

## Repo Role

Persona is the engine manager for coordinating multiple AI harnesses and
component daemons as one inspectable system. The manager surface reports engine
status, component health, and supervisor actions through the Persona manager
signal contract.

## Current Phase

This repo is in apex integration phase. Implementation code here stays limited
to the top-level engine-manager runtime stub, wire-test shims, Nix
composition, and end-to-end witnesses. Component behavior belongs in the
component repo that owns the concern.

## Version Control

Persona is a Git-backed colocated Jujutsu repository. Use `jj` for local
history work (`jj st`, `jj diff`, `jj commit`, `jj rebase`, `jj git push`) and
keep Git as the remote/storage compatibility layer.

## Writing Rules

- Reports live in `reports/`.
- Reports use prose plus visuals: ASCII diagrams, Mermaid charts, tables, and
  swimlanes.
- Keep implementation code out of reports.
- Architecture docs describe the present direction at a high level.
- When implementation begins, Rust uses methods on types, typed domain
  values, one object at each boundary, one crate error enum, direct Kameo
  actors for runtime logic, and sema-engine/Sema for durable typed storage.
- Persona CLI input and output are NOTA text unless a future command is
  explicitly binary.
