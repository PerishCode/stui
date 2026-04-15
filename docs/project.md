# stui project manifesto

## One-line definition

`stui` is a learning-oriented cross-platform UI/runtime experiment whose main goal is to validate architecture, not to preserve compatibility.

## Core stance

- Follow the best practices we believe in now.
- Inherit as little historical baggage as possible.
- Iterate aggressively, even when that means breaking earlier designs.
- Keep going until the project becomes truly useful, or the experiment is no longer fun.

## Priority order

1. **Architecture validation**
   - Decide which abstractions deserve to exist.
   - Find the right boundary between UI model and runtime.
   - Prefer uniformity, explainability, and fewer special cases.
2. **Performance exploration**
   - Avoid designs that make performance impossible later.
   - Add observability early enough to detect structural mistakes.
   - Optimize after the architecture starts proving itself.
3. **Usable prototype**
   - Build only enough surface area to test architectural decisions.
   - Treat usability as evidence-gathering, not product commitment.

## Design principles

### 1. Breakability is a feature

The project may rewrite or delete major pieces whenever a simpler or more coherent model appears.

### 2. Compatibility is optional

The project does not exist to preserve legacy APIs, migration paths, or ecosystem expectations.

### 3. Small scope, high signal

Each implementation step should teach something about rendering, layout, events, state, scheduling, or cross-platform runtime boundaries.

### 4. Prototype for truth

A prototype is successful when it reveals whether an idea is good, bad, or incomplete.

### 5. Performance matters structurally

Slow code is acceptable during exploration. Architectures that cannot become fast are not.

### 6. Concepts must reach UI

Long-range concepts should not remain purely abstract for long. After a concept is modeled, it should earn a visible UI or interaction landing soon enough to prove that the architecture is learning from reality rather than only rearranging vocabulary.

### 7. Capabilities should be inspectable

As a default rule, new capabilities should come with a believable path for inspection and triggering:

- state inspection
- event observation
- event triggering/injection
- optional IPC-based, namespace-isolated control surfaces when appropriate

Special cases can be discussed, but the baseline expectation is that capabilities are debuggable rather than opaque.

### 8. Smoke gating before delivery

Before considering a capability delivery complete, run the cheapest useful smoke gate:

- build/check succeeds
- the minimal launch path still runs
- the primary new interaction or event path can be exercised
- the new UI/debug landing behaves plausibly

Heavy end-to-end gates are intentionally out of scope for now and can be added later when the user decides they are worth the cost.

## Long-term proving ground

One long-term product-shaped proving ground for `stui` is an open **3D Agent desktop pet**.

This should be understood as an architectural pressure source, not as a promise to immediately build the end product. The value of this vision is that it forces the architecture to make room for a more demanding shape than a conventional desktop app.

The intended product qualities include:

- a foreground 3D pet that acts as an embodied interactive entity
- integrated Agent behavior rather than a purely scripted shell
- atomic interfaces for form modeling
- atomic interfaces for motion/action modeling
- irregular or non-rectangular interaction patterns
- management UI and operational tooling surfaces
- external device/peripheral communication as part of the system boundary

This long-term target matters because it pressures `stui` to preserve room for:

- host-first rather than window-first architecture
- multiple surface types and presentation paths
- runtime-level behavior orchestration
- coexistence of foreground interaction, backstage UI, and external integrations
- richer render models than a single rectangular software-buffer path
- early room for collision-volume semantics and layer semantics

An important practical implication is that `stui` should not over-optimize for short-term simplicity if that simplicity blocks concepts like collision volumes or layers until too late. Moderate local complexity is acceptable when it prevents much larger architectural rework later.

## Non-goals

- Shipping a stable framework quickly
- Providing production support
- Matching existing UI frameworks feature-for-feature
- Designing for every platform and every use case up front
- Keeping abstractions alive just because they already exist

## Practical meaning for early work

In the near term, `stui` should prefer:

- tiny experiments over broad systems
- explicit tradeoffs over vague extensibility
- disposable implementations over premature foundations
- architecture notes backed by code over architecture notes alone

## Workspace structure

The repository is organized around three roles:

- `lib/crates/*`
  - core modeling
  - runtime and host abstractions
  - reusable implementation crates
- `playgrounds/*`
  - narrow Rust experiments
  - temporary proving grounds for rendering, lifecycle, and platform assumptions
- `scripts/*`
  - Rust-based development and operational tooling
  - launchers, packers, and other non-core executables

This means script crates are not the architectural center. Core modeling belongs in `lib/crates`, while `playgrounds` exist to pressure-test ideas without turning every experiment into framework surface area.

## Current architectural constraints

### `stui-core`

- modeling only
- ports only
- no concrete runtime event/component catalog

For now that means core owns concepts like `Host`, `Surface`, `Event`, and `Component`, along with their data shape and boundary traits.

### `stui-runtime`

- owns concrete runtime declarations
- decides which events and components exist in a given experiment
- turns core slots into a runnable statement of intent

### `scripts/*`

- use `AppState` for global singleton management
- initialize `Config` separately and feed it downward
- use `clap` directly instead of inventing custom CLI machinery
- default toward `cargo install` then direct use; reserve `cargo run -p ...` for local debugging

