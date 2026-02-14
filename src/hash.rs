use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{Context, Result};
use xxhash_rust::xxh3::Xxh3;

const BUFFER_SIZE: usize = 64 * 1024; // 64KB buffer for streaming

/// Compute xxHash3-64 hash of a file, returning a 16-char hex string
pub fn compute_file_hash(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut hasher = Xxh3::new();

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:016x}", hasher.digest()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_compute_hash_known_content() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");

        // Write known content
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"hello world").unwrap();
        drop(file);

        let hash = compute_file_hash(&file_path).unwrap();

        // Hash should be 16 hex chars
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // Verify deterministic - same content = same hash
        let hash2 = compute_file_hash(&file_path).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_compute_hash_different_content() {
        let temp = TempDir::new().unwrap();

        let file1_path = temp.path().join("file1.txt");
        let file2_path = temp.path().join("file2.txt");

        std::fs::write(&file1_path, b"content A").unwrap();
        std::fs::write(&file2_path, b"content B").unwrap();

        let hash1 = compute_file_hash(&file1_path).unwrap();
        let hash2 = compute_file_hash(&file2_path).unwrap();

        // Different content should produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_hash_empty_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("empty.txt");

        std::fs::write(&file_path, b"").unwrap();

        let hash = compute_file_hash(&file_path).unwrap();

        // Empty file should still produce a valid hash
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn test_compute_hash_nonexistent_file() {
        let result = compute_file_hash(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
    }
}
