use std::{
    env,
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

const STUI_DEV_IPC_NAMESPACE_PREFIX_ENV: &str = "STUI_DEV_IPC_NAMESPACE_PREFIX";
const BUILD_TIMEOUT_MS: u64 = 120_000;
const START_TIMEOUT_MS: u64 = 15_000;
const REQUEST_TIMEOUT_MS: u64 = 5_000;
const STOP_TIMEOUT_MS: u64 = 5_000;
const POLL_INTERVAL_MS: u64 = 100;
const START_ATTEMPTS: usize = 2;

#[derive(Debug, Parser)]
#[command(name = "stui-dev")]
#[command(about = "Supervisor for stui playground development processes")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    Targets,
    Prune,
    Start(StartArgs),
    Stop(TargetArgs),
    Restart(StartArgs),
    Status(TargetArgs),
    StatusAll,
    Inspect(TargetArgs),
    Events(TargetArgs),
    Logs(TargetArgs),
}

#[derive(Debug, Clone, clap::Args)]
struct TargetArgs {
    playground: PlaygroundName,
    #[arg(long, default_value = "default")]
    instance: String,
    #[arg(long, default_value_t = 40)]
    lines: usize,
}

#[derive(Debug, Clone, clap::Args)]
struct StartArgs {
    playground: PlaygroundName,
    #[arg(long, default_value = "default")]
    instance: String,
    #[arg(long, value_enum)]
    behavior: Option<BehaviorArg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PlaygroundName {
    BlackBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BehaviorArg {
    Booting,
    Idle,
    Closing,
}

#[derive(Debug, Clone)]
struct StateRecord {
    playground: PlaygroundName,
    instance: String,
    namespace_prefix: Option<String>,
    log_path: PathBuf,
    session_id: String,
    pid: u32,
    started_at_ms: u128,
    binary_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeStatus {
    Running,
    Stopped,
    Stale,
}

#[derive(Debug, Clone)]
struct StatusFact {
    status: RuntimeStatus,
    stale_reason: Option<&'static str>,
}

#[derive(Debug)]
struct TimedOutput {
    output: Output,
    timed_out: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Action::Targets => targets_action(),
        Action::Prune => prune_action(),
        Action::Start(args) => start_target(args),
        Action::Stop(args) => stop_target(args),
        Action::Restart(args) => {
            stop_target(TargetArgs {
                playground: args.playground,
                instance: args.instance.clone(),
                lines: 40,
            })?;
            start_target(args)
        }
        Action::Status(args) => status_target(args),
        Action::StatusAll => status_all_action(),
        Action::Inspect(args) => inspect_target(args),
        Action::Events(args) => events_target(args),
        Action::Logs(args) => logs_target(args),
    }
}

fn targets_action() -> Result<()> {
    for playground in PlaygroundName::all() {
        println!("{}", playground.as_str());
    }

    Ok(())
}

fn prune_action() -> Result<()> {
    let states = read_all_states()?;
    if states.is_empty() {
        println!("status=empty removed=0");
        return Ok(());
    }

    let mut removed = 0usize;
    let mut kept = 0usize;

    for state in states {
        let fact = classify_status(Some(&state));
        if fact.status == RuntimeStatus::Stale {
            cleanup_stale_state(&state)?;
            removed += 1;
            println!(
                "status=pruned playground={} instance={} session={} stale_reason={}",
                state.playground.as_str(),
                state.instance,
                state.session_id,
                fact.stale_reason.unwrap_or("unknown")
            );
        } else {
            kept += 1;
        }
    }

    println!("status=complete removed={} kept={}", removed, kept);
    Ok(())
}

fn start_target(args: StartArgs) -> Result<()> {
    let namespace_prefix = dev_namespace_prefix();

    println!(
        "stage=build playground={} instance={}",
        args.playground.as_str(),
        args.instance
    );

    if let Some(existing) = read_state(args.playground, &args.instance)? {
        if request_inspect(&existing).is_ok() {
            bail!(
                "{} {} is already running",
                args.playground.as_str(),
                args.instance
            );
        }

        cleanup_stale_state(&existing)?;
    }

    ensure_runtime_dirs()?;
    build_playground(args.playground)?;

    let binary_path = playground_binary_path(args.playground);

    let mut last_error = None;
    let mut final_state = None;

    for attempt in 1..=START_ATTEMPTS {
        let session_id = create_session_id();
        let log_path = log_path(args.playground, &args.instance, &session_id);
        let log_file = File::create(&log_path).context("create dev log file")?;
        let stderr_file = log_file.try_clone().context("clone dev log file handle")?;

        let mut command = Command::new(&binary_path);
        command
            .arg("serve")
            .arg("--instance")
            .arg(&args.instance)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(stderr_file))
            .stdin(Stdio::null())
            .current_dir(repo_root());

        if let Some(prefix) = namespace_prefix.as_deref() {
            command.env(STUI_DEV_IPC_NAMESPACE_PREFIX_ENV, prefix);
        }

        if let Some(behavior) = args.behavior {
            command.arg("--behavior").arg(behavior.as_str());
        }

        let child = command.spawn().context("spawn playground dev process")?;
        println!(
            "stage=launch playground={} instance={} pid={} attempt={}/{}",
            args.playground.as_str(),
            args.instance,
            child.id(),
            attempt,
            START_ATTEMPTS
        );

        let state = StateRecord {
            playground: args.playground,
            instance: args.instance.clone(),
            namespace_prefix: namespace_prefix.clone(),
            log_path,
            session_id,
            pid: child.id(),
            started_at_ms: now_ms(),
            binary_path: binary_path.clone(),
        };

        write_state(&state)?;

        match wait_for_readiness(&state) {
            Ok(()) => {
                final_state = Some(state);
                break;
            }
            Err(error) => {
                println!(
                    "stage=retry playground={} instance={} attempt={}/{} detail={}",
                    args.playground.as_str(),
                    args.instance,
                    attempt,
                    START_ATTEMPTS,
                    error
                );
                let _ = force_kill_pid(state.pid);
                let _ = remove_state_file(state.playground, &state.instance);
                last_error = Some(error);
            }
        }
    }

    let Some(state) = final_state else {
        return Err(last_error.unwrap_or_else(|| anyhow!("playground failed to become ready")));
    };

    println!(
        "stage=ready playground={} instance={} session={}",
        state.playground.as_str(),
        state.instance,
        state.session_id
    );

    println!(
        "status=running playground={} instance={} pid={} session={} namespace={} log={}",
        state.playground.as_str(),
        state.instance,
        state.pid,
        state.session_id,
        state.namespace_prefix.as_deref().unwrap_or("<default>"),
        state.log_path.display()
    );

    Ok(())
}

fn stop_target(args: TargetArgs) -> Result<()> {
    let Some(state) = read_state(args.playground, &args.instance)? else {
        println!(
            "status=stopped playground={} instance={}",
            args.playground.as_str(),
            args.instance
        );
        return Ok(());
    };

    let response = request_control(&state, "shutdown");
    let graceful = wait_for_shutdown(&state).is_ok();

    if !graceful {
        force_kill_pid(state.pid).context("force kill stale playground process")?;
    }

    remove_state_file(args.playground, &args.instance)?;

    match response {
        Ok(output) => println!("{output}"),
        Err(error) if graceful => println!("status=stopped detail={error}"),
        Err(error) => println!("status=stopped fallback=force-kill detail={error}"),
    }

    Ok(())
}

fn status_target(args: TargetArgs) -> Result<()> {
    let state = read_state(args.playground, &args.instance)?;
    let fact = classify_status(state.as_ref());

    match state {
        Some(state) => println!(
            "status={} playground={} instance={} session={} namespace={} pid={} started_at_ms={} log={}{}",
            fact.status.as_str(),
            state.playground.as_str(),
            state.instance,
            state.session_id,
            state.namespace_prefix.as_deref().unwrap_or("<default>"),
            state.pid,
            state.started_at_ms,
            state.log_path.display(),
            fact
                .stale_reason
                .map(|reason| format!(" stale_reason={reason}"))
                .unwrap_or_default()
        ),
        None => println!(
            "status=stopped playground={} instance={}",
            args.playground.as_str(),
            args.instance
        ),
    }

    Ok(())
}

fn status_all_action() -> Result<()> {
    let states = read_all_states()?;

    if states.is_empty() {
        println!("status=empty");
        return Ok(());
    }

    let mut running = 0usize;
    let mut stale = 0usize;

    for state in states {
        let fact = classify_status(Some(&state));
        match fact.status {
            RuntimeStatus::Running => running += 1,
            RuntimeStatus::Stale => stale += 1,
            RuntimeStatus::Stopped => {}
        }
        println!(
            "status={} playground={} instance={} session={} namespace={} pid={} started_at_ms={} log={}{}",
            fact.status.as_str(),
            state.playground.as_str(),
            state.instance,
            state.session_id,
            state.namespace_prefix.as_deref().unwrap_or("<default>"),
            state.pid,
            state.started_at_ms,
            state.log_path.display(),
            fact
                .stale_reason
                .map(|reason| format!(" stale_reason={reason}"))
                .unwrap_or_default()
        );
    }

    println!(
        "summary running={} stale={} total={}",
        running,
        stale,
        running + stale
    );

    Ok(())
}

fn inspect_target(args: TargetArgs) -> Result<()> {
    let state = require_state(args.playground, &args.instance)?;
    println!("{}", request_inspect(&state)?);
    Ok(())
}

fn events_target(args: TargetArgs) -> Result<()> {
    let state = require_state(args.playground, &args.instance)?;
    println!("{}", request_events(&state, "poll")?);
    Ok(())
}

fn logs_target(args: TargetArgs) -> Result<()> {
    let state = require_state(args.playground, &args.instance)?;
    let contents = fs::read_to_string(&state.log_path)
        .with_context(|| format!("read log file {}", state.log_path.display()))?;
    let lines = tail_lines(&contents, args.lines);
    if lines.is_empty() {
        println!(
            "status=empty-log playground={} instance={} session={} log={}",
            state.playground.as_str(),
            state.instance,
            state.session_id,
            state.log_path.display()
        );
    } else {
        print!("{lines}");
    }
    Ok(())
}

fn build_playground(playground: PlaygroundName) -> Result<()> {
    let result = run_command_with_timeout(
        {
            let mut command = Command::new("cargo");
            command
                .arg("build")
                .arg("-p")
                .arg(playground.package_name())
                .arg("--bin")
                .arg(playground.binary_name())
                .current_dir(repo_root());
            command
        },
        Duration::from_millis(BUILD_TIMEOUT_MS),
    )
    .context("build playground binary")?;

    if result.timed_out {
        bail!("build timed out after {}ms", BUILD_TIMEOUT_MS);
    }

    if !result.output.status.success() {
        bail!(
            "build failed: {}{}{}",
            String::from_utf8_lossy(&result.output.stdout).trim(),
            if !result.output.stdout.is_empty() && !result.output.stderr.is_empty() {
                " | "
            } else {
                ""
            },
            String::from_utf8_lossy(&result.output.stderr).trim()
        );
    }

    Ok(())
}

fn wait_for_readiness(state: &StateRecord) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(START_TIMEOUT_MS);

