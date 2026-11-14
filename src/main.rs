use anyhow::{bail, Result};
use clap::Parser;
use std::{io::IsTerminal, time::Duration};

mod collector;
mod direction;
mod tui;

#[derive(Parser, Debug)]
#[command(name = "netview", about = "NetView — Native TCP connection viewer")]
struct Args {
    /// Refresh the interactive table every N seconds (default: 2).
    #[arg(short, long, value_name = "SECONDS")]
    watch: Option<u64>,
    /// Print one plain-text snapshot instead of opening the interactive interface.
    #[arg(long, conflicts_with = "watch")]
    snapshot: bool,

    /// Only show listening sockets.
    #[arg(short, long)]
    listening: bool,

    /// Only show connections whose process name or path contains this text.
    #[arg(short, long, value_name = "TEXT")]
    process: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if args.snapshot || !interactive {
        if args.watch.is_some() {
            bail!("--watch requires an interactive terminal; use --snapshot for plain output");
        }
        let mut app = tui::App::new(args.listening, args.process.unwrap_or_default());
        app.set_entries(collector::tcp_connections()?);
        print_table(&app);
        return Ok(());
    }
    tui::run(
        args.listening,
        args.process.unwrap_or_default(),
        Duration::from_secs(args.watch.unwrap_or(2).max(1)),
    )
}

fn print_table(app: &tui::App) {
    println!(
        "{:<22} {:<22} {:<13} {:>7} {:<10} PROCESS",
        "LOCAL", "REMOTE", "STATE", "PID", "DIRECTION"
    );
    println!("{}", "-".repeat(116));
    for entry in &app.visible {
        println!(
            "{:<22} {:<22} {:<13} {:>7} {:<10} {}",
            entry.local,
            entry.remote,
            entry.state,
            entry.pid,
            app.direction(entry).label(),
            entry.process
        );
    }
    println!("\n{} TCP entries", app.visible.len());
}
