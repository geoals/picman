use std::path::Path;

use anyhow::{Context, Result};

use crate::db::Database;

use super::init::DB_FILENAME;

/// Run the repair command: fix directory parent_id values based on paths
pub fn run_repair(library_path: &Path) -> Result<usize> {
    let library_path = library_path
        .canonicalize()
        .with_context(|| format!("Library path does not exist: {}", library_path.display()))?;

    let db_path = library_path.join(DB_FILENAME);
    if !db_path.exists() {
        anyhow::bail!(
            "No database found at {}. Run 'picman init' first.",
            db_path.display()
        );
    }

    let db = Database::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

    db.repair_directory_parents()
}
