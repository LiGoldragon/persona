# Persona item 13: minimal Nexus draft

This is a draft proposal only. It neither opens sockets nor starts processes,
providers, accounts, services, or harnesses. The tests use only pure fake
adapters.

The layout deliberately follows Orchestrate's three repository roles:

| crate | draft responsibility |
| --- | --- |
| `signal-persona` | closed ordinary Signal vocabulary |
| `meta-signal-persona` | closed privileged/meta Signal vocabulary |
| `persona-nexus` | Nexus Core seams, launch seams, and quota-ledger rules |

`PersonaNexus::receive_signal` accepts `Signal` only. `InlineDatomTranslation`
is an external CLI boundary: inline Datom is translated before the ordinary or
meta socket receives the typed signal. Flow roster is intentionally absent;
that belongs to Orchestrate.

The two `DraftLaunch` methods receive an already assembled first prompt and
hand it to the supplied harness exactly once. They are seams, not process
launchers.

The ledger is keyed by subscription. A later reset window replaces the former
window; an old-window sample is `Unknown(SupersededWindow)`. A future source
timestamp and a zero reset interval are respectively explicit Unknown states.
It adopts no provider target, daily count, or pacing estimate. `SemaEngineQuotaStore`
is the sema-engine persistence seam; its fixture is a pure in-memory adapter,
so this draft does not open a `.sema` file.

## Historical item 10 relationship

`../minimal-engine-catalog-poc` remains the historical item-10 catalog draft.
It intentionally differs: it has a single catalog crate and preserves supplied
quota snapshots. This item-13 proposal replaces none of that history; it adds
the Nexus/socket/closed-vocabulary and reset-invalidating ledger shape.

## Witness

```sh
cargo test --manifest-path design/proposals/minimal-persona-nexus-item13/Cargo.toml
```
