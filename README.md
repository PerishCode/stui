# stui

`stui` is a cross-platform UI/runtime experiment focused on architecture validation first.

## Priority order

1. Architecture validation
2. Performance exploration
3. Usable prototype

## Vision

`stui` exists to imitate current best practices, discard historical baggage, and iterate aggressively toward a cleaner UI/runtime model.

This project is allowed to break, restart, and replace its own ideas whenever a better abstraction appears.

One long-term proving ground for this work is an open **3D Agent desktop pet**: a system with form-modeling and motion-modeling primitives, embodied Agent behavior, irregular interaction surfaces, UI management tooling, and room for peripheral communication.

## Design principles

- Prefer clean architecture over compatibility.
- Validate abstractions before investing in tooling or polish.
- Keep the implementation small enough to understand and rebuild.
- Treat performance as an early constraint, but not the first optimization target.
- Build prototypes only far enough to expose architectural truth.

## Non-goals

- Compatibility with existing framework APIs
- Stable public interfaces
- Production-readiness guarantees
- Preserving bad abstractions for short-term convenience
- Solving every platform in the first pass

## Exit criteria

`stui` continues until it either becomes genuinely useful, or the experiment stops being interesting.

## Status

Current scope is intentionally minimal.

## Workspace layout

- `lib/crates/*`: core library modeling and reusable runtime pieces
- `playgrounds/*`: focused Rust experiments used to validate ideas quickly
- `scripts/*`: Rust-based developer scripts and operational tooling

Current examples of intent:

- `scripts/stui-dev`: development launcher/orchestrator
- `scripts/stui-pack`: packaging-oriented script crate

Initial crate split:

- `lib/crates/stui-core`: modeling + ports only
- `lib/crates/stui-runtime`: concrete runtime declarations for current experiments
- `lib/crates/stui-platform-desktop`: desktop host planning/adaptation layer
- `playgrounds/black-box`: smallest host/surface experiment

## Current milestone

The next milestone is a minimal rendering loop:

- launch through `stui-dev`
- present a small black undecorated host surface
- avoid treating a native window as the default core abstraction
- shut down gracefully when the host requests exit

## Script conventions

- script crates use `clap`
- script crates bootstrap `Config` separately from `AppState`
- installed usage is preferred over ad hoc `cargo run -p ...`

## Current runnable path

- Install-oriented entrypoint: `cargo install --path scripts/stui-dev`
- Local debug entrypoint: `cargo run -p stui-dev --`

`stui-dev` is now action-first and supervisor-oriented:

- `cargo run -p stui-dev -- targets`
- `cargo run -p stui-dev -- prune`
- `cargo run -p stui-dev -- start black-box`
- `cargo run -p stui-dev -- status black-box`
- `cargo run -p stui-dev -- status-all`
- `cargo run -p stui-dev -- inspect black-box`
- `cargo run -p stui-dev -- events black-box`
- `cargo run -p stui-dev -- logs black-box --lines 40`
- `cargo run -p stui-dev -- stop black-box`

Important boundary:

- `stui-dev` no longer links directly to playground/runtime code
- it builds with `cargo build`, then supervises the real playground binary directly
- long-lived management no longer rides on top of `cargo run`
- control/status/inspect/events execute against the binary or IPC surface with finite timeouts
- `start` now reports build/launch/ready stages, `status` reports session/stale reason, and `logs` defaults to a bounded tail view
- `targets` and `status-all` now let `stui-dev` expose its target surface and aggregate supervisor view even before a second playground exists
- `prune` now gives `stui-dev` a bounded stale-session cleanup path, and `start` uses a bounded retry before failing readiness

Today `start black-box` opens the black-box playground as a small undecorated desktop host surface, supports graceful exit through host close requests or `Esc`, and exposes a tiny local debug landing:

- press `1` to force `Booting`
- press `2` to force `Idle`
- press `3` to force `Closing`
- runtime snapshots and host/present capabilities are printed to stdout for inspection

There is also a direct playground command boundary for non-GUI snapshot work when needed:

- `cargo run -p stui-playground-black-box -- snapshot`
- `cargo run -p stui-playground-black-box -- snapshot --behavior idle`
- `cargo run -p stui-playground-black-box -- snapshot --format json`

This keeps scripted runtime inspection available without forcing `stui-dev` to become an in-process API client.

A minimal local IPC path now exists too:

