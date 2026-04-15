use std::{
    env,
    fs::{self, File},
    io::{ErrorKind, Write},
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
const READY_MARKER_PREFIX: &str = "status=ready playground=";
const STATE_STALE_GRACE_MS: u128 = 2_000;
const PROCESS_DISCOVERY_TIMEOUT_MS: u64 = 2_000;

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
    State(StateArgs),
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
    #[arg(long, default_value_t = false)]
    build: bool,
    #[arg(long, value_enum)]
    behavior: Option<BehaviorArg>,
}

#[derive(Debug, Clone, clap::Args)]
struct StateArgs {
    playground: Option<PlaygroundName>,
    #[arg(long, default_value = "default")]
    instance: String,
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

#[derive(Debug, Clone)]
struct ObservedTarget {
    playground: PlaygroundName,
    instance: String,
    namespace_prefix: String,
    log_path: PathBuf,
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

#[derive(Debug)]
struct NamespaceOpGuard {
    path: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _namespace_guard = if cli.command.requires_namespace_lock() {
        Some(acquire_namespace_op_lock(cli.command.as_str())?)
    } else {
        None
    };

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
        Action::State(args) => state_action(args),
        Action::Inspect(args) => inspect_target(args),
        Action::Events(args) => events_target(args),
        Action::Logs(args) => logs_target(args),
    }
}

fn acquire_namespace_op_lock(command_name: &str) -> Result<NamespaceOpGuard> {
    ensure_runtime_dirs()?;
    fs::create_dir_all(lock_dir()).context("create stui-dev lock dir")?;

    let namespace = dev_namespace_prefix();
    let path = lock_path(&namespace);

    match write_lock_file(&path, command_name) {
        Ok(()) => Ok(NamespaceOpGuard { path }),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            if clear_stale_namespace_lock(&path)? {
                write_lock_file(&path, command_name)
                    .with_context(|| format!("acquire namespace lock {}", path.display()))?;
                Ok(NamespaceOpGuard { path })
            } else {
                bail!(
                    "namespace busy: {} command={} lock={}",
                    namespace,
                    command_name,
                    path.display()
                );
            }
        }
        Err(error) => {
            Err(error).with_context(|| format!("acquire namespace lock {}", path.display()))
        }
    }
}

fn write_lock_file(path: &Path, command_name: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "command={}", command_name)?;
    Ok(())
}

fn clear_stale_namespace_lock(path: &Path) -> Result<bool> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error).with_context(|| format!("read lock file {}", path.display()))
        }
    };

    let pid = source
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.parse::<u32>().ok());

    if let Some(pid) = pid {
        if process_exists(pid).unwrap_or(false) {
            return Ok(false);
        }
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error).with_context(|| format!("remove stale lock {}", path.display())),
    }
}

fn targets_action() -> Result<()> {
    for playground in PlaygroundName::all() {
        println!("{}", playground.as_str());
    }

    Ok(())
}

fn prune_action() -> Result<()> {
    let removed_logs = prune_logs()?;
    let removed_locks = prune_locks()?;
    let removed_state_dirs = prune_state_dirs()?;
    println!(
        "status=complete removed_logs={} removed_locks={} removed_state_dirs={}",
        removed_logs, removed_locks, removed_state_dirs
    );
    Ok(())
}

