use stui_core::{
    CloseBehavior, CloseRequest, CloseRequestSource, ComponentDescriptor, EventDescriptor, Extent,
    HostDescriptor, HostState, LayerRole, PresentModel, Rgb24, SurfaceDescriptor, SurfaceState,
};

pub const BLACK_BOX_HOST: HostDescriptor = HostDescriptor::new(
    "black-box.host",
    "desktop-host",
    CloseBehavior::ManagedByRuntime,
    stui_core::SurfaceId("black-box.surface"),
);

pub const BLACK_BOX_SURFACE: SurfaceDescriptor = SurfaceDescriptor::new(
    "black-box.surface",
    "present-target",
    Extent::new(320, 240),
    false,
);

pub const BLACK_BOX_COMPONENTS: [ComponentDescriptor; 2] = [
    ComponentDescriptor::root("black-box.root", "root-component"),
    ComponentDescriptor::child("black-box.fill", "solid-fill-component", "black-box.root"),
];

pub const BLACK_BOX_EVENTS: [EventDescriptor; 6] = [
    EventDescriptor::new(
        "host.resumed",
        "host became active and is ready for runtime control",
    ),
    EventDescriptor::new("host.close-requested", "host requested graceful shutdown"),
    EventDescriptor::new(
        "input.escape-pressed",
        "development escape hatch for shutdown",
    ),
    EventDescriptor::new(
        "surface.invalidated",
        "surface requested a redraw for the current frame",
    ),
    EventDescriptor::new(
        "development.set-behavior",
        "development control forced a runtime behavior phase",
    ),
    EventDescriptor::new(
        "runtime.snapshots-synchronized",
        "host and surface snapshots were synchronized into runtime state",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorPhase {
    Booting,
    Idle,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeComponent {
    Root,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEvent {
    HostResumed,
    CloseRequested(CloseRequest),
    DevelopmentSetBehavior(BehaviorPhase),
    SurfaceInvalidated,
    SnapshotsSynchronized {
        host: HostState,
        surface: SurfaceState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeExitReason {
    HostRequestedClose,
    EscapePressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Accept(RuntimeExitReason),
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommand {
    RequestRedraw,
    Present(PresentModel),
    SetHostVisible(bool),
    CompleteClose(CloseDecision),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootComponentState {
    pub base_margin: u32,
}

impl RootComponentState {
    pub const fn black_box() -> Self {
        Self { base_margin: 4 }
    }

    pub fn resolved_margin(&self, extent: Extent) -> u32 {
        let adaptive = extent.width.min(extent.height) / 24;
        adaptive.max(self.base_margin)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillComponentState {
    pub base_color: Rgb24,
}

impl FillComponentState {
    pub const fn black_box() -> Self {
        Self {
            base_color: Rgb24::new(0, 0, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlackBoxComponentTree {
    pub root: RootComponentState,
    pub fill: FillComponentState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFillComponent {
    pub color: Rgb24,
    pub layer: LayerRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedFillSemantics {
    pub tint: Rgb24,
    pub layer: LayerRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRootComponent {
    pub frame_color: Rgb24,
    pub margin: u32,
    pub fill: ResolvedFillComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedBlackBoxTree {
    pub root: ResolvedRootComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlackBoxDebugSnapshot {
    pub behavior: BehaviorPhase,
    pub host: HostState,
    pub surface: SurfaceState,
    pub root: ResolvedRootComponent,
    pub present: PresentModel,
    pub presented_at_least_once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlackBoxDebugDocument<'a> {
    pub runtime: &'a str,
    pub host_id: &'a str,
    pub surface_id: &'a str,
    pub forced_behavior: Option<BehaviorPhase>,
    pub snapshot: BlackBoxDebugSnapshot,
}

impl BehaviorPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "booting",
            Self::Idle => "idle",
            Self::Closing => "closing",
        }
    }
}

fn layer_role_as_str(layer: LayerRole) -> &'static str {
    match layer {
        LayerRole::Background => "background",
        LayerRole::Foreground => "foreground",
        LayerRole::Overlay => "overlay",
    }
}

fn rgb24_hex_string(color: Rgb24) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

impl BlackBoxDebugSnapshot {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"behavior\":\"{}\"",
                ",\"host\":{{\"visible\":{},\"close_requested\":{}}}",
                ",\"surface\":{{\"extent\":{{\"width\":{},\"height\":{}}},\"visible\":{}}}",
                ",\"root\":{{",
                "\"frame_color\":{{\"red\":{},\"green\":{},\"blue\":{},\"hex\":\"{}\"}}",
                ",\"margin\":{}",
                ",\"fill\":{{",
                "\"color\":{{\"red\":{},\"green\":{},\"blue\":{},\"hex\":\"{}\"}}",
                ",\"layer\":\"{}\"",
                "}}",
                "}}",
                ",\"present\":{{",
                "\"layer\":\"{}\"",
                ",\"intent\":{{",
                "\"kind\":\"clear_inset\"",
                ",\"background\":{{\"red\":{},\"green\":{},\"blue\":{},\"hex\":\"{}\"}}",
                ",\"inset\":{{\"red\":{},\"green\":{},\"blue\":{},\"hex\":\"{}\"}}",
                ",\"margin\":{}",
                "}}",
                "}}",
                ",\"presented_at_least_once\":{}",
                "}}"
            ),
            self.behavior.as_str(),
            self.host.visible,
            self.host.close_requested,
            self.surface.extent.width,
            self.surface.extent.height,
            self.surface.visible,
            self.root.frame_color.red,
            self.root.frame_color.green,
            self.root.frame_color.blue,
            rgb24_hex_string(self.root.frame_color),
            self.root.margin,
            self.root.fill.color.red,
            self.root.fill.color.green,
            self.root.fill.color.blue,
            rgb24_hex_string(self.root.fill.color),
            layer_role_as_str(self.root.fill.layer),
            layer_role_as_str(self.present.layer),
            self.root.frame_color.red,
            self.root.frame_color.green,
            self.root.frame_color.blue,
            rgb24_hex_string(self.root.frame_color),
            self.root.fill.color.red,
            self.root.fill.color.green,
            self.root.fill.color.blue,
            rgb24_hex_string(self.root.fill.color),
            self.root.margin,
            self.presented_at_least_once,
        )
    }
}

impl<'a> BlackBoxDebugDocument<'a> {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"runtime\":\"{}\"",
                ",\"host_id\":\"{}\"",
                ",\"surface_id\":\"{}\"",
                ",\"forced_behavior\":{}",
                ",\"snapshot\":{}",
                "}}"
            ),
            self.runtime,
            self.host_id,
            self.surface_id,
            self.forced_behavior
                .map(|behavior| format!("\"{}\"", behavior.as_str()))
                .unwrap_or_else(|| "null".to_string()),
            self.snapshot.to_json(),
        )
    }
}

impl BlackBoxComponentTree {
    pub const fn black_box() -> Self {
        Self {
            root: RootComponentState::black_box(),
            fill: FillComponentState::black_box(),
        }
    }

    pub fn resolve(&self, extent: Extent, behavior: BehaviorPhase) -> ResolvedBlackBoxTree {
        let (frame_color, inherited_fill, margin_bias) = match behavior {
            BehaviorPhase::Booting => (
                Rgb24::new(20, 20, 20),
                InheritedFillSemantics {
                    tint: Rgb24::new(4, 4, 4),
                    layer: LayerRole::Overlay,
                },
                2,
            ),
            BehaviorPhase::Idle => (
                Rgb24::new(8, 8, 8),
                InheritedFillSemantics {
                    tint: Rgb24::new(0, 0, 0),
                    layer: LayerRole::Foreground,
                },
                0,
            ),
            BehaviorPhase::Closing => (
                Rgb24::new(3, 3, 3),
                InheritedFillSemantics {
                    tint: Rgb24::new(0, 0, 0),
                    layer: LayerRole::Overlay,
                },
                6,
            ),
        };

        ResolvedBlackBoxTree {
            root: ResolvedRootComponent {
                frame_color,
                margin: self
                    .root
                    .resolved_margin(extent)
                    .saturating_add(margin_bias),
                fill: ResolvedFillComponent {
                    color: self.fill.resolve(inherited_fill),
                    layer: inherited_fill.layer,
                },
            },
        }
    }
}

impl FillComponentState {
    pub fn resolve(&self, inherited: InheritedFillSemantics) -> Rgb24 {
        Rgb24::new(
            self.base_color.red.saturating_add(inherited.tint.red),
            self.base_color.green.saturating_add(inherited.tint.green),
            self.base_color.blue.saturating_add(inherited.tint.blue),
        )
    }
}

impl ResolvedBlackBoxTree {
    pub fn present_model(&self) -> PresentModel {
        PresentModel::clear_inset_in_layer(
            self.root.fill.layer,
            self.root.frame_color,
            self.root.fill.color,
            self.root.margin,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlackBoxRuntimeState {
    pub host: HostState,
    pub surface: SurfaceState,
    pub components: BlackBoxComponentTree,
    pub behavior: BehaviorPhase,
    pub presented_at_least_once: bool,
}

impl BlackBoxRuntimeState {
    pub const fn new(initial_extent: Extent) -> Self {
        Self {
            host: HostState {
                visible: false,
                close_requested: false,
            },
            surface: SurfaceState {
                extent: initial_extent,
                visible: false,
            },
            components: BlackBoxComponentTree::black_box(),
            behavior: BehaviorPhase::Booting,
            presented_at_least_once: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDeclaration {
    pub host: HostDescriptor,
    pub surface: SurfaceDescriptor,
    pub components: &'static [ComponentDescriptor],
    pub events: &'static [EventDescriptor],
}

impl RuntimeDeclaration {
    pub const fn black_box() -> Self {
        Self {
            host: BLACK_BOX_HOST,
            surface: BLACK_BOX_SURFACE,
            components: &BLACK_BOX_COMPONENTS,
            events: &BLACK_BOX_EVENTS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlackBoxRuntime {
    declaration: RuntimeDeclaration,
    state: BlackBoxRuntimeState,
}

impl Default for BlackBoxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl BlackBoxRuntime {
    pub fn new() -> Self {
        let declaration = RuntimeDeclaration::black_box();
        let state = BlackBoxRuntimeState::new(declaration.surface.initial_extent);

        Self { declaration, state }
    }

    pub fn declaration(&self) -> &RuntimeDeclaration {
        &self.declaration
    }

    pub fn state(&self) -> BlackBoxRuntimeState {
        self.state
    }

    pub fn debug_snapshot(&self) -> BlackBoxDebugSnapshot {
        let resolved = self
            .state
            .components
            .resolve(self.state.surface.extent, self.state.behavior);
        let present = resolved.present_model();

        BlackBoxDebugSnapshot {
            behavior: self.state.behavior,
            host: self.state.host,
            surface: self.state.surface,
            root: resolved.root,
            present,
            presented_at_least_once: self.state.presented_at_least_once,
        }
    }

    pub fn debug_summary(&self) -> String {
        let snapshot = self.debug_snapshot();
        format!(
            "behavior={:?} host(visible={} close_requested={}) surface(extent={}x{} visible={}) root(frame={:?} margin={}) fill(color={:?} layer={:?}) present={:?} presented_at_least_once={}",
            snapshot.behavior,
            snapshot.host.visible,
            snapshot.host.close_requested,
            snapshot.surface.extent.width,
            snapshot.surface.extent.height,
            snapshot.surface.visible,
            snapshot.root.frame_color,
            snapshot.root.margin,
            snapshot.root.fill.color,
            snapshot.root.fill.layer,
            snapshot.present,
            snapshot.presented_at_least_once,
        )
    }

    pub fn debug_snapshot_json(&self) -> String {
        self.debug_snapshot().to_json()
    }

    pub fn debug_document(
        &self,
        forced_behavior: Option<BehaviorPhase>,
    ) -> BlackBoxDebugDocument<'_> {
        BlackBoxDebugDocument {
            runtime: "black-box",
            host_id: self.declaration.host.id.0,
            surface_id: self.declaration.surface.id.0,
            forced_behavior,
            snapshot: self.debug_snapshot(),
        }
    }

    pub fn handle_event(&mut self, event: RuntimeEvent) -> Vec<RuntimeCommand> {
        match event {
            RuntimeEvent::HostResumed => {
                self.state.behavior = BehaviorPhase::Idle;
                vec![RuntimeCommand::RequestRedraw]
            }
            RuntimeEvent::CloseRequested(request) => {
                self.state.host.close_requested = true;
                self.state.behavior = BehaviorPhase::Closing;
                vec![RuntimeCommand::CompleteClose(self.decide_close(request))]
            }
            RuntimeEvent::DevelopmentSetBehavior(behavior) => {
                self.state.behavior = behavior;
                vec![RuntimeCommand::RequestRedraw]
            }
            RuntimeEvent::SurfaceInvalidated => self.render_commands(),
            RuntimeEvent::SnapshotsSynchronized { host, surface } => {
                self.state.host = host;
                self.state.surface = surface;
                Vec::new()
            }
        }
    }

    fn decide_close(&self, request: CloseRequest) -> CloseDecision {
        match request.source {
            CloseRequestSource::Host => {
                CloseDecision::Accept(RuntimeExitReason::HostRequestedClose)
            }
            CloseRequestSource::DevelopmentShortcut => {
                CloseDecision::Accept(RuntimeExitReason::EscapePressed)
            }
        }
    }

    fn render_commands(&mut self) -> Vec<RuntimeCommand> {
        let resolved = self
            .state
            .components
            .resolve(self.state.surface.extent, self.state.behavior);
        let model = resolved.present_model();
        let mut commands = vec![RuntimeCommand::Present(model)];

        if !self.state.presented_at_least_once {
            self.state.presented_at_least_once = true;
            commands.push(RuntimeCommand::SetHostVisible(true));
        }

        commands
    }
}