- the new `stui-ipc` crate provides namespace-aware local channel naming plus a tiny request/response transport primitive
- channel names are prefixed by `STUI_IPC_CHANNEL_PREFIX` when present, with a local default otherwise
- black-box mounts a thin example capability on top of that primitive rather than the primitive hard-coding black-box semantics
- transport lookup failures now surface a typed `channel not published` error instead of a bare missing-file detail
- channel bind collisions now surface a typed `channel already occupied` error for the same namespaced instance
- stale registry entries are pruned when their published address is no longer reachable
- the shared IPC convention now lives in `stui-ipc` for success/error envelopes plus event-catalog JSON construction
- the black-box IPC example now reuses that shared `{ ok, kind|error, data }` convention instead of hand-building its own envelope strings
- the events surface now has a real minimal endpoint: the control side exposes a catalog, and the events side supports `poll` to drain queued runtime events
- the polling events surface now has an explicit queue policy: capacity `32`, `drop-oldest`, and poll responses report drained count plus dropped counts
- the default request timeout, poll idle sleep, and event queue capacity now come from a shared `stui-ipc::IpcPolicy` default rather than black-box-only constants
- `stui-dev` now has its own higher-layer namespace env var, `STUI_DEV_IPC_NAMESPACE_PREFIX`, which feeds channel naming without reusing the lower-level `STUI_IPC_CHANNEL_PREFIX` name

Example flow:

- set namespace at the `stui-dev` layer: `$env:STUI_DEV_IPC_NAMESPACE_PREFIX = "stim.dev"`
- start one instance: `cargo run -p stui-dev -- start black-box --instance alpha`
- start another instance: `cargo run -p stui-dev -- start black-box --instance beta --behavior closing`
- inspect one instance: `cargo run -p stui-dev -- inspect black-box --instance alpha`
- poll one instance events: `cargo run -p stui-dev -- events black-box --instance beta`
- stop one instance: `cargo run -p stui-dev -- stop black-box --instance alpha`

The lifecycle is now runtime-driven:

- platform translates desktop events into runtime events
- runtime decides redraw, visibility, and exit commands
- desktop adapter executes those commands without becoming the business authority

Close handling is now explicit too:

- platform raises a close request with a source
- runtime returns a close decision
- host/surface snapshots are synchronized back into runtime state as the loop evolves

Render flow is now one step more general:

- `stui-core` models a minimal present intent
- runtime emits `Present(...)` instead of a black-box-only render command
- desktop execution interprets that present intent for the current host surface

Presenting is now also a real port boundary:

- `PresentPort` defines state inspection plus present execution
- desktop presentation now lives in an explicit presenter implementation
- the runner orchestrates runtime and host flow without owning the presentation details directly

The present model now has more than one shape:

- `ClearSolid` remains the minimal base intent
- `ClearInset` proves the model can describe simple layered composition without collapsing back into ad hoc desktop logic
- the black-box experiment now uses a near-black framed variant to exercise that path

Host and present semantics are now slightly more formal too:

- `HostPort` exposes host capabilities in addition to descriptor and state
- `PresentPort` exposes present capabilities in addition to descriptor, state, and execution
- the desktop path now uses explicit host and presenter objects rather than a runner owning every concern directly

Runtime semantics now also include a tiny component layer:

- the black-box runtime owns a root component state and a fill component state
- present output is now derived from component state plus surface extent
- this keeps the experiment on the declarative side of the line instead of hard-coding every present model directly in the event loop

That component layer now also has an explicit minimal tree relation:

- component descriptors can declare a parent
- the black-box fill component is explicitly modeled as a child of the root component
- this keeps the system moving toward semantics-bearing structure without jumping to reconciliation or layout machinery

Runtime now also performs a tiny explicit resolution step:

- component state first resolves into a runtime-owned tree result
- present output is derived from that resolved tree, not directly from raw component fields
- this keeps the next step toward scene/form semantics visible without widening scope too fast

The next semantic step is now present too:

- runtime has a minimal behavior phase (`Booting`, `Idle`, `Closing`)
- behavior phase influences component resolution before presentation
- this is the first step where runtime semantics shape not just what exists, but how the system is currently behaving

That behavior/component path now includes a tiny inherited semantic:

- the root resolves a fill semantic token before the child resolves its final color
- child meaning is no longer derived only from its own local state
- this is the first small sign of semantic propagation through the component relation

A minimal layer hook now exists too:

- `PresentModel` now carries a layer role
- root behavior/component resolution assigns a layer semantic to the fill child
- the current desktop presenter does not yet branch on layer, but the semantic is now part of the boundary instead of a future bolt-on

## Long-term architectural pressure test

The black-box milestone is intentionally small, but it is not directionless. One long-term target is a desktop 3D Agent pet product shape that pressures the architecture to support:

- embodied foreground interaction rather than only conventional rectangular apps
- open-ended form and motion modeling primitives
- management UI surfaces alongside the primary interactive entity
- peripheral/device communication paths
- host/surface/runtime boundaries that survive more than one presentation mode

See [docs/project.md](docs/project.md) for the working manifesto.
