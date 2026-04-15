#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb24 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb24 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub const fn into_u32(self) -> u32 {
        (self.blue as u32) | ((self.green as u32) << 8) | ((self.red as u32) << 16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
    Background,
    Foreground,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderIntent {
    ClearSolid {
        color: Rgb24,
    },
    ClearInset {
        background: Rgb24,
        inset: Rgb24,
        margin: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentModel {
    pub layer: LayerRole,
    pub intent: RenderIntent,
}

impl PresentModel {
    pub const fn clear_solid(color: Rgb24) -> Self {
        Self::clear_solid_in_layer(LayerRole::Foreground, color)
    }

    pub const fn clear_solid_in_layer(layer: LayerRole, color: Rgb24) -> Self {
        Self {
            layer,
            intent: RenderIntent::ClearSolid { color },
        }
    }

    pub const fn clear_inset(background: Rgb24, inset: Rgb24, margin: u32) -> Self {
        Self::clear_inset_in_layer(LayerRole::Foreground, background, inset, margin)
    }

    pub const fn clear_inset_in_layer(
        layer: LayerRole,
        background: Rgb24,
        inset: Rgb24,
        margin: u32,
    ) -> Self {
        Self {
            layer,
            intent: RenderIntent::ClearInset {
                background,
                inset,
                margin,
            },
        }
    }
}
