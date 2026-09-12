# UPGRADES

## 0.2.0 → 0.5.0 — Datom stack across the whole contract closure

### What breaks

1. **Every contract this manager speaks moves at once.** signal-message
   3.0.0, signal-harness 4.0.0, signal-persona 4.0.0, signal-mind 3.0.0,
   signal-router 5.0.0, signal-terminal 3.0.0, signal-upgrade 3.0.0,
   meta-signal-upgrade 3.0.0, meta-signal-system 3.0.0,
   meta-signal-persona 3.0.0, signal-introspect 3.0.0, signal-system 3.0.0.
   Every managed component's wire and typed startup configuration changes
   with them.
2. **The binary daemon configurations persona writes are re-shaped.** The
   router daemon configuration in particular lost its `Parts` indirection and
   its newtype-per-alias projection, so a configuration file written by an
   older persona will not decode in a current router.
3. **The `dotos` dialect is gone** from the whole graph — manifest, lock and
   text surface. `triad-runtime` renamed its argument surface accordingly.

### Deploying

Deployment is a CriomOS-home flake pin advance and is **not** performed by
this repository. Because persona writes each managed component's startup
configuration, the pin advance must carry persona **and every component it
launches** in one step: message, router, mind, terminal, harness, introspect,
spirit, system. A persona of this version writing a configuration for an
older component daemon produces a file that daemon cannot decode.

Component state that must be moved aside rather than migrated is documented
in each component's own `UPGRADES.md`; `message`'s durable store is one such
case and fails closed on an old file.

### Known boundary

The check `persona-message-daemon-stamps-origin-via-tap` was removed, with
the reason recorded in `flake.nix` where it stood. It asserted that
`message-daemon` forwards a stamped submission to the router socket;
`message` has never contained router-forwarding code at any revision, so the
check could not pass. Restore it when `message` forwards.
