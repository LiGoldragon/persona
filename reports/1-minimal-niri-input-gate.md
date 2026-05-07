# Minimal Niri input gate

Status: draft for audit
Author: Codex (operator)

This report narrows the input-management design to the smallest useful local
implementation: a Persona router that uses Niri's IPC event stream for focus
wakeups, a harness actor for each running harness, and a delivery gate that
queues messages until the target harness is unfocused, idle, and has an empty
prompt.

The design stays inside the push-not-pull rule. The router subscribes only when
there is blocked work, and it wakes only on producer events.

## Minimal scope

```mermaid
flowchart LR
    sender[message sender]
    router[Persona router]
    actor[HarnessActor]
    niri[Niri event stream]
    terminal[terminal endpoint]
    log[(message log)]

    sender --> router
    router --> log
    router --> actor
    actor --> terminal
    niri --> router
```

The first version owns only:

| Piece | Responsibility |
|---|---|
| Router | ordered message queue, pending delivery state, subscriptions |
| HarnessActor | target identity, endpoint, screen/prompt state, delivery method |
| NiriFocusSource | `$NIRI_SOCKET` event stream, focused window map |
| Endpoint | terminal/PTY injection only after the gate says safe |
| Message log | audit of accepted, queued, delivered, deferred |

It does not own the future desktop composer, compositor lease, native prompt
drafts, or full Persona state reducer yet.

## Why Niri works for the focus half

Niri exposes an IPC socket through `$NIRI_SOCKET`. Its event stream request
returns current state up front and then streams updates. Niri documents this as
the way to update without continuous polling.

```mermaid
sequenceDiagram
    participant R as router
    participant N as Niri socket

    R->>N: EventStream request
    N-->>R: current compositor state
    loop external focus/window changes
        N-->>R: event update
    end
```

The focus source is a producer. The router is a subscriber. The router does not
ask Niri "who is focused?" on a timer.

## Delivery gate

A terminal fallback delivery is safe only when all gate inputs are green:

```mermaid
flowchart TD
    message[pending message]
    focus{target focused?}
    prompt{prompt empty?}
    idle{harness idle?}
    deliver[deliver through endpoint]
    queue[stay queued]

    message --> focus
    focus -->|yes| queue
    focus -->|unknown| queue
    focus -->|no| prompt
    prompt -->|no or unknown| queue
    prompt -->|yes| idle
    idle -->|no or unknown| queue
    idle -->|yes| deliver
```

Every queued result records a typed block reason:

| Block reason | Producer that can wake it | First implementation |
|---|---|---|
| `blocked_on_focus` | Niri focus/window event stream | yes |
| `blocked_on_non_empty_prompt` | harness screen-state actor | fixture first, live later |
| `blocked_on_busy` | harness screen-state actor | fixture first, live later |
| `blocked_on_unknown` | none | stays queued |

## Router state machine

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Queued: message accepted
    Queued --> GateCheck: attempt delivery
    GateCheck --> Delivered: safe
    GateCheck --> WaitingForFocus: target focused
    GateCheck --> WaitingForPrompt: prompt occupied
    GateCheck --> WaitingForIdle: harness busy
    GateCheck --> WaitingUnknown: state unknown
    WaitingForFocus --> GateCheck: FocusChanged(target, false)
    WaitingForPrompt --> GateCheck: InputRegionChanged(target, empty)
    WaitingForIdle --> GateCheck: IdleStateChanged(target, idle)
    WaitingUnknown --> GateCheck: human retry / new authoritative event
    Delivered --> [*]
```

The router stores pending messages by target. The target actor stores the last
authoritative focus, prompt, and idle observations. A delivery attempt consumes
that state; it does not perform a polling loop to make it fresh.

## Conditional Niri subscription

The router does not need to subscribe to Niri forever. It subscribes when a
pending delivery is blocked on focus and unsubscribes when no pending delivery
needs focus wakeups.

```mermaid
sequenceDiagram
    participant R as router
    participant Q as pending queue
    participant N as NiriFocusSource
    participant A as HarnessActor

    R->>A: attempt message
    A-->>R: defer blocked_on_focus
    R->>Q: record pending(target, focus)
    R->>N: ensure subscribed
    N-->>R: FocusChanged(target, false)
    R->>A: attempt pending for target
    A-->>R: delivered or new block reason
    alt no pending focus blocks
        R->>N: unsubscribe / close stream
    end