    loop {
        match request_inspect(state) {
            Ok(_) => return Ok(()),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error).context("timed out waiting for playground readiness");
                }
            }
        }

        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

fn wait_for_shutdown(state: &StateRecord) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(STOP_TIMEOUT_MS);

    loop {
        if request_inspect(state).is_err() {
            return Ok(());
        }

        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for {} {} to stop gracefully",
                state.playground.as_str(),
                state.instance
            );
        }

        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

fn request_inspect(state: &StateRecord) -> Result<String> {
    request_playground(
        state,
        &["request", "--instance", &state.instance, "inspect"],
    )
}

fn request_control(state: &StateRecord, request: &str) -> Result<String> {
    request_playground(state, &["request", "--instance", &state.instance, request])
}

fn request_events(state: &StateRecord, request: &str) -> Result<String> {
    request_playground(
        state,
        &[
            "request",
            "--instance",
            &state.instance,
            "--surface",
            "events",
            request,
        ],
    )
}

fn request_playground(state: &StateRecord, args: &[&str]) -> Result<String> {
    let mut command = Command::new(&state.binary_path);
    command.args(args).current_dir(repo_root());

    if let Some(prefix) = state.namespace_prefix.as_deref() {
        command.env(STUI_DEV_IPC_NAMESPACE_PREFIX_ENV, prefix);
    }

    let result = run_command_with_timeout(command, Duration::from_millis(REQUEST_TIMEOUT_MS))
        .context("run playground request command")?;

    if result.timed_out {
        bail!("request timed out after {}ms", REQUEST_TIMEOUT_MS);
    }

    if result.output.status.success() {
        return Ok(String::from_utf8_lossy(&result.output.stdout)
            .trim()
            .to_string());
    }

    let stderr = String::from_utf8_lossy(&result.output.stderr)
        .trim()
        .to_string();
    let stdout = String::from_utf8_lossy(&result.output.stdout)
        .trim()
        .to_string();
    Err(anyhow!(
        "playground command failed: {}{}{}",
        stdout,
        if !stdout.is_empty() && !stderr.is_empty() {
            " | "
        } else {
            ""
        },
        stderr
    ))
}

