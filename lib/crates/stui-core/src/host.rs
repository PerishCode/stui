use crate::surface::SurfaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostId(pub &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseBehavior {
    ManagedByRuntime,
    ManagedByHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRequestSource {
    Host,
    DevelopmentShortcut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseRequest {
    pub source: CloseRequestSource,
}

impl CloseRequest {
    pub const fn new(source: CloseRequestSource) -> Self {
        Self { source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDescriptor {
    pub id: HostId,
    pub role: &'static str,
    pub close_behavior: CloseBehavior,
    pub primary_surface: SurfaceId,
}

impl HostDescriptor {
    pub const fn new(
        id: &'static str,
        role: &'static str,
        close_behavior: CloseBehavior,
        primary_surface: SurfaceId,
    ) -> Self {
        Self {
            id: HostId(id),
            role,
            close_behavior,
            primary_surface,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostState {
    pub visible: bool,
    pub close_requested: bool,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            visible: false,
            close_requested: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCapabilities {
    pub supports_repositioning: bool,
    pub supports_transparency: bool,
    pub supports_irregular_interaction: bool,
}

impl HostCapabilities {
    pub const fn new(
        supports_repositioning: bool,
        supports_transparency: bool,
        supports_irregular_interaction: bool,
    ) -> Self {
        Self {
            supports_repositioning,
            supports_transparency,
            supports_irregular_interaction,
        }
    }
}

pub trait HostPort {
    fn descriptor(&self) -> &HostDescriptor;
    fn state(&self) -> HostState;
    fn capabilities(&self) -> HostCapabilities;
}