fn start_target(args: StartArgs) -> Result<()> {
    let namespace_prefix = dev_namespace_prefix();

    if find_target(args.playground, &args.instance, &namespace_prefix)?.is_some() {
        bail!(
            "{} {} is already running in namespace {}",
            args.playground.as_str(),
            args.instance,
            namespace_prefix
        );
    }

    ensure_runtime_dirs()?;

    if args.build {
        println!(
            "stage=build playground={} instance={}",
            args.playground.as_str(),
            args.instance
        );
        build_playground(args.playground)?;
    }

    let binary_path = playground_binary_path(args.playground);

    let mut last_error = None;
    let mut final_state = None;

    for attempt in 1..=START_ATTEMPTS {
        let session_id = create_session_id();
        let log_path = log_path(&namespace_prefix, args.playground, &session_id);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let log_file = File::create(&log_path).context("create dev log file")?;
        let stderr_file = log_file.try_clone().context("clone dev log file handle")?;

        let mut command = Command::new(&binary_path);
        command
            .arg("serve")
            .arg("--instance")
            .arg(&args.instance)
            .arg("--namespace-prefix")
            .arg(&namespace_prefix)
            .arg("--log-path")
            .arg(log_path.as_os_str())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(stderr_file))
            .stdin(Stdio::null())
            .current_dir(repo_root());

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
            namespace_prefix: Some(namespace_prefix.clone()),
            log_path,
            session_id,
            pid: child.id(),
            started_at_ms: now_ms(),
            binary_path: binary_path.clone(),
        };

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
    let namespace_prefix = dev_namespace_prefix();
    let Some(state) = find_target(args.playground, &args.instance, &namespace_prefix)? else {
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

    match response {
        Ok(output) => println!("{output}"),
        Err(error) if graceful => println!("status=stopped detail={error}"),
        Err(error) => println!("status=stopped fallback=force-kill detail={error}"),
    }

    Ok(())
}

fn state_action(args: StateArgs) -> Result<()> {
    match args.playground {
        Some(playground) => state_target(playground, &args.instance),
        None => state_all_action(),
    }
}

fn state_target(playground: PlaygroundName, instance: &str) -> Result<()> {
    let namespace_prefix = dev_namespace_prefix();
    let state = find_target(playground, instance, &namespace_prefix)?;
    let fact = classify_status(state.as_ref());

    match state {
        Some(state) => println!(
            "status={} playground={} instance={} session={} namespace={} pid={} started_at_ms={} log={}{}",
            fact.status.as_str(),
            state.playground.as_str(),
            state.instance,
            state.session_id,
            state.namespace_prefix.as_deref().unwrap_or("default"),
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
            playground.as_str(),
            instance
        ),
    }

    Ok(())
}

fn state_all_action() -> Result<()> {
    let namespace_prefix = dev_namespace_prefix();
    let states = find_all_targets(&namespace_prefix)?;

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
    let state = require_target(args.playground, &args.instance, &dev_namespace_prefix())?;
    println!("{}", request_inspect(&state)?);
    Ok(())
}

fn events_target(args: TargetArgs) -> Result<()> {
    let state = require_target(args.playground, &args.instance, &dev_namespace_prefix())?;
    println!("{}", request_events(&state, "poll")?);
    Ok(())
}

fn logs_target(args: TargetArgs) -> Result<()> {
    let state = require_target(args.playground, &args.instance, &dev_namespace_prefix())?;
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
        if log_reports_ready(state)? {
            return Ok(());
        }

        if !process_exists(state.pid).unwrap_or(false) {
            bail!(
                "playground process exited before readiness: {} {}",
                state.playground.as_str(),
                state.instance
            );
        }

        if Instant::now() >= deadline {
            bail!("timed out waiting for playground readiness");
        }

        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

fn log_reports_ready(state: &StateRecord) -> Result<bool> {
    let contents = match fs::read_to_string(&state.log_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read readiness log {}", state.log_path.display()))
        }
    };

    Ok(contents.lines().any(|line| {
        line.starts_with(READY_MARKER_PREFIX)
            && line.contains(&format!("playground={}", state.playground.as_str()))
            && line.contains(&format!("instance={}", state.instance))
    }))
}