```

This is intentionally event-gated. The router can keep the stream open while
there are any focus-blocked targets. It closes the stream when focus events no
longer matter to pending work.

## Niri focus map

Niri's event stream gives a current-state prelude, then updates. The focus
source converts that into a map the router can use:

```mermaid
flowchart TB
    event[raw Niri event]
    map[window focus map]
    registry[harness target registry]
    focus[FocusChanged target focused]

    event --> map
    map --> registry
    registry --> focus
```

The harness registry binds a Persona harness to a Niri window id:

| Persona target | Niri field |
|---|---|
| `HarnessTarget` | durable Persona identity |
| `window_id` | Niri toplevel window id |
| `app_id` / title | discovery hints only |
| endpoint | terminal/PTY delivery object |

`app_id` and title are not stable identity. They help discovery. Once a harness
is registered, the target-window binding is explicit.

## Actor ownership

```mermaid
flowchart LR
    router[RouterActor]
    harness[HarnessActor]
    focus[NiriFocusSourceActor]
    screen[ScreenStateActor]
    endpoint[EndpointActor]

    router --> harness
    router --> focus
    harness --> screen
    harness --> endpoint
```

The verb belongs to the noun:

| Actor | Owns | Methods / messages |
|---|---|---|
| `RouterActor` | queue, subscriptions, routing state | accept, attempt, subscribe, expire |
| `HarnessActor` | target state, endpoint, gate inputs | decide delivery, deliver, defer |
| `NiriFocusSourceActor` | Niri socket stream, focus map | subscribe, unsubscribe, emit focus |
| `ScreenStateActor` | parsed terminal screen state | emit prompt/idle changes |
| `EndpointActor` | delivery channel | submit terminal message |

No free function owns delivery. Delivery is a method/message on the target
harness actor because the actor owns the endpoint and the current gate state.

## Minimal data flow

```mermaid
sequenceDiagram
    participant C as client
    participant R as RouterActor
    participant H as HarnessActor
    participant F as NiriFocusSource
    participant E as EndpointActor

    C->>R: Send(target, body)
    R->>H: AttemptDelivery(message)
    H-->>R: Deferred(blocked_on_focus)
    R->>F: SubscribeFocus(target window)
    F-->>R: FocusChanged(target, false)
    R->>H: AttemptDelivery(message)
    alt prompt empty + idle
        H->>E: Submit(message)
        E-->>H: Submitted
        H-->>R: Delivered
    else another block reason
        H-->>R: Deferred(reason)
    end
```

## Race policy

The minimal Niri gate reduces the known race but does not eliminate it.

```text
safe observation
  -> human can still type before terminal bytes are submitted
```

So the policy remains:

| Situation | Action |
|---|---|
| target focused | queue |
| target unfocused, prompt occupied | queue |
| target unfocused, prompt empty, harness idle | submit |
| any unknown | queue |

The future focus lease removes more of the race. The minimal Niri design is the
first step because focus events are already available and cheap.

## Tests before implementation

```mermaid
flowchart TD
    fixture[recorded Niri event fixtures]
    fake[FakeFocusSource]
    router[RouterActor tests]
    live[Niri live test]
    harness[visible harness collision test]

    fixture --> fake
    fake --> router
    router --> live
    live --> harness
```

Required tests:

| Test | Expected result |
|---|---|
| current-state prelude says target focused | message stays queued |
| `FocusChanged(target,false)` arrives | router attempts delivery once |
| focus changes for unrelated window | target queue is untouched |
| prompt occupied after focus clears | message moves to prompt block |
| no Niri socket | focus-gated delivery unavailable, message queued |
| visible human typing collision | no splice; target focused or prompt occupied blocks |

The tests use fake event sources first. The live Niri test comes after the
router state machine passes with recorded JSON lines.

## Audit questions

| Question | Why it matters |
|---|---|
| Is conditional subscription worth the complexity, or should the focus stream stay open while the router runs? | The user asked for subscribe-on-block; the event stream is still push either way |
| Is Niri window id stable enough for a running harness binding? | The harness registry needs an explicit rebinding story after window close/reopen |
| Should prompt and idle events block implementation until live screen parsing exists? | Without them, the focus half works but delivery remains too permissive |
| Should `WaitingUnknown` be manually dischargeable only? | Push-not-pull forbids a retry timer |
| Does this belong in `persona-router` as a separate repo before coding? | The router state machine is already its own component shape |

## Sources

- Niri IPC and event stream: <https://yalter.github.io/niri/IPC.html>
- Niri IPC crate docs: <https://yalter.github.io/niri/niri_ipc/>
- Niri request docs for `EventStream`: <https://yalter.github.io/niri/niri_ipc/enum.Request.html>
