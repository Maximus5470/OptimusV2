mod commands;

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "optimus-cli")]
#[command(about = "Optimus CLI - Manage languages, deployments, and configurations", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new programming language to Optimus
    AddLang {
        /// Language name (e.g., java, cpp, go)
        #[arg(short, long)]
        name: String,

        /// File extension (e.g., java, cpp, go)
        #[arg(short, long)]
        ext: String,

        /// Language version (e.g., 17, 20, 1.21)
        #[arg(short, long, default_value = "latest")]
        version: String,

        /// Base Docker image (optional)
        #[arg(short, long)]
        base_image: Option<String>,

        /// Command to run (e.g., java, g++, go)
        #[arg(short, long)]
        command: Option<String>,

        /// Queue name (defaults to optimus:queue:{language})
        #[arg(short, long)]
        queue: Option<String>,

        /// Memory limit in MB
        #[arg(short, long, default_value = "256")]
        memory: u32,

        /// CPU limit
        #[arg(long, default_value = "0.5")]
        cpu: f32,

        /// Skip Docker image build
        #[arg(long)]
        skip_docker: bool,
    },

    /// Remove a programming language from Optimus
    RemoveLang {
        /// Language name to remove
        #[arg(short, long)]
        name: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List all configured languages
    ListLangs,

    /// Build Docker image for a language
    BuildImage {
        /// Language name
        #[arg(short, long)]
        name: String,

        /// Skip build cache
        #[arg(long, default_value = "false")]
        no_cache: bool,
    },

    /// Render Kubernetes manifests from templates
    RenderK8s,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::AddLang {
            name,
            ext,
            version,
            base_image,
            command,
            queue,
            memory,
            cpu,
            skip_docker,
        } => {
            commands::add_language(
                &name,
                &ext,
                &version,
                base_image.as_deref(),
                command.as_deref(),
                queue.as_deref(),
                memory,
                cpu,
                !skip_docker,
            ).await?;
        }
        Commands::RemoveLang { name, yes } => {
            commands::remove_language(&name, yes).await?;
        }
        Commands::ListLangs => {
            commands::list_languages().await?;
        }
        Commands::BuildImage { name, no_cache } => {
            commands::build_docker_image(&name, no_cache).await?;
        }
        Commands::RenderK8s => {
            commands::render_k8s_manifests().await?;
        }
    }

    Ok(())
}
