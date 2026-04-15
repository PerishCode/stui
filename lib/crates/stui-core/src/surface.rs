use crate::render::PresentModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub &'static str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}

impl Extent {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDescriptor {
    pub id: SurfaceId,
    pub role: &'static str,
    pub initial_extent: Extent,
    pub decorated: bool,
}

impl SurfaceDescriptor {
    pub const fn new(
        id: &'static str,
        role: &'static str,
        initial_extent: Extent,
        decorated: bool,
    ) -> Self {
        Self {
            id: SurfaceId(id),
            role,
            initial_extent,
            decorated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceState {
    pub extent: Extent,
    pub visible: bool,
}

impl SurfaceState {
    pub const fn new(extent: Extent, visible: bool) -> Self {
        Self { extent, visible }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentCapabilities {
    pub supports_transparency: bool,
    pub supports_irregular_shape: bool,
    pub supports_clear_solid: bool,
    pub supports_clear_inset: bool,
}

impl PresentCapabilities {
    pub const fn new(
        supports_transparency: bool,
        supports_irregular_shape: bool,
        supports_clear_solid: bool,
        supports_clear_inset: bool,
    ) -> Self {
        Self {
            supports_transparency,
            supports_irregular_shape,
            supports_clear_solid,
            supports_clear_inset,
        }
    }
}

pub trait PresentPort {
    type Error;

    fn descriptor(&self) -> &SurfaceDescriptor;
    fn state(&self) -> SurfaceState;
    fn capabilities(&self) -> PresentCapabilities;
    fn present(&mut self, model: PresentModel) -> Result<(), Self::Error>;
}
