# AGENTS

This repository is being grown as a learning-oriented cross-platform UI/runtime experiment.

Current scope is intentionally minimal.

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
