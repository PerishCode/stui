use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "stui-pack")]
#[command(about = "Packaging-oriented helper for stui")]
struct Cli {}

fn main() -> Result<()> {
    let _ = Cli::parse();
    println!("stui-pack is reserved for future packaging flows.");
    Ok(())
}
