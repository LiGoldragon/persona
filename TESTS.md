# Test architecture — `persona` meta repo

How tests across multiple Persona components are organised
in this repo and run via Nix.

This document is the per-repo test-architecture record per
the workspace's "test architecture documents in
repositories" pattern. When a new cross-component test
lands, update this file with its shape + witnesses.

---

## What lives here

The `persona` meta repo holds **cross-component tests** —
tests that exercise more than one Persona component
together, using each component's published contract repo
as the integration surface.

Tests that exercise a single contract or component live
in **that contract's or component's own** `tests/`
directory, not here.

---

## The test surfaces

### 1. Cargo unit/integration tests (`tests/*.rs`)

Standard `cargo test` paths. Each test file is one
integration test. Currently:

- `tests/request.rs` — request shapes
- `tests/schema.rs` — schema declarations
- `tests/state.rs` — state engine

### 2. Wire-test shim binaries (`src/bin/wire_*.rs`)

Small CLI binaries that exercise the Signal contract repos
end-to-end through real bytes on stdin/stdout. **Used by
the Nix-chained derivations below**, not by `cargo test`.

| Binary | Role |
|---|---|
| `wire-emit-message` | Construct a `signal_persona_message::Frame` containing a `Submit`, encode length-prefixed, write to stdout |
| `wire-decode-message` | Read length-prefixed bytes from stdin; decode as `signal_persona_message::Frame`; assert `--expect-recipient` / `--expect-body` match |
| `wire-relay-message-to-store` | Read message frame from stdin; transform inner `SubmitMessage` to `signal_persona_store::CommitMessage`; emit new frame on stdout (the router-shaped step) |
| `wire-decode-store` | Read store frame from stdin; assert `--expect-recipient` / `--expect-sender` / `--expect-body` match |

Each shim is intentionally **terse** — it does one
encode-or-decode operation and exits. The architecture-truth
witnesses come from the chaining, not from inside the shim.

### 3. Nix-chained derivations (`flake.nix#checks`)

The architectural-truth witness chain. Each step is an
isolated Nix derivation; **no in-process memory can fake
the chain**.

```mermaid
flowchart LR
    step1[wire-step-1-emit-message<br/>derivA: shim writes bytes]
    step2[wire-step-2-relay-message-to-store<br/>derivB: shim reads derivA's output, writes new bytes]
    step3[wire-step-3-decode-store<br/>derivC: shim reads derivB's output, asserts content]
    rt[wire-message-channel-round-trip<br/>shim emit | shim decode]

    step1 --> step2 --> step3
```

Per `~/primary/skills/architectural-truth-tests.md`
§"Nix-chained tests — the strongest witness", these
derivations exist because:

- The writer's output is the *only* path between writer
  and reader (no shared memory, no shared filesystem
  collusion)
- The reader is a separate binary; can't be tricked by the
  writer's mock
- The output is content-addressed (`/nix/store/<hash>-step-1-emit-message`);
  any byte change surfaces as a hash change, not a flaky
  comparison

What each chain step proves:

| Check | Witnesses |
|---|---|
| `wire-step-1-emit-message` | The `signal_channel!`-emitted `MessageRequest::Submit` constructor works; `Frame::encode_length_prefixed` produces real bytes |
| `wire-step-2-relay-message-to-store` | The router-shaped translation between two Signal channels works; `signal-persona-message` and `signal-persona-store` both decode/encode through their macro-emitted types |
| `wire-step-3-decode-store` | The message → relay → store pipeline preserves the user's intent (recipient + body) and adds the router-supplied sender |
| `wire-message-channel-round-trip` | Single-channel sanity check; catches macro-level breakage in `signal-persona-message` independently of the relay |

Run all of them at once:

```sh
nix flake check
```

The output names each derivation; failures point at the
specific step that broke.

---

## When a new contract gets added

Adding `signal-persona-<channel>` should also add a
matching nix-chained check in this repo. Pattern:

1. Add `<channel>` to the deps in `Cargo.toml`
2. Add shim bins for the new channel: `wire_emit_<channel>.rs`,
   `wire_decode_<channel>.rs`
3. Add `[[bin]]` entries in `Cargo.toml`
4. Add nix derivations in `flake.nix#checks` chaining the
   shims (per the worked examples above)
5. Update this document with the new step + witness table

The witness pattern is the same: each step is one
derivation; bytes flow between via the file system
(`runCommand`'s `$out`); no in-process fakery possible.

---

## What the wire test does NOT do

- It does NOT exercise the actual `persona-router` daemon —
  the relay shim is a stand-in. Real router lands in
  Phase 5 of `~/primary/reports/designer/72-harmonized-implementation-plan.md`.
- It does NOT exercise `persona-orchestrate` (the future
  state actor) — `wire-decode-store` asserts on bytes, not
  on database state. End-to-end-with-database lands when
  `persona-orchestrate` ships its sema-backed implementation.
- It does NOT yet test `wire-decode-message` against
  `wire-relay-message-to-store`'s output (the relay's
  store-side output isn't decoded as a message frame —
  that wouldn't make sense).

---

## See also

- `~/primary/skills/architectural-truth-tests.md` — the
  test discipline this fixture demonstrates
- `~/primary/reports/designer/72-harmonized-implementation-plan.md`
  §2.1 — channel inventory
- `~/primary/reports/designer/73-signal-derive-research.md`
  — the `signal_channel!` macro design
- `signal-persona-message/` — the message channel contract
  (consumed here)
- `signal-persona-store/` — the store channel contract
  (consumed here)
- `signal-core/src/channel.rs` — the macro
