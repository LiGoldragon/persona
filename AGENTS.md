# Agent Instructions - Persona

You MUST read lore's `AGENTS.md` - the workspace-wide contract.

## Repo Role

Persona is the framework for coordinating multiple AI harnesses as one
inspectable system. The first project surface is harness-to-harness
messaging: durable messages, live subscriptions, direct harness input,
observed harness output, and explicit authorization.

## Current Phase

This repo is in apex integration phase. Implementation code here stays limited
to schema stubs, wire-test shims, Nix composition, and end-to-end witnesses.
Component behavior belongs in the component repo that owns the concern.

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
- When implementation begins, Rust follows lore's Rust style: methods on
  types, typed domain values, one object at each boundary, one crate error
  enum, direct Kameo actors for runtime logic, redb/rkyv for durable typed
  storage.
- Persona CLI input and output are NOTA text unless a future command is
  explicitly binary.
