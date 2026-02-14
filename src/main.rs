use anyhow::Result;
use clap::{Parser, Subcommand};
use picman::cli::{run_init, run_sync};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "picman")]
#[command(about = "Photo library management tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to library (launches TUI if no subcommand)
    #[arg(global = true)]
    library: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize database for a library
    Init {
        /// Path to library root
        path: PathBuf,
    },
    /// Sync database with filesystem changes
    Sync {
        /// Path to library root (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Recompute hashes for changed files
        #[arg(long)]
        hash: bool,
    },
    /// Find duplicate files
    Dupes {
        /// Scope to subdirectory
        subdir: Option<PathBuf>,
    },
    /// List files matching criteria
    List {
        /// Minimum rating (e.g., "8+")
        #[arg(long)]
        rating: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Create symlink view of filtered files
    View {
        /// Minimum rating (e.g., "8+")
        #[arg(long)]
        rating: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Output directory for symlinks
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Export metadata to sidecar JSON files
    Export,
    /// Import metadata from sidecar JSON files
    Import,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init { path }) => {
            println!("Initializing library at: {}", path.display());
            let stats = run_init(&path)?;
            println!(
                "Initialized: {} directories, {} files ({} images, {} videos)",
                stats.directories, stats.files, stats.images, stats.videos
            );
        }
        Some(Commands::Sync { path, hash }) => {
            let stats = run_sync(&path)?;
            println!(
                "Synced: +{} -{} directories, +{} -{} ~{} files",
                stats.directories_added,
                stats.directories_removed,
                stats.files_added,
                stats.files_removed,
                stats.files_modified
            );
            if hash {
                println!("Hash recomputation not yet implemented");
            }
        }
        Some(Commands::Dupes { subdir }) => {
            println!("Finding duplicates...");
            if let Some(sub) = subdir {
                println!("  Scoped to: {}", sub.display());
            }
            // TODO: Implement dupes
        }
        Some(Commands::List { rating, tag }) => {
            println!("Listing files...");
            // TODO: Implement list
            let _ = (rating, tag);
        }
        Some(Commands::View { rating, tag, output }) => {
            println!("Creating view at: {}", output.display());
            // TODO: Implement view
            let _ = (rating, tag);
        }
        Some(Commands::Export) => {
            println!("Exporting metadata...");
            // TODO: Implement export
        }
        Some(Commands::Import) => {
            println!("Importing metadata...");
            // TODO: Implement import
        }
        None => {
            // Launch TUI
            let library = cli.library.unwrap_or_else(|| PathBuf::from("."));
            println!("Launching TUI for: {}", library.display());
            // TODO: Implement TUI
        }
    }

    Ok(())
}