fn run_command_with_timeout(mut command: Command, timeout: Duration) -> Result<TimedOutput> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command.spawn().context("spawn timed command")?;
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(_status) = child.try_wait().context("poll timed command")? {
            let output = child.wait_with_output().context("collect command output")?;
            return Ok(TimedOutput {
                output,
                timed_out: false,
            });
        }

        if Instant::now() >= deadline {
            child.kill().context("kill timed out command")?;
            let output = child.wait_with_output().context("collect timeout output")?;
            return Ok(TimedOutput {
                output,
                timed_out: true,
            });
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn classify_status(state: Option<&StateRecord>) -> StatusFact {
    match state {
        None => StatusFact {
            status: RuntimeStatus::Stopped,
            stale_reason: None,
        },
        Some(state) => {
            if request_inspect(state).is_ok() {
                StatusFact {
                    status: RuntimeStatus::Running,
                    stale_reason: None,
                }
            } else if process_exists(state.pid).unwrap_or(false) {
                StatusFact {
                    status: RuntimeStatus::Stale,
                    stale_reason: Some("inspect-unreachable"),
                }
            } else {
                StatusFact {
                    status: RuntimeStatus::Stale,
                    stale_reason: Some("pid-not-running"),
                }
            }
        }
    }
}

fn tail_lines(contents: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }

    let collected = contents.lines().collect::<Vec<_>>();
    let start = collected.len().saturating_sub(limit);
    let mut output = collected[start..].join("\n");
    if contents.ends_with('\n') && !output.is_empty() {
        output.push('\n');
    }
    output
}