fn wait_for_shutdown(state: &StateRecord) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(STOP_TIMEOUT_MS);

    loop {
        if !process_exists(state.pid)? {
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
    command
        .args(args)
        .arg("--namespace-prefix")
        .arg(state.namespace_prefix.as_deref().unwrap_or("default"))
        .current_dir(repo_root());

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
            if process_exists(state.pid).unwrap_or(false)
                && log_reports_ready(state).unwrap_or(false)
            {
                StatusFact {
                    status: RuntimeStatus::Running,
                    stale_reason: None,
                }
            } else if process_exists(state.pid).unwrap_or(false)
                && now_ms().saturating_sub(state.started_at_ms) <= STATE_STALE_GRACE_MS
            {
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

fn require_target(
    playground: PlaygroundName,
    instance: &str,
    namespace_prefix: &str,
) -> Result<StateRecord> {
    find_target(playground, instance, namespace_prefix)?.ok_or_else(|| {
        anyhow!(
            "no running {} instance named {} in namespace {}; start it first",
            playground.as_str(),
            instance,
            namespace_prefix,
        )
    })
}

fn ensure_runtime_dirs() -> Result<()> {
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
    let repo_binary = repo_debug_binary_path(playground);
    let current_exe = env::current_exe().ok();
    let env_override = env::var_os(playground.binary_override_env())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let sibling_binary = current_exe
        .as_ref()
        .and_then(|path| sibling_binary_path(path, playground));
    let path_binary = find_binary_on_path(playground.binary_name());
    let prefer_repo_debug = current_exe
        .as_ref()
        .is_some_and(|path| is_repo_target_binary(path));

    let mut candidates = Vec::new();
    if let Some(path) = env_override {
        candidates.push(path);
    }

    if prefer_repo_debug {
        candidates.push(repo_binary.clone());
        if let Some(path) = sibling_binary {
            candidates.push(path);
        }
        if let Some(path) = path_binary {
            candidates.push(path);
        }
    } else {
        if let Some(path) = sibling_binary {
            candidates.push(path);
        }
        if let Some(path) = path_binary {
            candidates.push(path);
        }
        candidates.push(repo_binary.clone());
    }

    candidates.dedup();
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or(repo_binary)
}

fn repo_debug_binary_path(playground: PlaygroundName) -> PathBuf {
    target_debug_dir().join(format!(
        "{}{}",
        playground.binary_name(),
        env::consts::EXE_SUFFIX
    ))
}

fn sibling_binary_path(current_exe: &Path, playground: PlaygroundName) -> Option<PathBuf> {
    current_exe.parent().map(|dir| {
        dir.join(format!(
            "{}{}",
            playground.binary_name(),
            env::consts::EXE_SUFFIX
        ))
    })
}

fn find_binary_on_path(binary_name: &str) -> Option<PathBuf> {
    let executable = format!("{}{}", binary_name, env::consts::EXE_SUFFIX);
    let path = env::var_os("PATH")?;

    env::split_paths(&path)
        .map(|dir| dir.join(&executable))
        .find(|candidate| candidate.is_file())
}

fn is_repo_target_binary(path: &Path) -> bool {
    path.starts_with(repo_root().join("target"))
}

fn dev_runtime_root() -> PathBuf {
    repo_root().join(".tmp").join("stui-dev")
}

fn log_dir() -> PathBuf {
    dev_runtime_root().join("logs")
}

fn state_dir() -> PathBuf {
    dev_runtime_root().join("state")
}

fn lock_dir() -> PathBuf {
    dev_runtime_root().join("locks")
}

fn lock_path(namespace: &str) -> PathBuf {
    lock_dir().join(format!("{}.lock", sanitize(namespace)))
}

fn log_path(namespace_prefix: &str, playground: PlaygroundName, session_id: &str) -> PathBuf {
    log_dir()
        .join(sanitize(namespace_prefix))
        .join(playground.binary_name())
        .join(format!("{}.log", session_id))
}

fn find_target(
    playground: PlaygroundName,
    instance: &str,
    namespace_prefix: &str,
) -> Result<Option<StateRecord>> {
    Ok(find_all_targets(namespace_prefix)?
        .into_iter()
        .find(|target| target.playground == playground && target.instance == instance))
}

fn find_all_targets(namespace_prefix: &str) -> Result<Vec<StateRecord>> {
    let mut states = discover_targets(PlaygroundName::BlackBox)?
        .into_iter()
        .filter(|target| target.namespace_prefix == namespace_prefix)
        .map(observed_to_state)
        .collect::<Vec<_>>();

    states.sort_by(|a, b| {
        a.playground
            .as_str()
            .cmp(b.playground.as_str())
            .then(a.instance.cmp(&b.instance))
    });
    Ok(states)
}

fn observed_to_state(target: ObservedTarget) -> StateRecord {
    StateRecord {
        playground: target.playground,
        instance: target.instance,
        namespace_prefix: Some(target.namespace_prefix),
        log_path: target.log_path,
        session_id: target.started_at_ms.to_string(),
        pid: target.pid,
        started_at_ms: target.started_at_ms,
        binary_path: target.binary_path,
    }
}

fn discover_targets(playground: PlaygroundName) -> Result<Vec<ObservedTarget>> {
    #[cfg(target_os = "windows")]
    {
        discover_targets_windows(playground)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = playground;
        Ok(Vec::new())
    }
}

#[cfg(target_os = "windows")]
fn discover_targets_windows(playground: PlaygroundName) -> Result<Vec<ObservedTarget>> {
    let script = format!(
        "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process -Filter \"Name = '{}'\" | ForEach-Object {{ [Console]::WriteLine('{{0}}\t{{1}}', $_.ProcessId, $_.CommandLine) }}",
        playground.binary_name_with_suffix(),
    );
    let result = run_command_with_timeout(
        {
            let mut command = Command::new("powershell");
            command.args(["-NoProfile", "-Command", &script]);
            command
        },
        Duration::from_millis(PROCESS_DISCOVERY_TIMEOUT_MS),
    )?;

    if result.timed_out {
        bail!(
            "process discovery timed out after {}ms",
            PROCESS_DISCOVERY_TIMEOUT_MS
        );
    }

    if !result.output.status.success() {
        bail!(
            "process discovery failed: {}",
            String::from_utf8_lossy(&result.output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&result.output.stdout);
    let mut targets = Vec::new();

    for line in stdout.lines() {
        let Some((pid_text, command_line)) = line.split_once('\t') else {
            continue;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            continue;
        };
        let Some(observed) = parse_observed_target(playground, pid, command_line) else {
            continue;
        };
        targets.push(observed);
    }

    Ok(targets)
}

fn parse_observed_target(
    playground: PlaygroundName,
    pid: u32,
    command_line: &str,
) -> Option<ObservedTarget> {
    let tokens = split_command_line(command_line);
    if !tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case("serve"))
    {
        return None;
    }

    let instance = command_arg_value(&tokens, "--instance")?;
    let namespace_prefix = command_arg_value(&tokens, "--namespace-prefix")?;
    let log_path = PathBuf::from(command_arg_value(&tokens, "--log-path")?);
    let session_id = log_path.file_stem()?.to_string_lossy().to_string();
    let started_at_ms = session_id.parse::<u128>().ok()?;
    let binary_path = tokens.first().map(PathBuf::from)?;

    Some(ObservedTarget {
        playground,
        instance,
        namespace_prefix,
        log_path,
        pid,
        started_at_ms,
        binary_path,
    })
}

fn split_command_line(command_line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in command_line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn command_arg_value(tokens: &[String], flag: &str) -> Option<String> {
    tokens
        .windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn prune_logs() -> Result<usize> {
    let entries = match fs::read_dir(log_dir()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error).with_context(|| format!("read log dir {}", log_dir().display()))
        }
    };

    let mut removed = 0usize;
    for entry in entries {
        let entry = entry.context("read log dir entry")?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("log") {
            fs::remove_file(&path).with_context(|| format!("remove log {}", path.display()))?;
            removed += 1;
        }
    }

    Ok(removed)
}

fn prune_locks() -> Result<usize> {
    let entries = match fs::read_dir(lock_dir()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error).with_context(|| format!("read lock dir {}", lock_dir().display()))
        }
    };

    let mut removed = 0usize;
    for entry in entries {
        let entry = entry.context("read lock dir entry")?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
            continue;
        }

        if clear_stale_namespace_lock(&path)? {
            removed += 1;
        }
    }

    Ok(removed)
}

fn prune_state_dirs() -> Result<usize> {
    let path = state_dir();
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(1),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("remove state dir {}", path.display())),
    }
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