## Initial crate map

- `lib/crates/stui-core`
  - host/surface/component/event modeling
  - boundary traits for host/present integration
- `lib/crates/stui-runtime`
  - black-box milestone declarations
  - concrete component/event set for the current experiment
- `lib/crates/stui-platform-desktop`
  - desktop-oriented host planning layer
  - temporary place for a window-backed host adapter without polluting core vocabulary
- `playgrounds/black-box`
  - narrow experiment for the minimal black undecorated host surface
- `scripts/stui-dev`
  - developer-facing launcher built around `AppState` + `Config`
- `scripts/stui-pack`
  - reserved packaging script crate

## Current milestone

The current near-term target is to validate the smallest useful rendering/host loop:

- `stui-dev` can launch the experiment
- a small black undecorated host surface appears
- the implementation does not make “window” the default core abstraction
- the process exits cleanly and intentionally when the host requests shutdown

This milestone is about bootstrap shape and lifecycle correctness, not about building a full UI system.

The first runnable path is now shaped as:

- `scripts/stui-dev` launches the experiment
- `playgrounds/black-box` names the narrow experiment
- `lib/crates/stui-platform-desktop` owns the desktop event loop and present path
- `winit + softbuffer` stay contained in the desktop adapter layer

The next architectural refinement now in place is:

- desktop events are translated into runtime events
- `stui-runtime` decides redraw, visibility, and shutdown commands for the black-box experiment
- `stui-platform-desktop` executes runtime commands instead of owning lifecycle policy directly

The latest refinement is:

- close is now modeled as an explicit request/decision protocol
- host and surface snapshots are synchronized into runtime state
- the desktop adapter now feeds runtime with state snapshots as well as translated events

The current render refinement is:

- `stui-core` now includes a minimal render/present model
- runtime issues a generic present command carrying a present model
- desktop code translates that present model into the actual softbuffer write path

The latest present-port refinement is:

- `PresentPort` now defines both present-state inspection and present execution
- desktop presentation is isolated in a dedicated presenter implementation
- the runtime loop talks in terms of present commands while the presenter owns host-surface writes

The current render-intent refinement is:

- the present model now supports both `ClearSolid` and `ClearInset`
- this keeps the abstraction honest by forcing it to represent more than one trivial path
- the black-box experiment now exercises a simple inset-composition case while staying visually close to the original small black box

The latest host/surface refinement is:

- `HostPort` now includes capability inspection
- `PresentPort` now includes capability inspection alongside present execution
- desktop hosting and desktop presenting now live in separate concrete objects

This is still intentionally small, but it makes the host/surface side of the architecture feel more like real ports and less like thin naming over one desktop runner.

The latest runtime/component refinement is:

- black-box rendering is now derived from a tiny component tree rather than a single hard-coded present constant
- runtime owns a root component state and a fill component state
- the root component resolves a frame margin from current surface extent, which means present output is now shaped by runtime component semantics

This is still far from a full component system, but it is the first meaningful step away from “runtime as a thin event-to-present mapper.”

The latest component-tree refinement is:

- component descriptors can now encode a parent-child relation
- the black-box experiment explicitly models `fill` as a child of `root`
- this adds structural truth to the component layer without forcing the project into reconciliation, diffing, or layout work prematurely

The latest component-resolution refinement is:

- runtime now resolves the tiny component tree into a separate resolved tree shape
- present output is produced from resolved component meaning rather than directly from raw component state
- this creates an explicit semantic step between component structure and presentation without introducing a full renderer pipeline or reconciler

The latest behavior refinement is:

- runtime now carries a minimal behavior phase for the black-box experiment
- `HostResumed` and close requests drive behavior transitions
- component resolution now depends on behavior phase before present output is derived

This is still intentionally tiny, but it is the first step where runtime semantics begin to express “how the thing is behaving” rather than only “what gets drawn.”

The latest semantic-propagation refinement is:

- root resolution now produces a tiny inherited semantic for the fill child
- the fill child resolves its final color from both local state and inherited semantic
- this makes the parent-child relation do real semantic work instead of being only descriptive structure

The latest layer refinement is:

- `PresentModel` now includes a minimal `LayerRole`
- inherited component semantics now carry a layer decision into child resolution
- present output is now layer-tagged even though the current desktop presenter still renders a single-path result

This is intentionally early and intentionally small. The goal is not immediate compositing complexity, but ensuring layer semantics have a believable place in the architecture before later product demands make them expensive to retrofit.

## Ongoing guidance from the long-term vision

As the long-term 3D Agent pet vision evolves, new descriptive details should be treated as active architectural guidance.

Near-term tasks do not need to implement that product directly, but they should be evaluated against questions like:

- Does this make non-rectangular or non-standard hosts harder later?
- Does this keep room for richer behavior/runtime orchestration?
- Does this leave space for multiple simultaneous surfaces, including management UI?
- Does this preserve a path toward device/peripheral integration?
- Does this leave a believable path for collision volumes and layer-aware semantics?
- Does this concept have a believable UI landing and inspect/debug path, or is it still too closed over?

The goal is not to overbuild early. The goal is to let the future product shape act as a durable source of architectural taste and constraint.
