use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;
use tracing::info;

pub mod config;
pub mod db;
pub mod diff;
pub mod forges;
pub mod mcp;
pub mod providers;
pub mod reviewer;
pub mod scrubber;
pub mod server;
pub mod skills;
pub mod synthesizer;

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

        /// Directory containing static web dashboard assets (defaults to ./web/build)
        #[arg(long, env = "REVUE_STATIC_DIR")]
        static_dir: Option<String>,

        /// Database URL (sqlite:///path/to/revue.db or postgres://user:pass@host:5432/revue)
        #[arg(long, env = "DATABASE_URL")]
        database_url: Option<String>,
    },

    /// Run as an MCP (Model Context Protocol) stdio server for AI coding agents
    Mcp,

    /// Synthesize a Semgrep rule from a code review finding (AI-Learns-Once)
    LearnRule {
        /// Target repository (e.g. kleinbem/nix-config)
        #[arg(short, long)]
        repo: String,

        /// File path where the finding occurred
        #[arg(short, long)]
        file: String,

        /// Finding title
        #[arg(short, long)]
        title: String,

        /// Finding comment/explanation
        #[arg(short, long)]
        comment: String,

        /// Relevant code snippet
        #[arg(short, long)]
        snippet: Option<String>,

        /// Do not append to .semgrep/learned-rules.yaml; print to stdout only
        #[arg(long)]
        no_commit: bool,
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
        Commands::Pr {
            index,
            repo,
            dry_run,
        } => {
            reviewer::run_pr_review(index, repo.as_deref(), dry_run, cli.config.as_deref()).await?;
        }
        Commands::Daemon {
            listen,
            webhook_secret,
            static_dir,
            database_url,
        } => {
            info!("Starting revue daemon");
            server::run_daemon(
                &listen,
                cli.config.as_deref(),
                webhook_secret.as_deref(),
                static_dir.as_deref(),
                database_url.as_deref(),
            )
            .await?;
        }
        Commands::Mcp => {
            mcp::run_mcp_server(cli.config.as_deref()).await?;
        }
        Commands::LearnRule {
            repo,
            file,
            title,
            comment,
            snippet,
            no_commit,
        } => {
            let config = config::RevueConfig::find_and_load(cli.config.as_deref())?;
            let chain = reviewer::build_provider_chain(&config);
            let req = synthesizer::RuleSynthesizeRequest {
                repo,
                file,
                line: None,
                title,
                comment,
                snippet,
                commit: !no_commit,
            };
            let resp = synthesizer::synthesize_rule(req, &chain).await;
            if resp.success {
                println!("=== Synthesized Semgrep Rule: {} ===", resp.rule_id);
                println!("{}", resp.yaml);
                if let Some(path) = resp.saved_path {
                    println!("\nPersisted rule to {}", path);
                }
            } else {
                eprintln!(
                    "Failed to synthesize valid Semgrep rule: {}",
                    resp.error
                        .unwrap_or_else(|| "Unknown validation error".to_string())
                );
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