fn dev_namespace_prefix() -> String {
    env::var(STUI_DEV_IPC_NAMESPACE_PREFIX_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "default".to_string())
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

    fn binary_name_with_suffix(self) -> String {
        format!("{}{}", self.binary_name(), env::consts::EXE_SUFFIX)
    }

    fn binary_override_env(self) -> &'static str {
        match self {
            Self::BlackBox => "STUI_DEV_BLACK_BOX_BIN",
        }
    }
}

impl Action {
    fn requires_namespace_lock(&self) -> bool {
        !matches!(self, Self::Targets)
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Targets => "targets",
            Self::Prune => "prune",
            Self::Start(_) => "start",
            Self::Stop(_) => "stop",
            Self::Restart(_) => "restart",
            Self::State(_) => "state",
            Self::Inspect(_) => "inspect",
            Self::Events(_) => "events",
            Self::Logs(_) => "logs",
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

impl Drop for NamespaceOpGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choose_candidate(
        repo_binary: &Path,
        current_exe: Option<&Path>,
        env_override: Option<&Path>,
        path_binary: Option<&Path>,
    ) -> Vec<PathBuf> {
        let sibling_binary =
            current_exe.and_then(|path| sibling_binary_path(path, PlaygroundName::BlackBox));
        let prefer_repo_debug = current_exe.is_some_and(is_repo_target_binary);
        let mut candidates = Vec::new();

        if let Some(path) = env_override {
            candidates.push(path.to_path_buf());
        }

        if prefer_repo_debug {
            candidates.push(repo_binary.to_path_buf());
            if let Some(path) = sibling_binary {
                candidates.push(path);
            }
            if let Some(path) = path_binary {
                candidates.push(path.to_path_buf());
            }
        } else {
            if let Some(path) = sibling_binary {
                candidates.push(path);
            }
            if let Some(path) = path_binary {
                candidates.push(path.to_path_buf());
            }
            candidates.push(repo_binary.to_path_buf());
        }

        candidates.dedup();
        candidates
    }

    #[test]
    fn dev_path_prefers_repo_debug_binary() {
        let repo_binary = repo_root().join("target").join("debug").join(format!(
            "stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));
        let current_exe = repo_root()
            .join("target")
            .join("debug")
            .join(format!("stui-dev{}", env::consts::EXE_SUFFIX));
        let path_binary = PathBuf::from(format!(
            r"C:\Users\Nexu\.cargo\bin\stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));

        let candidates =
            choose_candidate(&repo_binary, Some(&current_exe), None, Some(&path_binary));

        assert_eq!(candidates.first(), Some(&repo_binary));
    }

    #[test]
    fn installed_path_prefers_sibling_or_path_before_repo_debug() {
        let repo_binary = repo_root().join("target").join("debug").join(format!(
            "stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));
        let current_exe = PathBuf::from(format!(
            r"C:\Users\Nexu\.cargo\bin\stui-dev{}",
            env::consts::EXE_SUFFIX
        ));
        let sibling_binary = PathBuf::from(format!(
            r"C:\Users\Nexu\.cargo\bin\stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));

        let candidates = choose_candidate(&repo_binary, Some(&current_exe), None, None);

        assert_eq!(candidates.first(), Some(&sibling_binary));
        assert_eq!(candidates.last(), Some(&repo_binary));
    }

    #[test]
    fn explicit_override_has_highest_priority() {
        let repo_binary = repo_root().join("target").join("debug").join(format!(
            "stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));
        let current_exe = repo_root()
            .join("target")
            .join("debug")
            .join(format!("stui-dev{}", env::consts::EXE_SUFFIX));
        let override_binary = PathBuf::from(format!(
            r"D:\stui\stui-playground-black-box{}",
            env::consts::EXE_SUFFIX
        ));

        let candidates = choose_candidate(
            &repo_binary,
            Some(&current_exe),
            Some(&override_binary),
            None,
        );

        assert_eq!(candidates.first(), Some(&override_binary));
    }
}
