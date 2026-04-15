use anyhow::{Context as _, Result};
use softbuffer::Context;
use stui_core::{CloseRequest, CloseRequestSource, HostPort, PresentPort, SurfaceState};
use stui_runtime::{
    BehaviorPhase, BlackBoxRuntime, CloseDecision, RuntimeCommand, RuntimeEvent, RuntimeExitReason,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{Key, NamedKey},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    host::DesktopHost, presenter::DesktopPresenter, DesktopHostConfig, DesktopSessionPlan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopRunReport {
    pub exit_reason: RuntimeExitReason,
    pub presented_at_least_once: bool,
}

pub fn run_black_box(plan: DesktopSessionPlan) -> Result<DesktopRunReport> {
    run_black_box_with_behavior(plan, None)
}

pub fn run_black_box_with_behavior(
    plan: DesktopSessionPlan,
    forced_behavior: Option<BehaviorPhase>,
) -> Result<DesktopRunReport> {
    let event_loop = EventLoop::new().context("create desktop event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| anyhow::anyhow!("create softbuffer display context: {error}"))?;

    let mut app = DesktopBlackBoxApp::new(plan, forced_behavior, context);
    event_loop
        .run_app(&mut app)
        .context("run desktop black-box app")?;

    app.report
        .context("desktop black-box app exited without an exit reason")
}

struct DesktopBlackBoxApp {
    plan: DesktopSessionPlan,
    forced_behavior: Option<BehaviorPhase>,
    runtime: BlackBoxRuntime,
    context: Context<OwnedDisplayHandle>,
    host: Option<DesktopHost>,
    presenter: Option<DesktopPresenter>,
    report: Option<DesktopRunReport>,
}

impl DesktopBlackBoxApp {
    fn new(
        plan: DesktopSessionPlan,
        forced_behavior: Option<BehaviorPhase>,
        context: Context<OwnedDisplayHandle>,
    ) -> Self {
        Self {
            plan,
            forced_behavior,
            runtime: BlackBoxRuntime::new(),
            context,
            host: None,
            presenter: None,
            report: None,
        }
    }

    fn config(&self) -> DesktopHostConfig {
        DesktopHostConfig {
            initial_extent: self.plan.surface.initial_extent,
            decorated: self.plan.surface.decorated,
        }
    }

    fn build_window_attributes(&self) -> WindowAttributes {
        let config = self.config();

        Window::default_attributes()
            .with_title("stui black-box")
            .with_decorations(config.decorated)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(
                config.initial_extent.width,
                config.initial_extent.height,
            ))
    }

    fn current_surface_state(&self) -> SurfaceState {
        self.presenter
            .as_ref()
            .map(PresentPort::state)
            .unwrap_or(self.plan.surface_state)
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop, exit_reason: RuntimeExitReason) {
        self.report = Some(DesktopRunReport {
            exit_reason,
            presented_at_least_once: self.runtime.state().presented_at_least_once,
        });
        event_loop.exit();
    }

    fn sync_snapshots(&mut self, event_loop: &ActiveEventLoop) {
        let host = self
            .host
            .as_ref()
            .map(HostPort::state)
            .unwrap_or(self.plan.host_state);
        let surface = self.current_surface_state();

        self.apply_runtime_event(
            event_loop,
            RuntimeEvent::SnapshotsSynchronized { host, surface },
        );
    }

    fn request_redraw(&self) {
        if let Some(host) = self.host.as_ref() {
            host.window().request_redraw();
        }
    }

    fn log_runtime_snapshot(&self, stage: &str) {
        println!("debug stage={stage} {}", self.runtime.debug_summary());
    }

    fn should_log_event(event: &RuntimeEvent) -> bool {
        !matches!(
            event,
            RuntimeEvent::SurfaceInvalidated | RuntimeEvent::SnapshotsSynchronized { .. }
        )
    }

    fn log_debug_capabilities(&self) {
        if let (Some(host), Some(presenter)) = (self.host.as_ref(), self.presenter.as_ref()) {
            println!(
                "debug host_capabilities={:?} present_capabilities={:?}",
                host.capabilities(),
                presenter.capabilities()
            );
        }
    }

    fn set_behavior_phase(&mut self, event_loop: &ActiveEventLoop, behavior: BehaviorPhase) {
        println!("debug control=set-behavior phase={behavior:?}");
        self.sync_snapshots(event_loop);
        self.apply_runtime_event(event_loop, RuntimeEvent::DevelopmentSetBehavior(behavior));
    }

    fn apply_runtime_event(&mut self, event_loop: &ActiveEventLoop, event: RuntimeEvent) {
        let should_log = Self::should_log_event(&event);
        let commands = self.runtime.handle_event(event);

        if should_log {
            self.log_runtime_snapshot("after-event");
        }

        for command in commands {
            match command {
                RuntimeCommand::RequestRedraw => {
                    self.request_redraw();
                }
                RuntimeCommand::Present(model) => {
                    if let Some(presenter) = self.presenter.as_mut() {
                        presenter
                            .present(model)
                            .expect("present via desktop presenter");
                    }
                    self.sync_snapshots(event_loop);
                }
                RuntimeCommand::SetHostVisible(visible) => {
                    if let Some(host) = self.host.as_mut() {
                        host.window().set_visible(visible);
                        host.set_visible(visible);
                    }
                    if let Some(presenter) = self.presenter.as_mut() {
                        presenter.set_visibility(visible);
                    }

                    self.sync_snapshots(event_loop);
                }
                RuntimeCommand::CompleteClose(decision) => match decision {
                    CloseDecision::Accept(reason) => {
                        self.exit(event_loop, reason);
                    }
                    CloseDecision::Ignore => {
                        if let Some(host) = self.host.as_mut() {
                            host.set_close_requested(false);
                        }
                        self.sync_snapshots(event_loop);
                    }
                },
            }
        }
    }
}

impl ApplicationHandler for DesktopBlackBoxApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = std::rc::Rc::new(
            event_loop
                .create_window(self.build_window_attributes())
                .expect("create black-box host window"),
        );

        let surface = softbuffer::Surface::new(&self.context, window.clone())
            .expect("create softbuffer surface for black-box host");

        let host = DesktopHost::new(self.plan.host.clone(), self.plan.host_state, window.clone());
        let presenter = DesktopPresenter::new(
            self.plan.surface.clone(),
            self.plan.surface_state,
            window,
            surface,
        );

        self.host = Some(host);
        self.presenter = Some(presenter);
        self.log_debug_capabilities();
        self.sync_snapshots(event_loop);
        self.log_runtime_snapshot("after-resume-sync");
        self.apply_runtime_event(event_loop, RuntimeEvent::HostResumed);

        if let Some(behavior) = self.forced_behavior {
            self.set_behavior_phase(event_loop, behavior);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(host) = self.host.as_ref() else {
            return;
        };

        if host.window().id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(host) = self.host.as_mut() {
                    host.set_close_requested(true);
                }
                self.sync_snapshots(event_loop);
                self.apply_runtime_event(
                    event_loop,
                    RuntimeEvent::CloseRequested(CloseRequest::new(CloseRequestSource::Host)),
                );
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(event.logical_key, Key::Named(NamedKey::Escape)) =>
            {
                if let Some(host) = self.host.as_mut() {
                    host.set_close_requested(true);
                }
                self.sync_snapshots(event_loop);
                self.apply_runtime_event(
                    event_loop,
                    RuntimeEvent::CloseRequested(CloseRequest::new(
                        CloseRequestSource::DevelopmentShortcut,
                    )),
                );
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(&event.logical_key, Key::Character(ch) if ch == "1") =>
            {
                self.set_behavior_phase(event_loop, BehaviorPhase::Booting);
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(&event.logical_key, Key::Character(ch) if ch == "2") =>
            {
                self.set_behavior_phase(event_loop, BehaviorPhase::Idle);
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(&event.logical_key, Key::Character(ch) if ch == "3") =>
            {
                self.set_behavior_phase(event_loop, BehaviorPhase::Closing);
            }
            WindowEvent::Resized(size) => {
                if let Some(presenter) = self.presenter.as_mut() {
                    presenter.set_extent(stui_core::Extent::new(size.width, size.height));
                }
                self.sync_snapshots(event_loop);
                self.apply_runtime_event(event_loop, RuntimeEvent::SurfaceInvalidated);
            }
            WindowEvent::RedrawRequested => {
                self.sync_snapshots(event_loop);
                self.apply_runtime_event(event_loop, RuntimeEvent::SurfaceInvalidated);
            }
            _ => {}
        }
    }
}
