use anyhow::Result;
use std::io::Write;
use std::{thread, time::Duration};
use stui_ipc::{
    error_response, ok_response, request as ipc_request, ChannelNamespace, EventQueueDropPolicy,
    IpcEventCatalog, IpcPolicy, LocalIpcServer,
};
use stui_platform_desktop::{
    plan_black_box_session, run_planned_black_box, run_planned_black_box_with_behavior,
    DesktopRunReport, DesktopSessionPlan,
};
use stui_runtime::{BehaviorPhase, BlackBoxRuntime, RuntimeDeclaration, RuntimeEvent};

const BLACK_BOX_EVENT_NAMES: &[&str] = &[
    "runtime.inspected",
    "runtime.behavior-changed",
    "runtime.shutdown-requested",
    "ipc.error",
];
#[derive(Debug, Default)]
struct EventQueueState {
    items: Vec<String>,
    dropped_since_last_poll: u64,
    total_dropped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlackBoxDebugBehavior {
    Booting,
    Idle,
    Closing,
}

impl From<BlackBoxDebugBehavior> for BehaviorPhase {
    fn from(value: BlackBoxDebugBehavior) -> Self {
        match value {
            BlackBoxDebugBehavior::Booting => BehaviorPhase::Booting,
            BlackBoxDebugBehavior::Idle => BehaviorPhase::Idle,
            BlackBoxDebugBehavior::Closing => BehaviorPhase::Closing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlackBoxPlaygroundConfig {
    pub forced_behavior: Option<BlackBoxDebugBehavior>,
    pub dump_snapshot: bool,
    pub snapshot_format: DebugSnapshotFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugSnapshotFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlackBoxPlayground {
    pub runtime: RuntimeDeclaration,
    pub plan: DesktopSessionPlan,
    pub config: BlackBoxPlaygroundConfig,
}

impl BlackBoxPlayground {
    pub fn load(config: BlackBoxPlaygroundConfig) -> Self {
        Self {
            runtime: RuntimeDeclaration::black_box(),
            plan: plan_black_box_session(),
            config,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "playground=black-box host={} surface={} extent={}x{} decorated={} forced_behavior={:?} dump_snapshot={}",
            self.plan.host.id.0,
            self.plan.surface.id.0,
            self.plan.surface.initial_extent.width,
            self.plan.surface.initial_extent.height,
            self.plan.surface.decorated,
            self.config.forced_behavior,
            self.config.dump_snapshot,
        )
    }

    pub fn debug_snapshot_output(&self) -> String {
        let mut runtime = BlackBoxRuntime::new();
        let forced_behavior = self.config.forced_behavior.map(BehaviorPhase::from);

        if let Some(behavior) = forced_behavior {
            runtime.handle_event(RuntimeEvent::DevelopmentSetBehavior(behavior));
        }

        match self.config.snapshot_format {
            DebugSnapshotFormat::Text => runtime.debug_summary(),
            DebugSnapshotFormat::Json => runtime.debug_document(forced_behavior).to_json(),
        }
    }

    pub fn run(&self) -> Result<DesktopRunReport> {
        if let Some(behavior) = self.config.forced_behavior {
            run_planned_black_box_with_behavior(behavior.into())
        } else {
            run_planned_black_box()
        }
    }

    pub fn ipc_server_summary(&self, namespace_prefix: Option<&str>, instance: &str) -> String {
        let namespace = namespace_for(namespace_prefix);
        let control_channel = ipc_control_channel(&namespace, instance);
        let events_channel = ipc_events_channel(&namespace, instance);

        format!(
            "ipc_server runtime=black-box instance={} prefix={} control_channel={} events_channel={} events_mode=request-response-poll",
            instance,
            namespace.prefix(),
            control_channel.as_str(),
            events_channel.as_str(),
        )
    }

    pub fn serve_ipc(&self, namespace_prefix: Option<&str>, instance: &str) -> Result<()> {
        let policy = IpcPolicy::default();
        let namespace = namespace_for(namespace_prefix);
        let control_server = LocalIpcServer::bind(ipc_control_channel(&namespace, instance))?;
        let events_server = LocalIpcServer::bind(ipc_events_channel(&namespace, instance))?;
        println!(
            "status=ready playground=black-box instance={} {}",
            instance,
            self.ipc_server_summary(namespace_prefix, instance)
        );
        std::io::stdout().flush()?;
        control_server.set_nonblocking(true)?;
        events_server.set_nonblocking(true)?;
        let mut runtime = BlackBoxRuntime::new();
        let mut events = EventQueueState::default();
        let mut next_event_id: u64 = 1;
        let mut shutdown_pending = false;

        if let Some(behavior) = self.config.forced_behavior.map(BehaviorPhase::from) {
            runtime.handle_event(RuntimeEvent::DevelopmentSetBehavior(behavior));
            push_event(
                &mut events,
                &mut next_event_id,
                "runtime.behavior-changed",
                &runtime.debug_document(Some(behavior)).to_json(),
            );
        }

        loop {
            let mut did_work = false;

            let control_connection = match control_server.try_accept() {
                Ok(connection) => connection,
                Err(_) => None,
            };

            if let Some(mut connection) = control_connection {
                did_work = true;

                let request = match connection.read_request() {
                    Ok(request) => request,
                    Err(_) => continue,
                };
                let (response, stop_now) = handle_control_request(
                    &namespace,
                    instance,
                    &mut runtime,
                    &mut events,
                    &mut next_event_id,
                    request.trim(),
                );
                if connection.write_response(&response).is_err() {
                    continue;
                }
                shutdown_pending = shutdown_pending || stop_now;
            }

            let events_connection = match events_server.try_accept() {
                Ok(connection) => connection,
                Err(_) => None,
            };

            if let Some(mut connection) = events_connection {
                did_work = true;

                let request = match connection.read_request() {
                    Ok(request) => request,
                    Err(_) => continue,
                };
                let response = handle_events_request(instance, &mut events, request.trim());
                if connection.write_response(&response).is_err() {
                    continue;
                }
            }

            if !did_work {
                if shutdown_pending && events.items.is_empty() {
                    break;
                }
                thread::sleep(Duration::from_millis(policy.poll_idle_sleep_ms));
            }
        }

        Ok(())
    }

    pub fn send_ipc_request(
        &self,
        namespace_prefix: Option<&str>,
        instance: &str,
        request: &str,
    ) -> Result<String> {
        let namespace = namespace_for(namespace_prefix);
        let channel = ipc_control_channel(&namespace, instance);
        Ok(ipc_request(&channel, request)?)
    }

    pub fn send_ipc_event_request(
        &self,
        namespace_prefix: Option<&str>,
        instance: &str,
        request: &str,
    ) -> Result<String> {
        let namespace = namespace_for(namespace_prefix);
        let channel = ipc_events_channel(&namespace, instance);
        Ok(ipc_request(&channel, request)?)
    }
}

fn namespace_for(prefix: Option<&str>) -> ChannelNamespace {
    match prefix {
        Some(prefix) => ChannelNamespace::from_prefix(prefix),
        None => ChannelNamespace::from_env(),
    }
}

fn handle_control_request(
    namespace: &ChannelNamespace,
    instance: &str,
    runtime: &mut BlackBoxRuntime,
    events: &mut EventQueueState,
    next_event_id: &mut u64,
    request: &str,
) -> (String, bool) {
    if request.eq_ignore_ascii_case("inspect") {
        let document = runtime.debug_document(None).to_json();
        push_event(events, next_event_id, "runtime.inspected", &document);
        return (ok_response("inspect", &document), false);
    }

    if request.eq_ignore_ascii_case("events") {
        return (event_catalog_response(namespace, instance), false);
    }

    if let Some(behavior) = request
        .strip_prefix("set-behavior ")
        .and_then(parse_behavior_phase)
    {
        runtime.handle_event(RuntimeEvent::DevelopmentSetBehavior(behavior));
        let document = runtime.debug_document(Some(behavior)).to_json();
        push_event(events, next_event_id, "runtime.behavior-changed", &document);
        return (ok_response("set-behavior", &document), false);
    }

    if request.eq_ignore_ascii_case("shutdown") {
        let data = format!(
            "{{\"runtime\":\"black-box\",\"status\":\"shutting-down\",\"snapshot\":{}}}",
            runtime.debug_snapshot_json()
        );
        push_event(events, next_event_id, "runtime.shutdown-requested", &data);
        return (ok_response("shutdown", &data), true);
    }

    if request
        .strip_prefix("set-behavior ")
        .is_some_and(|value| parse_behavior_phase(value).is_none())
    {
        push_event(
            events,
            next_event_id,
            "ipc.error",
            &error_event_data("invalid-request", "expected one of: booting, idle, closing"),
        );
        return (
            error_response("invalid-request", "expected one of: booting, idle, closing"),
            false,
        );
    }

    push_event(
        events,
        next_event_id,
        "ipc.error",
        &error_event_data("unknown-request", &format!("unknown request: {}", request)),
    );

    (
        error_response("unknown-request", &format!("unknown request: {}", request)),
        false,
    )
}

fn handle_events_request(instance: &str, events: &mut EventQueueState, request: &str) -> String {
    if request.eq_ignore_ascii_case("poll") {
        let drained = format!("[{}]", events.items.join(","));
        let dropped_since_last_poll = events.dropped_since_last_poll;
        let total_dropped = events.total_dropped;
        let drained_count = events.items.len();
        events.items.clear();
        events.dropped_since_last_poll = 0;

        return ok_response(
            "events.poll",
            &format!(
                concat!(
                    "{{",
                    "\"runtime\":\"black-box\"",
                    ",\"instance\":\"{}\"",
                    ",\"events\":{}",
                    ",\"drained_count\":{}",
                    ",\"dropped_since_last_poll\":{}",
                    ",\"total_dropped\":{}",
                    ",\"queue_capacity\":{}",
                    "}}"
                ),
                instance,
                drained,
                drained_count,
                dropped_since_last_poll,
                total_dropped,
                IpcPolicy::default().event_queue_capacity,
            ),
        );
    }

    error_response(
        "unknown-events-request",
        &format!("unknown events request: {}", request),
    )
}

fn ipc_control_channel(namespace: &ChannelNamespace, instance: &str) -> stui_ipc::ChannelName {
    namespace.channel("black-box", instance, "debug")
}

fn ipc_events_channel(namespace: &ChannelNamespace, instance: &str) -> stui_ipc::ChannelName {
    namespace.channel("black-box", instance, "events")
}

fn event_catalog_response(namespace: &ChannelNamespace, instance: &str) -> String {
    let control_channel = ipc_control_channel(namespace, instance);
    let events_channel = ipc_events_channel(namespace, instance);

    ok_response(
        "events",
        &format!(
            "{{\"catalog\":{},\"queue\":{{\"capacity\":{},\"drop_policy\":\"{}\"}}}}",
            IpcEventCatalog {
                runtime: "black-box",
                instance,
                transport: "local-request-response",
                events_channel: events_channel.as_str(),
                control_channel: control_channel.as_str(),
                mode: "request-response-poll",
                emits: BLACK_BOX_EVENT_NAMES,
            }
            .to_json(),
            IpcPolicy::default().event_queue_capacity,
            IpcPolicy::default().event_queue_drop_policy().as_str(),
        ),
    )
}

fn push_event(events: &mut EventQueueState, next_event_id: &mut u64, kind: &str, data_json: &str) {
    match IpcPolicy::default().event_queue_drop_policy() {
        EventQueueDropPolicy::DropOldest => {
            if events.items.len() >= IpcPolicy::default().event_queue_capacity {
                events.items.remove(0);
                events.dropped_since_last_poll += 1;
                events.total_dropped += 1;
            }
        }
    }

    events.items.push(format!(
        "{{\"id\":{},\"kind\":\"{}\",\"data\":{}}}",
        *next_event_id, kind, data_json
    ));
    *next_event_id += 1;
}

fn error_event_data(code: &str, detail: &str) -> String {
    format!(
        "{{\"code\":\"{}\",\"detail\":\"{}\"}}",
        stui_ipc::escape_json_string(code),
        stui_ipc::escape_json_string(detail)
    )
}

fn parse_behavior_phase(value: &str) -> Option<BehaviorPhase> {
    match value.trim().to_ascii_lowercase().as_str() {
        "booting" => Some(BehaviorPhase::Booting),
        "idle" => Some(BehaviorPhase::Idle),
        "closing" => Some(BehaviorPhase::Closing),
        _ => None,
    }
}
