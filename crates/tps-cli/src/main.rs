use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use tps_core::JumpTarget;
use tps_store::Store;
use tps_tmux::TmuxClient;

#[derive(Debug, Parser)]
#[command(name = "tmux-pane-switcher")]
#[command(about = "Rank and jump between tmux panes based on recent activity")]
struct Cli {
    #[arg(long)]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Observe,
    ListRanked {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
        format: OutputFormat,
        #[arg(long)]
        server_key: Option<String>,
    },
    Jump {
        #[arg(long)]
        server_key: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        window_id: Option<String>,
        #[arg(long)]
        pane_id: Option<String>,
    },
    Doctor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Table,
    TmuxTarget,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db_path.unwrap_or_else(default_db_path);
    let tmux = TmuxClient::new();

    match cli.command {
        Commands::Observe => {
            let snapshots = tmux.collect_snapshot()?;
            let mut store = Store::open(&db_path)?;
            let count = store.upsert_snapshots(&snapshots)?;
            println!("observed {count} panes into {}", db_path.display());
        }
        Commands::ListRanked {
            limit,
            format,
            server_key,
        } => {
            let server_key = resolve_server_key(server_key, &tmux)?;
            let store = Store::open(&db_path)?;
            let panes = store.list_ranked(&server_key, limit)?;

            match format {
                OutputFormat::Table => print_ranked_table(&panes),
                OutputFormat::TmuxTarget => {
                    for pane in panes {
                        println!("{}", pane.tmux_target());
                    }
                }
            }
        }
        Commands::Jump {
            server_key,
            session_id,
            window_id,
            pane_id,
        } => {
            let target = match (session_id, window_id, pane_id) {
                (Some(session_id), Some(window_id), Some(pane_id)) => JumpTarget {
                    server_key: resolve_server_key(server_key, &tmux)?,
                    session_id,
                    window_id,
                    pane_id,
                },
                (None, None, None) => {
                    let server_key = resolve_server_key(server_key, &tmux)?;
                    let store = Store::open(&db_path)?;
                    store
                        .top_ranked(&server_key)?
                        .ok_or_else(|| anyhow::anyhow!("no ranked tmux pane available"))?
                }
                _ => bail!("pass all of --session-id, --window-id, and --pane-id, or none of them"),
            };

            tmux.jump(&target)?;
        }
        Commands::Doctor => {
            let version = tmux.check_tmux().context("tmux is not available")?;
            let server_key = tmux
                .server_key()
                .context("could not resolve current tmux server")?;
            let store = Store::open(&db_path)?;
            let pane_count = store.live_pane_count(&server_key).unwrap_or(0);

            println!("tmux: {version}");
            println!("server: {server_key}");
            println!("database: {}", db_path.display());
            println!("live_panes: {pane_count}");
        }
    }

    Ok(())
}

fn print_ranked_table(panes: &[tps_core::RankedPane]) {
    for pane in panes {
        println!(
            "{}\t{}\t{}.{}\t{}\t{}\t{}",
            pane.tmux_target(),
            pane.current_command,
            pane.window_index,
            pane.pane_index,
            pane.session_name,
            pane.window_name,
            pane.display_title
        );
    }
}

fn resolve_server_key(server_key: Option<String>, tmux: &TmuxClient) -> Result<String> {
    match server_key {
        Some(server_key) => Ok(server_key),
        None => tmux.server_key(),
    }
}

fn default_db_path() -> PathBuf {
    if let Ok(path) = env::var("TPS_DB_PATH") {
        return PathBuf::from(path);
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("tmux-pane-switcher")
            .join("state.sqlite");
    }

    PathBuf::from(".tmux-pane-switcher.sqlite")
}
