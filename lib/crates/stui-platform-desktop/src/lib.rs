mod host;
mod presenter;
mod runner;

use anyhow::Result;
use stui_core::{
    CloseBehavior, Extent, HostDescriptor, HostState, SurfaceDescriptor, SurfaceState,
};
use stui_runtime::{BehaviorPhase, RuntimeDeclaration};

pub use runner::{run_black_box, run_black_box_with_behavior, DesktopRunReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopHostConfig {
    pub initial_extent: Extent,
    pub decorated: bool,
}

impl DesktopHostConfig {
    pub const fn black_box() -> Self {
        Self {
            initial_extent: Extent::new(320, 240),
            decorated: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSessionPlan {
    pub host: HostDescriptor,
    pub surface: SurfaceDescriptor,
    pub host_state: HostState,
    pub surface_state: SurfaceState,
}

impl DesktopSessionPlan {
    pub fn from_runtime(runtime: &RuntimeDeclaration, config: DesktopHostConfig) -> Self {
        let host = HostDescriptor::new(
            runtime.host.id.0,
            runtime.host.role,
            CloseBehavior::ManagedByRuntime,
            runtime.host.primary_surface,
        );
        let surface = SurfaceDescriptor::new(
            runtime.surface.id.0,
            runtime.surface.role,
            config.initial_extent,
            config.decorated,
        );

        Self {
            host,
            surface,
            host_state: HostState::default(),
            surface_state: SurfaceState::new(config.initial_extent, false),
        }
    }
}

pub fn plan_black_box_session() -> DesktopSessionPlan {
    DesktopSessionPlan::from_runtime(
        &RuntimeDeclaration::black_box(),
        DesktopHostConfig::black_box(),
    )
}

pub fn run_planned_black_box() -> Result<DesktopRunReport> {
    run_black_box(plan_black_box_session())
}

pub fn run_planned_black_box_with_behavior(behavior: BehaviorPhase) -> Result<DesktopRunReport> {
    run_black_box_with_behavior(plan_black_box_session(), Some(behavior))
}
