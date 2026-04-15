use std::rc::Rc;

use stui_core::{HostCapabilities, HostDescriptor, HostPort, HostState};
use winit::window::Window;

pub struct DesktopHost {
    descriptor: HostDescriptor,
    state: HostState,
    capabilities: HostCapabilities,
    window: Rc<Window>,
}

impl DesktopHost {
    pub fn new(descriptor: HostDescriptor, state: HostState, window: Rc<Window>) -> Self {
        Self {
            descriptor,
            state,
            capabilities: HostCapabilities::new(false, false, false),
            window,
        }
    }

    pub fn window(&self) -> &Rc<Window> {
        &self.window
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.state.visible = visible;
    }

    pub fn set_close_requested(&mut self, close_requested: bool) {
        self.state.close_requested = close_requested;
    }
}

impl HostPort for DesktopHost {
    fn descriptor(&self) -> &HostDescriptor {
        &self.descriptor
    }

    fn state(&self) -> HostState {
        self.state
    }

    fn capabilities(&self) -> HostCapabilities {
        self.capabilities
    }
}
