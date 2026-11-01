use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

pub mod config;
pub mod diff;
pub mod forges;
pub mod providers;
pub mod reviewer;
pub mod server;

#[derive(Parser, Debug)]
#[command(
    name = "nix-revue",
    about = "Nix-native AI PR review engine with subscription fallback (Claude, Gemini, Ollama)",
    version
)]
struct Cli {
    #[arg(short, long, global = true, default_value = ".coderabbit.yaml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Review a local git / jj diff
    Diff {
        /// Base revision to compare against (e.g. main, HEAD~1)
        #[arg(default_value = "main")]
        base: String,

        /// Output format: markdown or json
        #[arg(short, long, default_value = "markdown")]
        format: String,
    },

    /// Review a Pull Request on Forgejo or GitHub
    Pr {
        /// Pull request index / number
        index: u64,

        /// Forge repository (e.g. kleinbem/nix-config)
        #[arg(short, long)]
        repo: Option<String>,

        /// Dry run: print review comments to terminal instead of posting
        #[arg(short, long)]
        dry_run: bool,
    },

    /// Run the webhook daemon for Forgejo and GitHub
    Daemon {
        /// Address to bind the HTTP webhook listener
        #[arg(short, long, default_value = "0.0.0.0:8383")]
        listen: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nix_revue=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    info!("Starting nix-revue");

    match cli.command {
        Commands::Diff { base, format } => {
            reviewer::run_diff_review(&base, &format, &cli.config).await?;
        }
        Commands::Pr { index, repo, dry_run } => {
            reviewer::run_pr_review(index, repo.as_deref(), dry_run, &cli.config).await?;
        }
        Commands::Daemon { listen } => {
            server::run_daemon(&listen, &cli.config).await?;
        }
    }

    Ok(())
}
