use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use stui_playground_black_box::{
    BlackBoxDebugBehavior, BlackBoxPlayground, BlackBoxPlaygroundConfig, DebugSnapshotFormat,
};

#[derive(Debug, Parser)]
#[command(name = "stui-playground-black-box")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve(ServeArgs),
    Request(RequestArgs),
    Run(RunArgs),
    Snapshot(SnapshotArgs),
}

#[derive(Debug, clap::Args)]
struct ServeArgs {
    #[arg(long, default_value = "default")]
    instance: String,
    #[arg(long, default_value = "default")]
    namespace_prefix: String,
    #[arg(long)]
    log_path: Option<String>,
    #[arg(long, value_enum)]
    behavior: Option<BehaviorArg>,
}

#[derive(Debug, clap::Args)]
struct RequestArgs {
    #[arg(long, default_value = "default")]
    instance: String,
    #[arg(long, default_value = "default")]
    namespace_prefix: String,
    #[arg(long, value_enum, default_value_t = SurfaceArg::Control)]
    surface: SurfaceArg,
    request: String,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    #[arg(long, value_enum)]
    behavior: Option<BehaviorArg>,
}

#[derive(Debug, clap::Args)]
struct SnapshotArgs {
    #[arg(long, value_enum)]
    behavior: Option<BehaviorArg>,
    #[arg(long, value_enum, default_value_t = SnapshotFormatArg::Text)]
    format: SnapshotFormatArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BehaviorArg {
    Booting,
    Idle,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SurfaceArg {
    Control,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum SnapshotFormatArg {
    #[default]
    Text,
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve(args) => {
            let playground = playground_config(args.behavior, false, SnapshotFormatArg::Text);
            playground.serve_ipc(Some(&args.namespace_prefix), &args.instance)
        }
        Command::Request(args) => {
            let playground = BlackBoxPlayground::load(BlackBoxPlaygroundConfig::default());
            let response = match args.surface {
                SurfaceArg::Control => playground.send_ipc_request(
                    Some(&args.namespace_prefix),
                    &args.instance,
                    &args.request,
                )?,
                SurfaceArg::Events => playground.send_ipc_event_request(
                    Some(&args.namespace_prefix),
                    &args.instance,
                    &args.request,
                )?,
            };
            println!("{response}");
            Ok(())
        }
        Command::Run(args) => {
            let playground = playground_config(args.behavior, false, SnapshotFormatArg::Text);
            println!("{}", playground.summary());
            let report = playground.run()?;
            println!(
                "exit_reason={:?} presented_at_least_once={}",
                report.exit_reason, report.presented_at_least_once
            );
            Ok(())
        }
        Command::Snapshot(args) => {
            let playground = playground_config(args.behavior, true, args.format);
            println!("{}", playground.debug_snapshot_output());
            Ok(())
        }
    }
}

fn playground_config(
    behavior: Option<BehaviorArg>,
    dump_snapshot: bool,
    format: SnapshotFormatArg,
) -> BlackBoxPlayground {
    BlackBoxPlayground::load(BlackBoxPlaygroundConfig {
        forced_behavior: behavior.map(map_behavior),
        dump_snapshot,
        snapshot_format: match format {
            SnapshotFormatArg::Text => DebugSnapshotFormat::Text,
            SnapshotFormatArg::Json => DebugSnapshotFormat::Json,
        },
    })
}

fn map_behavior(behavior: BehaviorArg) -> BlackBoxDebugBehavior {
    match behavior {
        BehaviorArg::Booting => BlackBoxDebugBehavior::Booting,
        BehaviorArg::Idle => BlackBoxDebugBehavior::Idle,
        BehaviorArg::Closing => BlackBoxDebugBehavior::Closing,
    }
}
