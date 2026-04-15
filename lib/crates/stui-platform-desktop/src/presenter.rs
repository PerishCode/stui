use std::{num::NonZeroU32, rc::Rc};

use softbuffer::{SoftBufferError, Surface};
use stui_core::{
    PresentCapabilities, PresentModel, PresentPort, RenderIntent, SurfaceDescriptor, SurfaceState,
};
use winit::{event_loop::OwnedDisplayHandle, window::Window};

pub struct DesktopPresenter {
    descriptor: SurfaceDescriptor,
    state: SurfaceState,
    window: Rc<Window>,
    surface: Surface<OwnedDisplayHandle, Rc<Window>>,
}

impl DesktopPresenter {
    pub fn new(
        descriptor: SurfaceDescriptor,
        state: SurfaceState,
        window: Rc<Window>,
        surface: Surface<OwnedDisplayHandle, Rc<Window>>,
    ) -> Self {
        Self {
            descriptor,
            state,
            window,
            surface,
        }
    }

    pub fn set_visibility(&mut self, visible: bool) {
        self.state.visible = visible;
    }

    pub fn sync_extent_from_window(&mut self) {
        let size = self.window.inner_size();
        self.state.extent = stui_core::Extent::new(size.width, size.height);
    }

    pub fn set_extent(&mut self, extent: stui_core::Extent) {
        self.state.extent = extent;
    }
}

impl PresentPort for DesktopPresenter {
    type Error = SoftBufferError;

    fn descriptor(&self) -> &SurfaceDescriptor {
        &self.descriptor
    }

    fn state(&self) -> SurfaceState {
        self.state
    }

    fn capabilities(&self) -> PresentCapabilities {
        PresentCapabilities::new(false, false, true, true)
    }

    fn present(&mut self, model: PresentModel) -> Result<(), Self::Error> {
        self.sync_extent_from_window();

        if self.state.extent.width == 0 || self.state.extent.height == 0 {
            return Ok(());
        }

        let width = NonZeroU32::new(self.state.extent.width).expect("non-zero width checked");
        let height = NonZeroU32::new(self.state.extent.height).expect("non-zero height checked");

        self.surface.resize(width, height)?;

        let mut buffer = self.surface.buffer_mut()?;
        match model.intent {
            RenderIntent::ClearSolid { color } => {
                buffer.fill(color.into_u32());
            }
            RenderIntent::ClearInset {
                background,
                inset,
                margin,
            } => {
                let width = self.state.extent.width as usize;
                let height = self.state.extent.height as usize;
                let background = background.into_u32();
                let inset = inset.into_u32();

                buffer.fill(background);

                if margin.saturating_mul(2) < self.state.extent.width
                    && margin.saturating_mul(2) < self.state.extent.height
                {
                    let margin = margin as usize;
                    for y in margin..(height - margin) {
                        let row_start = y * width;
                        for x in margin..(width - margin) {
                            buffer[row_start + x] = inset;
                        }
                    }
                }
            }
        }
        buffer.present()?;

        Ok(())
    }
}
