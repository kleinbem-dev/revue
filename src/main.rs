use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;
use tracing::info;

pub mod config;
pub mod diff;
pub mod forges;
pub mod providers;
pub mod reviewer;
pub mod server;

#[derive(Parser, Debug)]
#[command(
    name = "revue",
    about = "Self-hosted, forge-agnostic AI code review engine with multi-provider cascade (Gemini, Claude, Ollama)",
    version
)]
struct Cli {
    /// Explicit path to configuration file (defaults to .revue.yaml or revue.yaml)
    #[arg(short, long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new .revue.yaml configuration file in the current directory
    Init {
        /// Overwrite existing config file if present
        #[arg(short, long)]
        force: bool,
    },

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

        /// Webhook HMAC secret for signature verification (or set REVUE_WEBHOOK_SECRET)
        #[arg(long, env = "REVUE_WEBHOOK_SECRET")]
        webhook_secret: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "revue=info,nix_revue=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { force } => {
            let target = Path::new(".revue.yaml");
            if target.exists() && !force {
                eprintln!(".revue.yaml already exists. Use --force to overwrite.");
                return Ok(());
            }
            std::fs::write(target, config::RevueConfig::default_yaml_template())?;
            println!("Created .revue.yaml with default configuration.");
        }
        Commands::Diff { base, format } => {
            reviewer::run_diff_review(&base, &format, cli.config.as_deref()).await?;
        }
        Commands::Pr { index, repo, dry_run } => {
            reviewer::run_pr_review(index, repo.as_deref(), dry_run, cli.config.as_deref()).await?;
        }
        Commands::Daemon { listen, webhook_secret } => {
            info!("Starting revue daemon");
            server::run_daemon(&listen, cli.config.as_deref(), webhook_secret.as_deref()).await?;
        }
    }

    Ok(())
}