fn process_exists(pid: u32) -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        let result = run_command_with_timeout(
            {
                let mut command = Command::new("tasklist");
                command.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
                command
            },
            Duration::from_millis(REQUEST_TIMEOUT_MS),
        )?;

        if result.timed_out {
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&result.output.stdout);
        Ok(stdout.contains(&format!(",\"{}\"", pid)) || stdout.contains(&format!("\"{}\"", pid)))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("run kill -0")?;
        Ok(status.success())
    }
}

fn cleanup_stale_state(state: &StateRecord) -> Result<()> {
    let _ = force_kill_pid(state.pid);
    remove_state_file(state.playground, &state.instance)
}

fn read_state(playground: PlaygroundName, instance: &str) -> Result<Option<StateRecord>> {
    let path = state_path(playground, instance);
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read state {}", path.display())),
    };
    let Some(record) = parse_state_source(&source)? else {
        return Ok(None);
    };

    if record.playground != playground || record.instance != instance {
        return Ok(None);
    }

    Ok(Some(record))
}

fn require_state(playground: PlaygroundName, instance: &str) -> Result<StateRecord> {
    read_state(playground, instance)?.ok_or_else(|| {
        anyhow!(
            "no recorded {} instance named {}; start it first",
            playground.as_str(),
            instance
        )
    })
}

fn read_all_states() -> Result<Vec<StateRecord>> {
    let dir = state_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read state dir {}", dir.display()))
        }
    };

    let mut states = Vec::new();
    for entry in entries {
        let entry = entry.context("read state dir entry")?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("state") {
            continue;
        }

        let source =
            fs::read_to_string(&path).with_context(|| format!("read state {}", path.display()))?;
        if let Some(state) = parse_state_source(&source)? {
            states.push(state);
        }
    }

    states.sort_by(|a, b| {
        a.playground
            .as_str()
            .cmp(b.playground.as_str())
            .then(a.instance.cmp(&b.instance))
    });
    Ok(states)
}

