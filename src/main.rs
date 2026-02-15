use anyhow::Result;
use clap::{Parser, Subcommand};
use picman::cli::{run_init, run_list, run_rate, run_sync, run_tag, ListOptions, TagOptions};
use picman::tui::run_tui;
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
        /// Auto-tag image orientation (landscape/portrait)
        #[arg(long)]
        orientation: bool,
    },
    /// Find duplicate files
    Dupes {
        /// Scope to subdirectory
        subdir: Option<PathBuf>,
    },
    /// List files matching criteria
    List {
        /// Path to library root
        path: PathBuf,
        /// Minimum rating (1-5)
        #[arg(long)]
        rating: Option<i32>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Rate a file (1-5 stars)
    Rate {
        /// Path to library root
        library: PathBuf,
        /// File to rate (relative to library)
        file: PathBuf,
        /// Rating 1-5 stars (omit to clear)
        rating: Option<i32>,
    },
    /// Add or remove tags from a file
    Tag {
        /// Path to library root
        library: PathBuf,
        /// File to tag (relative to library)
        file: PathBuf,
        /// Tags to add
        #[arg(short, long)]
        add: Vec<String>,
        /// Tags to remove
        #[arg(short, long)]
        remove: Vec<String>,
        /// List current tags
        #[arg(short, long)]
        list: bool,
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
        Some(Commands::Sync { path, hash, orientation }) => {
            let stats = run_sync(&path, hash, orientation)?;
            println!(
                "Synced: +{} -{} directories, +{} -{} ~{} files",
                stats.directories_added,
                stats.directories_removed,
                stats.files_added,
                stats.files_removed,
                stats.files_modified
            );
            if hash {
                println!(
                    "Hashed: {} files ({} errors)",
                    stats.files_hashed, stats.hash_errors
                );
            }
            if orientation {
                println!("Orientation tagged: {} files", stats.orientation_tagged);
            }
        }
        Some(Commands::Dupes { subdir }) => {
            println!("Finding duplicates...");
            if let Some(sub) = subdir {
                println!("  Scoped to: {}", sub.display());
            }
            // TODO: Implement dupes
        }
        Some(Commands::List { path, rating, tag }) => {
            let options = ListOptions {
                min_rating: rating,
                tag,
            };
            let files = run_list(&path, options)?;
            for file in &files {
                let rating_str = file
                    .rating
                    .map(|r| format!(" [{}]", "*".repeat(r as usize)))
                    .unwrap_or_default();
                let tags_str = if file.tags.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", file.tags.join(", "))
                };
                println!("{}{}{}", file.path, rating_str, tags_str);
            }
            println!("{} files", files.len());
        }
        Some(Commands::Rate {
            library,
            file,
            rating,
        }) => {
            run_rate(&library, &file, rating)?;
            match rating {
                Some(r) => println!("Rated {} with {} stars", file.display(), r),
                None => println!("Cleared rating from {}", file.display()),
            }
        }
        Some(Commands::Tag {
            library,
            file,
            add,
            remove,
            list,
        }) => {
            let options = TagOptions { add, remove, list };
            let tags = run_tag(&library, &file, options)?;
            if tags.is_empty() {
                println!("{}: no tags", file.display());
            } else {
                println!("{}: {}", file.display(), tags.join(", "));
            }
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
            run_tui(&library)?;
        }
    }

    Ok(())
}
