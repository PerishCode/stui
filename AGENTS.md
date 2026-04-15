# AGENTS

This file is the repository's documentation entrypoint and the single source of truth for active project guidance.

- Do not maintain a parallel top-level `README.md` as a competing overview or instructions document.
- If high-level project guidance, workflow policy, or operator-facing usage changes, update `AGENTS.md` first.
- Treat other docs as supporting material only; they must not override or drift from `AGENTS.md`.

This repository is being grown as a learning-oriented cross-platform UI/runtime experiment.

Current scope is intentionally minimal.

## Project stance

- Priority order:
  1. Architecture validation
  2. Performance exploration
  3. Usable prototype
- Prefer clean architecture over compatibility.
- Do not preserve legacy paths, compatibility shims, or backward-compatibility wording unless the user explicitly asks for them.
- Validate abstractions before investing in tooling or polish.
- Keep implementations small enough to understand, rebuild, and replace.
- Treat performance as an early structural concern, but not the first optimization target.
- The project is allowed to break, restart, and replace its own ideas when a better abstraction appears.

## Workspace structure

- `lib/crates/*`: core modeling, reusable runtime pieces, and platform adapters
- `playgrounds/*`: narrow proving-ground experiments used to validate ideas quickly
- `scripts/*`: Rust-based development and operational tooling

Current intended roles:

- `lib/crates/stui-core`: modeling + ports only
- `lib/crates/stui-runtime`: concrete runtime declarations for current experiments
- `lib/crates/stui-platform-desktop`: desktop host/present adaptation layer
- `playgrounds/black-box`: smallest host/surface proving cell
- `scripts/stui-dev`: supervisor-oriented development control surface
- `scripts/stui-pack`: packaging-oriented script crate

## Long-term product vision

One long-term proving ground for this architecture is a **3D desktop Agent pet**.

The intended shape is not just a desktop toy, but an open system that can:

- render a 3D desktop pet as a first-class host/surface-driven experience
- integrate Agent capabilities as part of the pet's behavior model
- expose atomic interfaces for **form modeling** and **motion/action modeling**
- support irregular or non-rectangular interaction patterns
- support UI management surfaces and back-office tooling
- support peripheral/device communication as part of the broader runtime story

This vision should influence architectural judgment even when near-term work stays small. In particular, future decisions should preserve room for:

- non-window-first hosts
- richer present/render paths beyond minimal desktop black-box rendering
- runtime-level behavior orchestration
- coexistence of foreground embodied interaction, management UI, and external device integration
- collision volume semantics and layer semantics early enough to avoid later architectural repainting

When tradeoffs appear, prefer introducing foundational concepts like collision volumes and layers slightly early over discovering too late that the architecture cannot express them without large-scale rework. The main risk to optimize against is not local complexity growth, but architecture defects that force broad redesign.

This vision is expected to evolve. The user may append or refine it over time, and those additions should be treated as live architectural guidance rather than marketing copy.

## Execution rules

- After long-range concept modeling, land the concept in visible UI behavior soon enough to validate it. Do not let architecture work drift into closed-door abstraction growth without a user-visible proving point.
- New capabilities should be introduced with inspect/debug affordances in mind. As a default, provide a debuggable path for state inspection and event triggering; special exceptions can be discussed case by case.
- Prefer an optional IPC-based, namespace-isolated event interface for debug/control surfaces so that capabilities are observable and triggerable from outside the immediate UI loop.
- Before considering a capability delivery complete, run a lowest-cost smoke gate yourself: build/check, minimal launch path, key interaction/event path, and the new UI/debug landing. Heavy end-to-end gates are optional and will be added by the user as needed.

## `stui-dev` operations handbook

- Treat `stui-dev` as the main command-boundary development control surface.
- Keep `stui-dev` supervisor-oriented and out of direct playground/lib internals.

### Command usage policy

- Development and debugging flows should use `cargo run -p ...` directly.
- Formal tool usage should prefer `cargo install --path ...` once, followed by the installed command directly.
- Concretely:
  - development/debug: `cargo run -p stui-dev -- ...`
  - installed/operational use: `cargo install --path scripts/stui-dev`, then `stui-dev ...`

### Operational expectations

- Do not treat ad hoc `target/debug/*.exe` invocation as the primary workflow; use it only as a temporary troubleshooting path when necessary.
- When discussing or validating behavior, be explicit about whether the path under test is the dev path (`cargo run -p`) or the installed path (`cargo install` + command).
- `stui-dev` should resolve playground binaries in a way that respects that split: dev usage may target repo `target/debug`, while installed usage should prefer installed/explicitly configured binaries over a hardcoded repo-local path.
- Keep smoke gates cheap but real: build/check, minimal launch, key control/inspect path, and visible/debug landing.
