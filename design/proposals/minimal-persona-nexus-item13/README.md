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
It adopts no provider target, daily count, or pacing estimate.

`SemaEngineQuotaStorage` is the concrete persistence adapter. It registers the
typed `persona_quota_windows` table, reads it through sema-engine's read-only
storage reader, and commits an explicit `assert` or `mutate` request. Its
dependencies are immutable: `nexus` is pinned to
`c495f2acbfff57e017092b9cc1fbf9f73ca2badf` and `sema-engine` to
`516f01fe0b03157efc6cc3d38b588f0ca123ac94`. The test fixture instead uses a
pure recording `QuotaWindowStorage`; it proves the assert-then-mutate decision
without opening a `.sema` file. Opening an `Engine` remains runtime work owned
by a real Nexus and is deliberately not exercised here.

## Historical item 10 relationship

`../minimal-engine-catalog-poc` remains the historical item-10 catalog draft.
It intentionally differs: it has a single catalog crate and preserves supplied
quota snapshots. This item-13 proposal replaces none of that history; it adds
the Nexus/socket/closed-vocabulary and reset-invalidating ledger shape.

## Witness

```sh
cargo test --manifest-path design/proposals/minimal-persona-nexus-item13/Cargo.toml
```
