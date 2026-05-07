# persona skill

Persona is the high-level integration repository for the multi-harness system.
Keep this repo focused on architectural integration, schema-facing stubs, and
reports that explain how the component repositories fit together.

Current component boundaries:

- `persona-signal` owns the shared rkyv frame contract.
- `persona-store` owns the durable database and transaction boundary.
- `persona-router` owns delivery decisions and pending-delivery state.
- `persona-system` owns OS and window-manager observations.
- `persona-harness` owns harness actor lifecycle.
- `persona-message` owns the human/harness NOTA CLI boundary.

Do not duplicate contract records here once they belong in `persona-signal`.
Do not open Persona's main database directly once `persona-store` exists.

