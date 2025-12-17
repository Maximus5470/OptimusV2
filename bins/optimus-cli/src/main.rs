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

        /// Queue name (defaults to jobs:{language})
        #[arg(short, long)]
        queue: Option<String>,

        /// Memory limit in MB
        #[arg(short, long, default_value = "256")]
        memory: u32,

        /// CPU limit
        #[arg(long, default_value = "0.5")]
        cpu: f32,
    },

    /// Initialize a new Optimus project
    Init {
        /// Project path
        #[arg(short, long, default_value = ".")]
        path: String,
    },
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
            ).await?;
        }
        Commands::Init { path } => {
            commands::init_project(&path).await?;
        }
    }

    Ok(())
}