fn write_state(state: &StateRecord) -> Result<()> {
    let path = state_path(state.playground, &state.instance);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    fs::write(
        &path,
        format!(
            concat!(
                "playground={}\n",
                "instance={}\n",
                "namespace_prefix={}\n",
                "log_path={}\n",
                "session_id={}\n",
                "pid={}\n",
                "started_at_ms={}\n",
                "binary_path={}\n"
            ),
            state.playground.as_str(),
            state.instance,
            state.namespace_prefix.as_deref().unwrap_or(""),
            state.log_path.display(),
            state.session_id,
            state.pid,
            state.started_at_ms,
            state.binary_path.display(),
        ),
    )
    .with_context(|| format!("write state {}", path.display()))
}

fn parse_state_source(source: &str) -> Result<Option<StateRecord>> {
    let mut playground = None;
    let mut instance = None;
    let mut namespace_prefix = None;
    let mut log_path = PathBuf::new();
    let mut session_id = String::new();
    let mut pid = 0;
    let mut started_at_ms = 0;
    let mut binary_path = PathBuf::new();

    for line in source.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "playground" => playground = Some(PlaygroundName::from_str(value)?),
            "instance" => instance = Some(value.to_string()),
            "namespace_prefix" => {
                namespace_prefix = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }
            "log_path" => log_path = PathBuf::from(value),
            "session_id" => session_id = value.to_string(),
            "pid" => pid = value.parse().unwrap_or_default(),
            "started_at_ms" => started_at_ms = value.parse().unwrap_or_default(),
            "binary_path" => binary_path = PathBuf::from(value),
            _ => {}
        }
    }

    let Some(playground) = playground else {
        return Ok(None);
    };
    let Some(instance) = instance else {
        return Ok(None);
    };
    if binary_path.as_os_str().is_empty() {
        binary_path = playground_binary_path(playground);
    }

    Ok(Some(StateRecord {
        playground,
        instance,
        namespace_prefix,
        log_path,
        session_id,
        pid,
        started_at_ms,
        binary_path,
    }))
}

fn remove_state_file(playground: PlaygroundName, instance: &str) -> Result<()> {
    let path = state_path(playground, instance);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove state {}", path.display())),
    }
}

fn ensure_runtime_dirs() -> Result<()> {
    fs::create_dir_all(state_dir()).context("create stui-dev state dir")?;
    fs::create_dir_all(log_dir()).context("create stui-dev log dir")?;
    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn target_debug_dir() -> PathBuf {
    repo_root().join("target").join("debug")
}

fn playground_binary_path(playground: PlaygroundName) -> PathBuf {
    target_debug_dir().join(format!(
        "{}{}",
        playground.binary_name(),
        env::consts::EXE_SUFFIX
    ))
}

fn dev_runtime_root() -> PathBuf {
    repo_root().join(".tmp").join("stui-dev")
}

fn state_dir() -> PathBuf {
    dev_runtime_root().join("state")
}

fn log_dir() -> PathBuf {
    dev_runtime_root().join("logs")
}

fn state_path(playground: PlaygroundName, instance: &str) -> PathBuf {
    state_dir().join(format!(
        "{}--{}.state",
        playground.as_str(),
        sanitize(instance)
    ))
}

fn log_path(playground: PlaygroundName, instance: &str, session_id: &str) -> PathBuf {
    log_dir().join(format!(
        "{}--{}--{}.log",
        playground.as_str(),
        sanitize(instance),
        session_id
    ))
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn create_session_id() -> String {
    now_ms().to_string()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis()
}

fn dev_namespace_prefix() -> Option<String> {
    env::var(STUI_DEV_IPC_NAMESPACE_PREFIX_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn force_kill_pid(pid: u32) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("run taskkill")?;

        if status.success() {
            return Ok(());
        }

        return Err(anyhow!("taskkill failed for pid {pid}"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let status = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("run kill")?;

        if status.success() {
            return Ok(());
        }

        Err(anyhow!("kill failed for pid {pid}"))
    }
}

impl PlaygroundName {
    fn all() -> &'static [Self] {
        &[Self::BlackBox]
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::BlackBox => "black-box",
        }
    }

    fn package_name(self) -> &'static str {
        match self {
            Self::BlackBox => "stui-playground-black-box",
        }
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::BlackBox => "stui-playground-black-box",
        }
    }

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "black-box" => Ok(Self::BlackBox),
            other => bail!("unsupported playground in state: {other}"),
        }
    }
}

impl BehaviorArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "booting",
            Self::Idle => "idle",
            Self::Closing => "closing",
        }
    }
}

impl RuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Stale => "stale",
        }
    }
}
