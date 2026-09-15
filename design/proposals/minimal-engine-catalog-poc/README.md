# Minimal engine catalog proof of concept

This is an uninstalled, undeployed design draft. It is a standalone Rust
package containing only in-memory catalog state. It has no executable, sockets,
processes, providers, account access, service definitions, activation, or live
engine behavior.

The catalog keeps desired generation separately from the last observed
generation for each nominal fixture engine ID. Observations older than the
stored observation are rejected. A newer observation is recorded even when its
generation mismatches the desired one, so reporting can describe what was
actually seen.

Quota snapshots retain the provider-reported source timestamp, reset interval,
remaining amount, pace, and reset interpretation. `QuotaPace` is supplied as
observed/accounted data; this draft does not derive or adopt a daily target.

## Contract and implementation

`contract/engine_catalog.ethos` is a draft data contract. `src/lib.rs` is a
hand-written, trait-first implementation: all catalog behavior is declared on
traits and implemented for `InMemoryCatalog`. The Ethos file is not yet a
generation input and this package deliberately carries no wire or Nexus
surface.

## Witnesses

```sh
cargo test --manifest-path design/proposals/minimal-engine-catalog-poc/Cargo.toml
nix flake check --no-build path:design/proposals/minimal-engine-catalog-poc
```

The Nix check is draft-only: it exposes the Cargo test command for a configured
remote builder and does not activate or deploy anything. Its Nixpkgs revision
is pinned directly in `flake.nix`; no local Nix build was run. The fixture IDs
in the tests are provisional.

## Limits

This proof of concept is not an engine controller, collector, scheduler, quota
policy, or persistence design. It intentionally does not choose a daily quota
target or interpret a reset timestamp beyond preserving the supplied
interpretation.
